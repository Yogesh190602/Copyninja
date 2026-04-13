# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CopyNinja is a clipboard history manager for Linux (Wayland & X11) written in Rust. It has two modes: a background **daemon** (`copyninja daemon`) that monitors the clipboard, and a **picker** (`copyninja pick`) that shows a GTK4 UI for selecting past entries. It supports text and images, cross-device sync, and auto-paste.

## Build & Development Commands

```bash
cargo build --release     # Build release binary
cargo test                # Run all unit tests
cargo clippy              # Lint
cargo fmt                 # Format
cargo test <test_name>    # Run a single test by name
./install.sh              # Build + install binary + systemd service + keybinding
./uninstall.sh            # Remove everything
```

Runtime dependencies: `gtk4`, `wl-clipboard`, `wtype`, `xclip`, `xdotool`, `ydotool`, `libnotify`.

## Architecture

Two-process model invoked via clap subcommands in `main.rs`:

- **Daemon** (`src/daemon/`): Runs a tokio async runtime. Detects session type (Wayland/X11) in `session.rs`, then starts the appropriate clipboard watcher — `wayland.rs` uses `wl-paste --watch` (event-driven), `x11.rs` uses `xclip` (polling). Exposes a D-Bus interface (`dbus.rs`) for programmatic entry addition. Has a retry loop (up to 5 min) for session availability at boot. If native Wayland fails, falls back to X11/XWayland.

- **Picker** (`src/picker/`): GTK4 application. `app.rs` builds the UI (dark Catppuccin Mocha theme from `css.rs`), handles search/keybindings/image thumbnails. `paste.rs` implements the auto-paste fallback chain: wtype → xdotool → ydotool key → ydotool type → copy-only.

- **Storage** (`src/storage.rs`): Global singleton via `OnceLock<Storage>`. Manages JSON history file with backup rotation, MD5-based deduplication, and entry pruning. Handles backward compatibility with legacy text-only entries via `ClipEntry::resolved_content()`.

- **Content** (`src/content.rs`): `ClipContent` enum — `Text { text, preview }` or `Image { path, mime }`. Uses serde tagged enum (`#[serde(tag = "type")]`).

- **Config** (`src/config.rs`): TOML config from `~/.config/copyninja/config.toml`. All fields optional with defaults via `#[serde(default)]`. Includes `SyncConfig` sub-table.

- **Sync** (`src/sync.rs`): File-based cross-device sync. Exports entries as individual JSON files, uses tombstones for deletions, watches sync directory for changes with the `notify` crate.

## Key Design Decisions

- Storage is a global singleton (`OnceLock`) initialized in `main.rs` before CLI parsing
- Legacy JSON format support: `ClipEntry` has both `content: Option<ClipContent>` and old `text`/`preview` fields
- Config file is hot-reloadable at runtime (no rebuild needed)
- RUST_LOG env var overrides the config `log_level`
