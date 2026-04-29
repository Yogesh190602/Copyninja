pub mod dbus;
pub mod session;
pub mod wayland;
pub mod x11;

use log::{debug, error, info, warn};
use session::SessionType;
use std::path::Path;
use std::time::Duration;

/// Parse a `text/uri-list` payload and, if any URI points to a local image
/// file, read its bytes and return them together with the detected MIME type.
/// Returns None if no usable image is found.
///
/// Handles the Nautilus/GNOME Files "Ctrl+C on an image file" case, where the
/// clipboard contains a `file://` URI instead of raw image bytes.
pub fn read_image_from_uri_list(raw: &[u8]) -> Option<(Vec<u8>, String)> {
    let text = std::str::from_utf8(raw).ok()?;
    for line in text.lines() {
        let line = line.trim();
        // text/uri-list may have comments starting with '#'
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let path = match line.strip_prefix("file://") {
            Some(p) => percent_decode(p),
            None => continue, // skip non-local URIs (http://, smb://, etc.)
        };
        let p = Path::new(&path);
        let mime = match p.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase()) {
            Some(ref e) if e == "png" => "image/png",
            Some(ref e) if e == "jpg" || e == "jpeg" => "image/jpeg",
            Some(ref e) if e == "webp" => "image/webp",
            Some(ref e) if e == "gif" => "image/gif",
            Some(ref e) if e == "bmp" => "image/bmp",
            _ => continue,
        };
        match std::fs::read(p) {
            Ok(data) if !data.is_empty() => return Some((data, mime.to_string())),
            Ok(_) => debug!("URI-list target {} is empty", path),
            Err(e) => debug!("Failed to read URI-list target {}: {}", path, e),
        }
    }
    None
}

/// Minimal RFC 3986 percent-decode for file URIs (handles %20 etc.).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn run(config: &crate::config::Config) {
    // Start sync watcher if enabled
    crate::sync::start_watcher(config.sync.clone());

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        if let Err(e) = run_async().await {
            error!("Daemon fatal error: {}", e);
            std::process::exit(1);
        }
    });
}

async fn run_async() -> anyhow::Result<()> {
    info!("CopyNinja daemon starting");

    let _conn = dbus::setup().await?;
    info!("D-Bus service registered: com.copyninja.Daemon");

    // Retry loop: try to start clipboard watcher for up to 5 minutes
    const MAX_RETRIES: u32 = 60;
    for attempt in 0..MAX_RETRIES {
        let session = session::detect();
        info!(
            "Session type: {:?} (attempt {}/{})",
            session,
            attempt + 1,
            MAX_RETRIES
        );

        match session {
            SessionType::Wayland => {
                // Try event-driven watcher first (wlroots compositors: Sway, Hyprland, etc.)
                match wayland::start().await {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        warn!("Wayland watcher failed: {}", e);
                        // Try polling fallback (GNOME/Mutter — lacks wlr-data-control)
                        info!("Trying Wayland polling fallback...");
                        match wayland::start_polling().await {
                            Ok(()) => return Ok(()),
                            Err(e) => {
                                warn!("Wayland polling failed: {}", e);
                                // Last resort: X11/XWayland
                                info!("Trying X11/XWayland fallback...");
                                match x11::start().await {
                                    Ok(()) => return Ok(()),
                                    Err(e) => warn!("X11 fallback failed: {}", e),
                                }
                            }
                        }
                    }
                }
            }
            SessionType::X11 => match x11::start().await {
                Ok(()) => return Ok(()),
                Err(e) => warn!("X11 watcher failed: {}", e),
            },
            SessionType::Unknown => {
                warn!("No graphical session detected yet");
            }
        }

        if attempt < MAX_RETRIES - 1 {
            info!("Retrying in 5 seconds...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    anyhow::bail!("Failed to start clipboard watcher after {} attempts", MAX_RETRIES)
}
