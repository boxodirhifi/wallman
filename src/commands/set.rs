use directories::ProjectDirs;
use std::{fs, path::PathBuf};

pub fn run(
    image: PathBuf,
    mode: Option<String>,
    monitor: Option<String>,
    blur: Option<u32>,
) {
    let config = crate::config::load();

    let mode = mode.unwrap_or(config.mode);

    let blur = blur.unwrap_or(config.blur);


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

    let cache_filename = if let Some(ref m) = monitor {
        format!("{}.png", m)
    } else {
        "current.png".to_string()
    };

    let cached_wallpaper = cache_dir.join(&cache_filename);
    let temporary_wallpaper = cache_dir.join(format!("{}.tmp", cache_filename));

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

    let display_monitor = monitor.as_deref().unwrap_or("all monitors");
    println!(
        "Cached wallpaper: {} → {} (Target: {})",
             image.display(),
             cached_wallpaper.display(),
             display_monitor
    );

    let ipc_monitor = monitor.unwrap_or_default();
    let command = format!(
        "SET|{}|{}|{}|{}",
        cached_wallpaper.display(),
                          mode,
                          ipc_monitor,
                          blur
    );

    crate::ipc::send_command(&command);
}
