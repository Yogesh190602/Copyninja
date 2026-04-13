use log::warn;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub max_entries: usize,
    pub max_backups: usize,
    pub history_file: PathBuf,
    pub log_level: String,
    pub auto_paste: bool,
    /// Paste shortcut mode: "auto" (detect), "terminal" (Ctrl+Shift+V), "normal" (Ctrl+V)
    #[serde(default = "default_paste_mode")]
    pub paste_mode: String,
    pub image_dir: PathBuf,
    pub max_image_size_mb: u32,
    #[serde(default)]
    pub sync: SyncConfig,
}

fn default_paste_mode() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    pub enabled: bool,
    pub sync_dir: PathBuf,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sync_dir: PathBuf::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_entries: 50,
            max_backups: 3,
            history_file: default_history_file(),
            log_level: "info".to_string(),
            auto_paste: true,
            paste_mode: default_paste_mode(),
            image_dir: default_image_dir(),
            max_image_size_mb: 10,
            sync: SyncConfig::default(),
        }
    }
}

fn default_image_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().expect("cannot determine home directory").join(".local/share"))
        .join("copyninja")
        .join("images")
}

fn default_history_file() -> PathBuf {
    dirs::home_dir()
        .expect("cannot determine home directory")
        .join(".clipboard_history.json")
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("copyninja").join("config.toml"))
}

/// Parse config from a TOML string. Returns defaults on error.
pub fn parse(content: &str) -> Config {
    match toml::from_str(content) {
        Ok(config) => config,
        Err(e) => {
            warn!("Malformed config: {}. Using defaults.", e);
            Config::default()
        }
    }
}

pub fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => parse(&content),
        Err(_) => Config::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.max_entries, 50);
        assert_eq!(config.max_backups, 3);
        assert_eq!(config.log_level, "info");
        assert!(config.auto_paste);
    }

    #[test]
    fn test_partial_config() {
        let config: Config = toml::from_str("max_entries = 100").unwrap();
        assert_eq!(config.max_entries, 100);
        // Other fields should have defaults
        assert_eq!(config.max_backups, 3);
        assert!(config.auto_paste);
    }

    #[test]
    fn test_full_config() {
        let toml = r#"
            max_entries = 200
            max_backups = 5
            log_level = "debug"
            auto_paste = false
            history_file = "/tmp/test.json"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.max_entries, 200);
        assert_eq!(config.max_backups, 5);
        assert_eq!(config.log_level, "debug");
        assert!(!config.auto_paste);
        assert_eq!(config.history_file, PathBuf::from("/tmp/test.json"));
    }

    #[test]
    fn test_invalid_toml_returns_defaults() {
        let config = parse("NOT VALID TOML {{{{");
        assert_eq!(config.max_entries, 50);
    }

    #[test]
    fn test_empty_string_returns_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.max_entries, 50);
        assert!(config.auto_paste);
    }
}
