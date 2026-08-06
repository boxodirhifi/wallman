use directories::ProjectDirs;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub backend: String,
    pub mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: "swaybg".into(),
            mode: "fill".into(),
        }
    }
}

pub fn load() -> Config {
    let project_dirs =
        ProjectDirs::from("", "", "wallman")
            .expect("Failed to determine config directory");

    let config_path = project_dirs
        .config_dir()
        .join("config.toml");

    match fs::read_to_string(&config_path) {
        Ok(contents) => {
            toml::from_str(&contents)
                .unwrap_or_else(|_| Config::default())
        }

        Err(_) => Config::default(),
    }
}
