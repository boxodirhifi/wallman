use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_mode")]
    pub mode: String,

    #[serde(default = "default_blur")]
    pub blur: u32,
}

fn default_mode() -> String {
    "fill".to_string()
}

fn default_blur() -> u32 {
    8
}

pub fn config_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("", "", "wallman")
    .expect("Failed to determine config directory");
    dirs.config_dir().join("config.toml")
}

pub fn load() -> Config {
    let path = config_path();
    if !path.exists() {
        return Config {
            mode: default_mode(),
            blur: default_blur(),
        };
    }
    let content = fs::read_to_string(&path).expect("Failed to read config");
    toml::from_str(&content).expect("Failed to parse config")
}

pub fn save(config: &Config) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("Failed to create config directory");
    }
    let content = toml::to_string_pretty(config).expect("Failed to serialize config");
    fs::write(&path, content).expect("Failed to write config");
}
