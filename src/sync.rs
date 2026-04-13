use crate::config::SyncConfig;
use crate::storage::{self, ClipEntry};
use log::{debug, error, info, warn};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Get or create a persistent device ID.
fn device_id() -> String {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"));
    let id_path = config_dir.join("copyninja").join("device_id");

    if let Ok(id) = fs::read_to_string(&id_path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let _ = fs::create_dir_all(id_path.parent().unwrap());
    let _ = fs::write(&id_path, &id);
    info!("Generated new device ID: {}", id);
    id
}

/// Export local history entries to the sync directory as individual JSON files.
pub fn export_to_sync_dir(sync_dir: &Path) {
    let entries_dir = sync_dir.join("entries");
    let _ = fs::create_dir_all(&entries_dir);

    let history = storage::load_history();
    let _device = device_id();

    for entry in &history {
        let entry_path = entries_dir.join(format!("{}.json", entry.hash));
        if !entry_path.exists() {
            if let Ok(json) = serde_json::to_string_pretty(entry) {
                if let Err(e) = fs::write(&entry_path, json) {
                    warn!("Failed to export entry {}: {}", entry.hash, e);
                }
            }
        }
    }
    debug!("Exported {} entries to sync dir", history.len());
}

/// Import entries from the sync directory into local history.
pub fn import_from_sync_dir(sync_dir: &Path) {
    let entries_dir = sync_dir.join("entries");
    let deleted_dir = sync_dir.join("deleted");

    if !entries_dir.exists() {
        return;
    }

    // Collect tombstones (deleted hashes)
    let mut deleted_hashes = std::collections::HashSet::new();
    if deleted_dir.exists() {
        if let Ok(entries) = fs::read_dir(&deleted_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    deleted_hashes.insert(name.to_string());
                }
            }
        }
    }

    let mut local_history = storage::load_history();
    let local_hashes: std::collections::HashSet<String> =
        local_history.iter().map(|e| e.hash.clone()).collect();

    let mut imported = 0;
    if let Ok(entries) = fs::read_dir(&entries_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&path) {
                match serde_json::from_str::<ClipEntry>(&content) {
                    Ok(clip_entry) => {
                        // Skip if already local or tombstoned
                        if local_hashes.contains(&clip_entry.hash) {
                            // Merge pinned state (OR logic)
                            if clip_entry.pinned {
                                if let Some(local) = local_history.iter_mut().find(|e| e.hash == clip_entry.hash) {
                                    if !local.pinned {
                                        local.pinned = true;
                                    }
                                }
                            }
                            continue;
                        }
                        if deleted_hashes.contains(&clip_entry.hash) {
                            continue;
                        }
                        local_history.push(clip_entry);
                        imported += 1;
                    }
                    Err(e) => {
                        debug!("Skipping invalid sync entry {}: {}", path.display(), e);
                    }
                }
            }
        }
    }

    if imported > 0 {
        // Sort by time (newest first)
        local_history.sort_by(|a, b| b.time.partial_cmp(&a.time).unwrap_or(std::cmp::Ordering::Equal));
        if let Err(e) = storage::save_history(&local_history) {
            error!("Failed to save after sync import: {}", e);
        }
        info!("Imported {} entries from sync dir", imported);
    }
}

/// Write a tombstone to prevent a deleted entry from being re-synced.
pub fn write_tombstone(sync_dir: &Path, hash: &str) {
    let deleted_dir = sync_dir.join("deleted");
    let _ = fs::create_dir_all(&deleted_dir);
    let _ = fs::write(deleted_dir.join(hash), "");

    // Also remove the entry file from sync dir
    let entry_path = sync_dir.join("entries").join(format!("{}.json", hash));
    let _ = fs::remove_file(entry_path);
}

/// Start watching the sync directory for changes from other devices.
/// Runs in a background thread, calls import when changes detected.
pub fn start_watcher(config: SyncConfig) {
    if !config.enabled || config.sync_dir.as_os_str().is_empty() {
        return;
    }

    let sync_dir = config.sync_dir.clone();
    let entries_dir = sync_dir.join("entries");
    let _ = fs::create_dir_all(&entries_dir);

    // Do an initial import
    import_from_sync_dir(&sync_dir);
    export_to_sync_dir(&sync_dir);

    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();

        let mut watcher = match RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            let _ = tx.send(());
                        }
                        _ => {}
                    }
                }
            },
            notify::Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to create sync watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&entries_dir, RecursiveMode::NonRecursive) {
            error!("Failed to watch sync dir {}: {}", entries_dir.display(), e);
            return;
        }

        info!("Sync watcher active on {}", entries_dir.display());

        // Debounce: wait for changes, then import after a short delay
        loop {
            match rx.recv() {
                Ok(()) => {
                    // Drain any additional events (debounce)
                    std::thread::sleep(Duration::from_millis(500));
                    while rx.try_recv().is_ok() {}

                    debug!("Sync dir changed, importing");
                    import_from_sync_dir(&sync_dir);
                    export_to_sync_dir(&sync_dir);
                }
                Err(_) => break,
            }
        }
    });
}
