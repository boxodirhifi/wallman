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
    ProjectDirs::from("", "", "wallman")
    .expect("Failed to determine cache directory");

    let cache_dir = project_dirs.cache_dir();

    fs::create_dir_all(cache_dir)
    .expect("Failed to create cache directory");

    let cached_wallpaper = cache_dir.join("current.png");

    // Decode the source image and write a real PNG to the cache.
    let decoded = image::open(&image)
    .unwrap_or_else(|e| {
        panic!(
            "Failed to decode wallpaper '{}': {e}",
            image.display()
        )
    });

    decoded
    .save_with_format(
        &cached_wallpaper,
        image::ImageFormat::Png,
    )
    .unwrap_or_else(|e| {
        panic!(
            "Failed to encode cached wallpaper '{}': {e}",
            cached_wallpaper.display()
        )
    });

    println!(
        "Cached wallpaper: {} → {}",
        image.display(),
             cached_wallpaper.display()
    );

    let command = format!(
        "SET|{}|{}",
        cached_wallpaper.display(),
                          mode,
    );

    crate::ipc::send_command(&command);
}
