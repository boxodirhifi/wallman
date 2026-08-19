use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct WallmanState {
    pub default_image: Option<PathBuf>,
    pub default_mode: Option<String>,
    pub default_blur: Option<u32>,
    pub monitor_overrides: Vec<MonitorOverride>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MonitorOverride {
    pub monitor: String,
    pub image: PathBuf,
    pub mode: String,
    pub blur: u32,
}

pub fn state_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("", "", "wallman")
    .expect("Failed to determine state directory");
    dirs.cache_dir().join("state.json")
}

pub fn save(state: &WallmanState) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let content = serde_json::to_string_pretty(state).ok();
    if let Some(content) = content {
        fs::write(&path, content).ok();
    }
}

pub fn load() -> WallmanState {
    let path = state_path();
    if !path.exists() {
        return WallmanState::default();
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return WallmanState::default(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}
