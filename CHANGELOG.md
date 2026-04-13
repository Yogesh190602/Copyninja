# Changelog

## [1.1.0] - 2026-03-23

### Added
- Runtime TOML config file (`~/.config/copyninja/config.toml`)
  - `max_entries` — max clipboard history entries (default: 50)
  - `max_backups` — backup file count for recovery (default: 3)
  - `history_file` — custom history file path
  - `log_level` — logging verbosity (default: info)
  - `auto_paste` — enable/disable auto-paste after selection (default: true)
- `copyninja --version` flag
- Corrupt JSON recovery with automatic backup rotation
- Pinned entries now survive pruning (bug fix)

### Fixed
- Pinned entries could be evicted when history reached max_entries

## [1.0.0] - 2026-03-22

### Added
- Initial Rust rewrite from Python
- Clipboard monitoring daemon (Wayland + X11)
- GTK4 picker UI with search, pin, delete, clear-all
- Auto-paste via wtype/xdotool/ydotool fallback chain
- Systemd user service integration
- D-Bus interface for external clipboard entry submission
- Catppuccin Mocha dark theme
