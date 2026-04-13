pub const THEME: &str = r#"
/* Catppuccin Mocha theme */

window {
    background-color: #1e1e2e;
    color: #cdd6f4;
}

headerbar {
    background-color: #1e1e2e;
    border-bottom: 1px solid #313244;
    min-height: 40px;
    padding: 0 8px;
}

headerbar .title-label {
    font-size: 14px;
    font-weight: 700;
    color: #cdd6f4;
}

headerbar .count-badge {
    background-color: #313244;
    color: #a6adc8;
    border-radius: 10px;
    padding: 2px 8px;
    font-size: 11px;
    min-height: 18px;
}

.search-entry {
    background-color: #313244;
    color: #cdd6f4;
    border: 1px solid #45475a;
    border-radius: 8px;
    margin: 8px 10px 4px 10px;
    padding: 6px 10px;
    font-size: 13px;
    min-height: 28px;
}

.search-entry:focus-within {
    border-color: #89b4fa;
    box-shadow: 0 0 0 1px rgba(137,180,250,0.3);
}

.search-entry image {
    color: #6c7086;
}

list {
    background-color: transparent;
}

list row {
    background-color: transparent;
    padding: 0;
    margin: 0;
    border-radius: 0;
    outline: none;
}

list row:selected {
    background-color: transparent;
}

.clip-card {
    background-color: #1e1e2e;
    border-bottom: 1px solid #252535;
    padding: 8px 12px;
    margin: 0;
    transition: background-color 150ms ease;
}

.clip-card:hover {
    background-color: #262637;
}

list row:selected .clip-card {
    background-color: #2a2a3d;
    border-left: 3px solid #89b4fa;
    padding-left: 9px;
}

.clip-preview {
    color: #cdd6f4;
    font-size: 12px;
}

.clip-time {
    color: #6c7086;
    font-size: 10px;
    margin-top: 2px;
}

.pin-icon {
    color: #f9e2af;
    font-size: 10px;
    margin-right: 6px;
}

.section-header {
    color: #6c7086;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 1px;
    padding: 10px 14px 4px 14px;
}

.menu-btn {
    background: transparent;
    border: none;
    color: #585b70;
    min-width: 24px;
    min-height: 24px;
    padding: 2px;
    border-radius: 6px;
}

.menu-btn:hover {
    background-color: #313244;
    color: #cdd6f4;
}

popover {
    background-color: #313244;
    border: 1px solid #45475a;
    border-radius: 8px;
}

popover modelbutton {
    padding: 6px 12px;
    color: #cdd6f4;
    font-size: 12px;
}

popover modelbutton:hover {
    background-color: #45475a;
}

.footer-bar {
    background-color: #1e1e2e;
    border-top: 1px solid #313244;
    padding: 6px 10px;
}

.clear-btn {
    background-color: transparent;
    color: #a6adc8;
    border: 1px solid #45475a;
    border-radius: 8px;
    padding: 4px 16px;
    font-size: 12px;
    min-height: 28px;
}

.clear-btn:hover {
    background-color: #45475a;
    color: #cdd6f4;
}

.clear-btn.confirm {
    background-color: #f38ba8;
    color: #1e1e2e;
    border-color: #f38ba8;
    font-weight: 700;
}

.empty-icon {
    color: #45475a;
    font-size: 48px;
}

.empty-label {
    color: #585b70;
    font-size: 14px;
    margin-top: 8px;
}

.empty-sublabel {
    color: #45475a;
    font-size: 11px;
    margin-top: 4px;
}

scrolledwindow undershoot.top,
scrolledwindow undershoot.bottom {
    background: none;
}

scrollbar slider {
    background-color: #45475a;
    border-radius: 99px;
    min-width: 4px;
}

scrollbar slider:hover {
    background-color: #585b70;
}

.clip-image {
    border-radius: 6px;
    min-width: 48px;
    min-height: 48px;
}
"#;
