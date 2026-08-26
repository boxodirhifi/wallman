use directories::ProjectDirs;
use std::{fs, path::PathBuf};
use image::GenericImageView;

pub fn run(
    image: PathBuf,
    mode: Option<String>,
    monitor: Option<String>,
    blur: Option<u32>,
) {
    let config = crate::config::load();

    let mode = mode.unwrap_or(config.mode);

    let blur = blur.unwrap_or(config.blur);

    if let Some(ref m) = monitor {
            let is_valid = m.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !is_valid {
                eprintln!("Error: invalid monitor name '{}'. Only letters, numbers, hyphens, and underscores are allowed.", m);
                std::process::exit(1);
            }
        }

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

    // Resize if exceeds max dimension before caching
    let max_dim = 3840u32;
    let (w, h) = decoded.dimensions();
    let decoded = if w > max_dim || h > max_dim {
        println!("Resizing source image to fit within {}x{} max dimension", max_dim, max_dim);
        decoded.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3)
    } else {
        decoded
    };

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

    // If setting a global wallpaper, clean up stale per-monitor caches
    if monitor.is_none() {
        if let Ok(entries) = fs::read_dir(cache_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();

                // Skip global files
                if file_name_str.starts_with("current.") || file_name_str == "colors.toml" || file_name_str == "state.json" {
                    continue;
                }

                // Delete per-monitor cache files
                if file_name_str.ends_with(".png") || file_name_str.ends_with(".toml") || file_name_str.ends_with(".raw") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    let display_monitor = monitor.as_deref().unwrap_or("all monitors");
    println!(
        "Cached wallpaper: {} → {} (Target: {})",
             image.display(),
             cached_wallpaper.display(),
             display_monitor
    );

    let ipc_monitor = monitor.as_deref().unwrap_or_default().to_string();

    // Create our strongly-typed JSON command instead of a formatted string
    let command = crate::ipc::Command::Set {
        image: cached_wallpaper.display().to_string(),
        mode: mode.clone(),
        monitor: ipc_monitor.clone(),
        blur,
    };

    crate::ipc::send_command(&command);
    #[derive(serde::Serialize)]
    struct Meta { mode: String, blur: u32 }

    let metadata = Meta { mode: mode.clone(), blur };
    let meta_filename = if let Some(ref m) = monitor {
        format!("{}.toml", m)
    } else {
        "current.toml".to_string()
    };

    let meta_path = cache_dir.join(&meta_filename);
    if let Err(e) = std::fs::write(&meta_path, toml::to_string(&metadata).unwrap_or_default()) {
        eprintln!("Failed to write metadata: {e}");
    }
    // Persist state for next login
    let mut state = crate::state::load();
    if ipc_monitor.is_empty() {
        state.default_image = Some(cached_wallpaper.clone());
        state.default_mode = Some(mode.clone());
        state.default_blur = Some(blur);
    } else {
        // Remove existing override for this monitor if any
        state.monitor_overrides.retain(|o| o.monitor != ipc_monitor);
        state.monitor_overrides.push(crate::state::MonitorOverride {
            monitor: ipc_monitor.clone(),
                                     image: cached_wallpaper.clone(),
                                     mode: mode.clone(),
                                     blur,
        });
    }
    crate::state::save(&state);
}
