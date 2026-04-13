use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::content::ClipContent;

static STORAGE: OnceLock<Storage> = OnceLock::new();

/// Clipboard history entry. Supports both the new `content` field and legacy `text`/`preview` fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipEntry {
    /// New format: tagged content (text or image)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ClipContent>,
    /// Legacy text field (for backward compatibility with old JSON)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Legacy preview field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub hash: String,
    pub time: f64,
    #[serde(default)]
    pub pinned: bool,
}

impl ClipEntry {
    /// Get the resolved content, handling legacy entries that only have text/preview.
    pub fn resolved_content(&self) -> ClipContent {
        if let Some(ref content) = self.content {
            content.clone()
        } else if let Some(ref text) = self.text {
            ClipContent::Text {
                text: text.clone(),
                preview: self.preview.clone().unwrap_or_default(),
            }
        } else {
            ClipContent::Text {
                text: String::new(),
                preview: String::new(),
            }
        }
    }

    /// Create a new text entry.
    pub fn new_text(text: String, preview: String, hash: String, time: f64) -> Self {
        Self {
            content: Some(ClipContent::Text {
                text: text.clone(),
                preview: preview.clone(),
            }),
            text: Some(text),
            preview: Some(preview),
            hash,
            time,
            pinned: false,
        }
    }

    /// Create a new image entry.
    pub fn new_image(path: PathBuf, mime: String, hash: String, time: f64) -> Self {
        Self {
            content: Some(ClipContent::Image { path, mime }),
            text: None,
            preview: None,
            hash,
            time,
            pinned: false,
        }
    }

    /// Get preview text for display.
    pub fn display_preview(&self) -> String {
        self.resolved_content().preview().to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Storage {
    pub history_path: PathBuf,
    pub max_entries: usize,
    pub max_backups: usize,
    pub image_dir: PathBuf,
}

impl Storage {
    pub fn from_config(config: &Config) -> Self {
        // Ensure image directory exists
        let _ = fs::create_dir_all(&config.image_dir);
        Self {
            history_path: config.history_file.clone(),
            max_entries: config.max_entries,
            max_backups: config.max_backups,
            image_dir: config.image_dir.clone(),
        }
    }

    pub fn load_history(&self) -> Vec<ClipEntry> {
        load_history_from(&self.history_path, self.max_backups)
    }

    pub fn save_history(&self, history: &[ClipEntry]) -> Result<()> {
        rotate_backups(&self.history_path, self.max_backups);
        save_history_to(history, &self.history_path)
    }

    pub fn process_text(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }

        let hash = get_hash(text);
        let mut history = self.load_history();

        // Dedup: if entry exists, move it to top
        if let Some(pos) = history.iter().position(|e| e.hash == hash) {
            let mut entry = history.remove(pos);
            entry.time = now();
            history.insert(0, entry);
            if let Err(e) = self.save_history(&history) {
                log::error!("Failed to save history: {}", e);
            }
            return;
        }

        // Build preview: collapse newlines, truncate to 100 chars
        let preview: String = text
            .chars()
            .map(|c| if c == '\n' { ' ' } else { c })
            .take(100)
            .collect();

        let entry = ClipEntry::new_text(text.to_string(), preview, hash, now());
        history.insert(0, entry);
        self.prune_and_save(&mut history);
    }

    /// Process image data from clipboard — save to file, add to history.
    pub fn process_image(&self, data: &[u8], mime: &str) {
        if data.is_empty() {
            return;
        }

        let hash = get_hash_bytes(data);
        let mut history = self.load_history();

        // Dedup
        if let Some(pos) = history.iter().position(|e| e.hash == hash) {
            let mut entry = history.remove(pos);
            entry.time = now();
            history.insert(0, entry);
            if let Err(e) = self.save_history(&history) {
                log::error!("Failed to save history: {}", e);
            }
            return;
        }

        // Determine file extension from MIME
        let ext = match mime {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/webp" => "webp",
            "image/gif" => "gif",
            "image/bmp" => "bmp",
            _ => "png",
        };

        let image_path = self.image_dir.join(format!("{}.{}", hash, ext));

        // Save image to file
        if let Err(e) = fs::write(&image_path, data) {
            log::error!("Failed to save image: {}", e);
            return;
        }

        let entry = ClipEntry::new_image(image_path, mime.to_string(), hash, now());
        history.insert(0, entry);
        self.prune_and_save(&mut history);
    }

    fn prune_and_save(&self, history: &mut Vec<ClipEntry>) {
        if history.len() > self.max_entries {
            let pinned: Vec<ClipEntry> = history.iter().filter(|e| e.pinned).cloned().collect();
            let mut unpinned: Vec<ClipEntry> = history.iter().filter(|e| !e.pinned).cloned().collect();
            let unpinned_limit = self.max_entries.saturating_sub(pinned.len());

            // Clean up image files for evicted entries
            for evicted in unpinned.iter().skip(unpinned_limit) {
                if let Some(ClipContent::Image { path, .. }) = &evicted.content {
                    let _ = fs::remove_file(path);
                }
            }

            unpinned.truncate(unpinned_limit);
            *history = history
                .iter()
                .filter(|e| {
                    if e.pinned {
                        true
                    } else {
                        unpinned.iter().any(|u| u.hash == e.hash)
                    }
                })
                .cloned()
                .collect();
        }

        if let Err(e) = self.save_history(history) {
            log::error!("Failed to save history: {}", e);
        }
    }
}

/// Initialize the global storage from config. Call once at startup.
pub fn init(config: &Config) {
    let _ = STORAGE.set(Storage::from_config(config));
}

/// Get the global storage instance.
fn global() -> &'static Storage {
    STORAGE.get().expect("Storage not initialized — call storage::init() first")
}

pub fn get_hash(text: &str) -> String {
    get_hash_bytes(text.as_bytes())
}

pub fn get_hash_bytes(data: &[u8]) -> String {
    let digest = md5::compute(data);
    format!("{:x}", digest)[..12].to_string()
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

fn backup_path(path: &Path, n: usize) -> PathBuf {
    let name = format!("{}.bak.{}", path.file_name().unwrap().to_string_lossy(), n);
    path.with_file_name(name)
}

fn load_history_from(path: &Path, max_backups: usize) -> Vec<ClipEntry> {
    // Try main file first
    if let Ok(content) = fs::read_to_string(path) {
        match serde_json::from_str(&content) {
            Ok(entries) => return entries,
            Err(e) => {
                log::warn!("Corrupt history file {}: {}", path.display(), e);
            }
        }
    }

    // Try backups in order (newest first)
    for n in 1..=max_backups {
        let bak = backup_path(path, n);
        if let Ok(content) = fs::read_to_string(&bak) {
            match serde_json::from_str::<Vec<ClipEntry>>(&content) {
                Ok(entries) => {
                    log::warn!("Recovered history from backup {}", bak.display());
                    // Write recovered data back as main file
                    if let Err(e) = save_history_to(&entries, path) {
                        log::error!("Failed to restore main file from backup: {}", e);
                    }
                    return entries;
                }
                Err(e) => {
                    log::warn!("Backup {} also corrupt: {}", bak.display(), e);
                }
            }
        }
    }

    log::warn!("All history files corrupt or missing, starting fresh");
    Vec::new()
}

fn rotate_backups(path: &Path, max_backups: usize) {
    if max_backups == 0 {
        return;
    }
    // Shift .bak.2 → .bak.3, .bak.1 → .bak.2, etc.
    for n in (1..max_backups).rev() {
        let from = backup_path(path, n);
        let to = backup_path(path, n + 1);
        if from.exists() {
            let _ = fs::rename(&from, &to);
        }
    }
    // Copy current main file to .bak.1
    let bak1 = backup_path(path, 1);
    if path.exists() {
        let _ = fs::copy(path, &bak1);
    }
}

fn save_history_to(history: &[ClipEntry], path: &Path) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(history)?;
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// --- Backward-compatible public API (uses global storage) ---

pub fn load_history() -> Vec<ClipEntry> {
    global().load_history()
}

pub fn save_history(history: &[ClipEntry]) -> Result<()> {
    global().save_history(history)
}

pub fn process_text(text: &str) {
    global().process_text(text);
}

pub fn process_image(data: &[u8], mime: &str) {
    global().process_image(data, mime);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_storage(dir: &Path) -> Storage {
        Storage {
            history_path: dir.join("history.json"),
            max_entries: 5,
            max_backups: 3,
            image_dir: dir.join("images"),
        }
    }

    /// Helper to get the text from a ClipEntry for test assertions.
    fn entry_text(entry: &ClipEntry) -> &str {
        entry.text.as_deref().unwrap_or("")
    }

    fn entry_preview(entry: &ClipEntry) -> &str {
        entry.preview.as_deref().unwrap_or("")
    }

    #[test]
    fn test_hash_deterministic() {
        assert_eq!(get_hash("hello"), get_hash("hello"));
    }

    #[test]
    fn test_hash_length_12() {
        assert_eq!(get_hash("test").len(), 12);
    }

    #[test]
    fn test_hash_different_inputs() {
        assert_ne!(get_hash("hello"), get_hash("world"));
    }

    #[test]
    fn test_process_text_adds_entry() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path());
        s.process_text("hello world");
        let h = s.load_history();
        assert_eq!(h.len(), 1);
        assert_eq!(entry_text(&h[0]), "hello world");
    }

    #[test]
    fn test_process_text_ignores_empty() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path());
        s.process_text("");
        s.process_text("   ");
        assert_eq!(s.load_history().len(), 0);
    }

    #[test]
    fn test_dedup_moves_to_top() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path());
        s.process_text("first");
        s.process_text("second");
        s.process_text("first"); // duplicate
        let h = s.load_history();
        assert_eq!(h.len(), 2);
        assert_eq!(entry_text(&h[0]), "first");
        assert_eq!(entry_text(&h[1]), "second");
    }

    #[test]
    fn test_prune_at_max_entries() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path()); // max_entries = 5
        for i in 0..7 {
            s.process_text(&format!("entry {}", i));
        }
        let h = s.load_history();
        assert_eq!(h.len(), 5);
        // Newest should be first
        assert_eq!(entry_text(&h[0]), "entry 6");
    }

    #[test]
    fn test_prune_protects_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path()); // max_entries = 5
        // Add 5 entries
        for i in 0..5 {
            s.process_text(&format!("entry {}", i));
        }
        // Pin the oldest entry (entry 0, which is at position 4)
        let mut h = s.load_history();
        h[4].pinned = true;
        s.save_history(&h).unwrap();

        // Add 2 more — should evict unpinned, keep pinned
        s.process_text("new 1");
        s.process_text("new 2");
        let h = s.load_history();
        // Pinned entry must still be present
        assert!(h.iter().any(|e| entry_text(e) == "entry 0" && e.pinned));
        assert!(h.len() <= 5);
    }

    #[test]
    fn test_preview_collapses_newlines() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path());
        s.process_text("line1\nline2\nline3");
        let h = s.load_history();
        assert_eq!(entry_preview(&h[0]), "line1 line2 line3");
    }

    #[test]
    fn test_save_creates_backup() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path());
        s.process_text("first save");
        s.process_text("second save");
        let bak1 = backup_path(&s.history_path, 1);
        assert!(bak1.exists(), "backup .bak.1 should exist after second save");
    }

    #[test]
    fn test_corrupt_json_loads_from_backup() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path());
        // Two saves to create a backup (.bak.1 is made on the second save)
        s.process_text("good data");
        s.process_text("more data");

        // Corrupt the main file
        fs::write(&s.history_path, "NOT VALID JSON").unwrap();

        // Load should recover from backup
        let h = s.load_history();
        assert!(!h.is_empty(), "should recover from backup");
        assert!(h.iter().any(|e| entry_text(e) == "good data"));
    }

    #[test]
    fn test_all_corrupt_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path());

        // Write corrupt main file and corrupt backups
        fs::write(&s.history_path, "BAD").unwrap();
        for n in 1..=3 {
            fs::write(backup_path(&s.history_path, n), "BAD").unwrap();
        }

        let h = s.load_history();
        assert!(h.is_empty());
    }

    #[test]
    fn test_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let s = test_storage(dir.path());
        assert!(s.load_history().is_empty());
    }
}
