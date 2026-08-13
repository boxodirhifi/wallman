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
        let default_mode = self.config.mode.clone();
        let default_blur = self.config.blur;

        // Scan for per-monitor cached wallpapers
        let mut monitor_overrides: HashMap<String, (std::path::PathBuf, String)> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(cache_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.ends_with(".png") && file_name != "current.png" {
                    let monitor_name = file_name.trim_end_matches(".png").to_string();
                    let path = entry.path();
                    monitor_overrides.insert(monitor_name, (path, default_mode.clone()));
                }
            }
        }

        if cached_wallpaper.exists() {
            println!("Restoring cached wallpaper...");
        }
        if !monitor_overrides.is_empty() {
            println!("Restoring {} per-monitor wallpaper(s)...", monitor_overrides.len());
        }

        let (renderer_sender, renderer_receiver) = std::sync::mpsc::channel();
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
