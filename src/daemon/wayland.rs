use anyhow::{bail, Result};
use log::{debug, error, info};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Start monitoring the Wayland clipboard via `wl-paste --watch`.
/// This function runs indefinitely until the watcher process exits.
pub async fn start() -> Result<()> {
    info!("Starting Wayland clipboard watcher (wl-paste --watch)");

    let mut child = Command::new("wl-paste")
        .args(["--watch", "echo", ""])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Check if the process exits immediately (missing display or unsupported protocol).
    tokio::time::sleep(Duration::from_millis(500)).await;
    if let Ok(Some(status)) = child.try_wait() {
        let output = child.wait_with_output().await?;
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("wl-paste exited immediately with status: {}", status);
        }
        bail!(
            "wl-paste exited immediately with status: {} ({})",
            status,
            stderr
        );
    }

    info!("wl-paste --watch is running, monitoring clipboard changes");

    let stdout = child
        .stdout
        .take()
        .expect("stdout was piped but is missing");
    let mut lines = BufReader::new(stdout).lines();

    // Each line from wl-paste --watch means the clipboard changed
    while let Ok(Some(_line)) = lines.next_line().await {
        debug!("Clipboard change detected, fetching content");
        fetch_and_store().await;
    }

    // If we get here, the wl-paste process exited
    let status = child.wait().await?;
    error!("wl-paste --watch exited with status: {}", status);
    bail!("wl-paste --watch exited unexpectedly")
}

/// Polling fallback for compositors that lack wlr-data-control (e.g. GNOME/Mutter).
/// Polls `wl-paste` every 500 ms; calls fetch_and_store() when content changes.
/// Returns Err only if wl-paste is not installed at all.
pub async fn start_polling() -> Result<()> {
    // Verify wl-paste exists by doing a quick probe.
    let probe = Command::new("wl-paste")
        .args(["--list-types"])
        .output()
        .await;
    if let Err(e) = probe {
        bail!("wl-paste not found, cannot use Wayland polling: {}", e);
    }

    info!("Wayland --watch unavailable; falling back to wl-paste polling (500ms)");

    let mut last_hash: Option<md5::Digest> = None;

    loop {
        let output = Command::new("wl-paste")
            .args(["--no-newline"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                let hash = compute_hash(&out.stdout);
                if last_hash.as_ref() != Some(&hash) {
                    last_hash = Some(hash);
                    debug!("Clipboard change detected (polling), fetching content");
                    fetch_and_store().await;
                }
            }
            _ => {
                last_hash = None;
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Detect available MIME types and fetch the best one.
async fn fetch_and_store() {
    // Check what MIME types are available
    let types = match list_mime_types().await {
        Some(t) => t,
        None => return,
    };

    // Prefer images over text (user likely wants to capture the image)
    let image_mimes = ["image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp"];
    for mime in &image_mimes {
        if types.contains(&mime.to_string()) {
            match fetch_clipboard_bytes(mime).await {
                Ok(data) if !data.is_empty() => {
                    debug!("Captured image clipboard ({}, {} bytes)", mime, data.len());
                    crate::storage::process_image(&data, mime);
                    return;
                }
                _ => continue,
            }
        }
    }

    // File-manager image copy: Nautilus/Files puts text/uri-list (a file:// URI)
    // in the clipboard, not image bytes. If the URI points to a local image,
    // read the file and store its bytes as a proper image entry.
    if types.contains(&"text/uri-list".to_string()) {
        if let Ok(uri_list) = fetch_clipboard_bytes("text/uri-list").await {
            if let Some((data, mime)) = crate::daemon::read_image_from_uri_list(&uri_list) {
                debug!("Captured image from URI list ({}, {} bytes)", mime, data.len());
                crate::storage::process_image(&data, &mime);
                return;
            }
        }
    }

    // Fall back to text
    if types.iter().any(|t| t.contains("text/")) || types.contains(&"UTF8_STRING".to_string()) {
        match fetch_clipboard_text().await {
            Ok(text) if !text.trim().is_empty() => {
                crate::storage::process_text(&text);
            }
            _ => {}
        }
    }
}

async fn list_mime_types() -> Option<Vec<String>> {
    let output = Command::new("wl-paste")
        .args(["--list-types"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let types = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some(types)
}

async fn fetch_clipboard_text() -> Result<String> {
    let output = Command::new("wl-paste")
        .args(["--type", "text/plain", "--no-newline"])
        .output()
        .await?;

    if !output.status.success() {
        bail!("wl-paste failed with status: {}", output.status);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn fetch_clipboard_bytes(mime: &str) -> Result<Vec<u8>> {
    let output = Command::new("wl-paste")
        .args(["--type", mime])
        .output()
        .await?;

    if !output.status.success() {
        bail!("wl-paste --type {} failed with status: {}", mime, output.status);
    }

    Ok(output.stdout)
}

fn compute_hash(data: &[u8]) -> md5::Digest {
    md5::compute(data)
}
