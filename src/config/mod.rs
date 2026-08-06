use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Deserialize, Serialize)]
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

pub fn save(config: &Config) {
    let project_dirs =
    ProjectDirs::from("", "", "wallman")
    .expect("Failed to determine config directory");

    let config_dir = project_dirs.config_dir();

    fs::create_dir_all(config_dir)
    .expect("Failed to create config directory");

    let config_path = config_dir.join("config.toml");

    let contents =
    toml::to_string_pretty(config)
    .expect("Failed to serialize config");

    fs::write(config_path, contents)
    .expect("Failed to write config");
}
