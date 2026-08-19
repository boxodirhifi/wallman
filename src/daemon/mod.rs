use directories::ProjectDirs;
use std::collections::HashMap;
use std::thread;
use crate::ipc;

pub struct Daemon {
    config: crate::config::Config,
}

impl Daemon {
    pub fn new() -> Self {
        let config = crate::config::load();
        Self { config }
    }

    pub fn run(&mut self) {
        let listener = ipc::create_listener();

        let project_dirs = ProjectDirs::from("", "", "wallman")
        .expect("Failed to determine cache directory");

        let cache_dir = project_dirs.cache_dir();
        let cached_wallpaper = cache_dir.join("current.png");
        #[derive(serde::Deserialize)]
        struct Meta { mode: String, blur: u32 }

        let mut default_mode = self.config.mode.clone();
        let mut default_blur = self.config.blur;
        if let Ok(meta_str) = std::fs::read_to_string(cache_dir.join("current.toml")) {
            if let Ok(meta) = toml::from_str::<Meta>(&meta_str) {
                default_mode = meta.mode;
                default_blur = meta.blur;
            }
        }

        // Scan for per-monitor cached wallpapers
        let mut monitor_overrides: HashMap<String, (std::path::PathBuf, String)> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(cache_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.ends_with(".png") && file_name != "current.png" {
                    let monitor_name = file_name.trim_end_matches(".png").to_string();
                    let path = entry.path();

                    let mut monitor_mode = default_mode.clone();
                    let meta_path = cache_dir.join(format!("{}.toml", monitor_name));
                    if let Ok(meta_str) = std::fs::read_to_string(meta_path) {
                        if let Ok(meta) = toml::from_str::<Meta>(&meta_str) {
                            monitor_mode = meta.mode;
                        }
                    }

                    monitor_overrides.insert(monitor_name, (path, monitor_mode));
                }
            }
        }

        if cached_wallpaper.exists() {
            println!("Restoring cached wallpaper...");
        }
        if !monitor_overrides.is_empty() {
            println!("Restoring {} per-monitor wallpaper(s)...", monitor_overrides.len());
        }

        let (renderer_sender, renderer_receiver) = calloop::channel::channel();
        let renderer_image = cached_wallpaper.clone();
        let renderer_mode = default_mode.clone();

        thread::spawn(move || {
            crate::renderer::run(
                renderer_image,
                renderer_mode,
                default_blur,
                monitor_overrides,
                renderer_receiver,
            )
            .expect("Renderer crashed");
        });

        println!("wallman daemon started");

        // --- RESTORE WALLPAPER ON STARTUP ---
        let restore_sender = renderer_sender.clone();
        std::thread::spawn(move || {
            // Brief wait for renderer to create surfaces
            std::thread::sleep(std::time::Duration::from_millis(100));

            let saved = crate::state::load();

            if let Some(ref image) = saved.default_image {
                if image.exists() {
                    let mode = saved.default_mode.clone().unwrap_or_else(|| "fill".to_string());
                    let blur = saved.default_blur.unwrap_or(8);
                    restore_sender
                    .send(crate::renderer::RendererCommand::SetWallpaper {
                        image: image.clone(),
                          mode,
                          monitor: String::new(),
                          blur,
                    })
                    .ok();
                }
            }

            for o in &saved.monitor_overrides {
                if o.image.exists() {
                    restore_sender
                    .send(crate::renderer::RendererCommand::SetWallpaper {
                        image: o.image.clone(),
                          mode: o.mode.clone(),
                          monitor: o.monitor.clone(),
                          blur: o.blur,
                    })
                    .ok();
                }
            }
        });
        // --- END RESTORE ---

        ipc::serve(listener, move |command| {
            println!("Daemon received: {:?}", command);

            match command {
                ipc::Command::Set { image, mode, monitor, blur } => {
                    renderer_sender
                    .send(crate::renderer::RendererCommand::SetWallpaper {
                        image: std::path::PathBuf::from(image),
                          mode,
                          monitor,
                          blur,
                    })
                    .expect("Failed to send wallpaper update to renderer");
                }

                ipc::Command::Reload => {
                    renderer_sender
                    .send(crate::renderer::RendererCommand::Reload)
                    .expect("Failed to send reload command to renderer");
                }

                ipc::Command::Stop => {
                    println!("Received Stop command. Shutting down...");
                    std::process::exit(0);
                }
            }
        });

    }
}
