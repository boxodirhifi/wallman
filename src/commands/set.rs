use directories::ProjectDirs;
use std::{fs, path::PathBuf};

pub fn run(
    image: PathBuf,
    mode: Option<String>,
) {
    let config = crate::config::load();

    let mode = mode.unwrap_or(config.mode);
    if !image.exists() {
        eprintln!("Error: '{}' does not exist.", image.display());
        std::process::exit(1);
    }

    let project_dirs =
    ProjectDirs::from("", "", "wallman").expect("Failed to determine cache directory");

    let cache_dir = project_dirs.cache_dir();

    fs::create_dir_all(cache_dir).expect("Failed to create cache directory");

    let cached_wallpaper = cache_dir.join("current.png");

    fs::copy(&image, &cached_wallpaper)
    .expect("Failed to copy wallpaper into cache");

    let command = format!(
        "SET|{}|{}",
        cached_wallpaper.display(),
                          mode,
    );

    crate::ipc::send_command(&command);
}
