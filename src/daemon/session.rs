use log::{debug, warn};
use std::env;
use std::fs;
use std::process::Command;

#[derive(Debug, Clone)]
pub enum SessionType {
    Wayland,
    X11,
    Unknown,
}

pub fn detect() -> SessionType {
    // Strategy 1: XDG_SESSION_TYPE
    if let Ok(session) = env::var("XDG_SESSION_TYPE") {
        match session.as_str() {
            "wayland" => {
                debug!("Detected Wayland via XDG_SESSION_TYPE");
                return SessionType::Wayland;
            }
            "x11" => {
                debug!("Detected X11 via XDG_SESSION_TYPE");
                return SessionType::X11;
            }
            _ => {}
        }
    }

    // Strategy 2: Display environment variables
    if env::var("WAYLAND_DISPLAY").is_ok() {
        debug!("Detected Wayland via WAYLAND_DISPLAY");
        return SessionType::Wayland;
    }
    if env::var("DISPLAY").is_ok() {
        debug!("Detected X11 via DISPLAY");
        return SessionType::X11;
    }

    // Strategy 3: Import environment from active graphical session via loginctl
    if let Some(session) = import_graphical_env() {
        return session;
    }

    // Strategy 4: Check running compositor processes
    if let Some(session) = detect_from_processes() {
        return session;
    }

    SessionType::Unknown
}

/// Query loginctl for active graphical sessions, import their display
/// environment variables into our process, and return the session type.
fn import_graphical_env() -> Option<SessionType> {
    let output = Command::new("loginctl")
        .args(["list-sessions", "--no-legend"])
        .output()
        .ok()?;

    let sessions_text = String::from_utf8_lossy(&output.stdout);

    for line in sessions_text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let sid = parts[0];

        // Get session type
        let type_output = Command::new("loginctl")
            .args(["show-session", sid, "-p", "Type", "--value"])
            .output()
            .ok()?;
        let session_type = String::from_utf8_lossy(&type_output.stdout)
            .trim()
            .to_string();

        if session_type != "wayland" && session_type != "x11" {
            continue;
        }

        // Get leader PID to read its environment
        let leader_output = Command::new("loginctl")
            .args(["show-session", sid, "-p", "Leader", "--value"])
            .output()
            .ok()?;
        let leader_pid = String::from_utf8_lossy(&leader_output.stdout)
            .trim()
            .to_string();

        if leader_pid.is_empty() || leader_pid == "0" {
            continue;
        }

        // Read environment from /proc/<pid>/environ
        let environ_path = format!("/proc/{}/environ", leader_pid);
        if let Ok(environ) = fs::read(&environ_path) {
            let environ_str = String::from_utf8_lossy(&environ);
            for var in environ_str.split('\0') {
                if let Some((key, value)) = var.split_once('=') {
                    match key {
                        "WAYLAND_DISPLAY" | "DISPLAY" | "XDG_RUNTIME_DIR" => {
                            debug!("Importing {}={} from session {}", key, value, sid);
                            // SAFETY: called during single-threaded init before watchers start
                            unsafe {
                                env::set_var(key, value);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        debug!(
            "Detected {} session via loginctl (session {})",
            session_type, sid
        );
        return match session_type.as_str() {
            "wayland" => Some(SessionType::Wayland),
            "x11" => Some(SessionType::X11),
            _ => None,
        };
    }

    None
}

fn detect_from_processes() -> Option<SessionType> {
    let wayland_compositors = [
        "Hyprland",
        "sway",
        "mutter",
        "kwin_wayland",
        "weston",
    ];
    let x11_wms = ["Xorg", "i3", "openbox", "xfwm4"];

    for name in wayland_compositors {
        if process_running(name) {
            debug!("Detected Wayland via running process: {}", name);
            return Some(SessionType::Wayland);
        }
    }

    for name in x11_wms {
        if process_running(name) {
            debug!("Detected X11 via running process: {}", name);
            return Some(SessionType::X11);
        }
    }

    warn!("Could not detect session type from running processes");
    None
}

fn process_running(name: &str) -> bool {
    Command::new("pgrep")
        .args(["-x", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
