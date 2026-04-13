mod app;
mod css;
pub(crate) mod paste;

use crate::config::Config;

pub fn run(config: &Config) {
    // Detect the previously focused window BEFORE the picker steals focus.
    // This is the only reliable way to know if a terminal was focused.
    let target_is_terminal = match config.paste_mode.as_str() {
        "terminal" => true,
        "normal" => false,
        _ => paste::get_focused_window_class()
            .as_deref()
            .map(paste::is_terminal_class_pub)
            .unwrap_or(false),
    };
    app::run(config, target_is_terminal);
}
