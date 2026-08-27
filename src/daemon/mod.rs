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

        // Load persistent state first
        let saved_state = crate::state::load();

        let mut default_mode = saved_state.default_mode.clone().unwrap_or_else(|| self.config.mode.clone());
        let mut default_blur = saved_state.default_blur.unwrap_or(self.config.blur);

        // Fallback to current.toml if state.json didn't have it (legacy support)
        if saved_state.default_mode.is_none() {
            if let Ok(meta_str) = std::fs::read_to_string(cache_dir.join("current.toml")) {
                if let Ok(meta) = toml::from_str::<Meta>(&meta_str) {
                    default_mode = meta.mode;
                    default_blur = meta.blur;
                }
            }
        }

        let mut monitor_overrides: HashMap<String, (std::path::PathBuf, String)> = HashMap::new();
        let mut initial_per_monitor_blur: HashMap<String, u32> = HashMap::new();

        // Populate overrides and blurs directly from state.json
        for o in &saved_state.monitor_overrides {
            if o.image.exists() {
                monitor_overrides.insert(o.monitor.clone(), (o.image.clone(), o.mode.clone()));
                initial_per_monitor_blur.insert(o.monitor.clone(), o.blur);
            }
        }

        // Legacy scan for any missed per-monitor PNGs
        if let Ok(entries) = std::fs::read_dir(cache_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.ends_with(".png") && file_name != "current.png" {
                    let monitor_name = file_name.trim_end_matches(".png").to_string();
                    if !monitor_overrides.contains_key(&monitor_name) {
                        let path = entry.path();
                        let mut monitor_mode = default_mode.clone();
                        let mut monitor_blur = default_blur;
                        let meta_path = cache_dir.join(format!("{}.toml", monitor_name));
                        if let Ok(meta_str) = std::fs::read_to_string(meta_path) {
                            if let Ok(meta) = toml::from_str::<Meta>(&meta_str) {
                                monitor_mode = meta.mode;
                                monitor_blur = meta.blur;
                            }
                        }
                        monitor_overrides.insert(monitor_name.clone(), (path, monitor_mode));
                        initial_per_monitor_blur.insert(monitor_name, monitor_blur);
                    }
                }
            }
        }

        let renderer_image = saved_state.default_image.clone().unwrap_or_else(|| cached_wallpaper.clone());

        if renderer_image.exists() {
            println!("Restoring cached wallpaper...");
        }
        if !monitor_overrides.is_empty() {
            println!("Restoring {} per-monitor wallpaper(s)...", monitor_overrides.len());
        }

        let (renderer_sender, renderer_receiver) = calloop::channel::channel();
        let renderer_mode = default_mode.clone();

        thread::spawn(move || {
            crate::renderer::run(
                renderer_image,
                renderer_mode,
                default_blur,
                initial_per_monitor_blur, // <-- Passed correctly now!
                monitor_overrides,
                renderer_receiver,
            )
            .expect("Renderer crashed");
        });

        println!("wallman daemon started");

        // --- 100ms RESTORE THREAD DELETED ---

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
