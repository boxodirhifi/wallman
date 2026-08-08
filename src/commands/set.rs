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
    let temporary_wallpaper = cache_dir.join("current.png.tmp");

    // Decode the source image and save it as PNG to a temporary file.
    let decoded = image::open(&image)
    .unwrap_or_else(|e| {
        eprintln!(
            "Error: failed to open '{}': {e}",
            image.display()
        );
        std::process::exit(1);
    });

    decoded.save_with_format(
        &temporary_wallpaper,
        image::ImageFormat::Png,
    )
    .unwrap_or_else(|e| {
        eprintln!(
            "Error: failed to write cached wallpaper: {e}"
        );
        std::process::exit(1);
    });

    // Atomically replace the previous cache file.
    fs::rename(&temporary_wallpaper, &cached_wallpaper)
    .unwrap_or_else(|e| {
        eprintln!(
            "Error: failed to replace cached wallpaper: {e}"
        );
        std::process::exit(1);
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
