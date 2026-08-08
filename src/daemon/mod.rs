use directories::ProjectDirs;
use std::sync::{Arc, Mutex};
use std::thread;
use crate::{
    backend::controller::WallpaperController,
    ipc,
};

pub struct Daemon {
    controller: Arc<Mutex<WallpaperController>>,
    config: crate::config::Config,
}

impl Daemon {
    pub fn new() -> Self {
        let config = crate::config::load();

        let backend =
        crate::backend::factory::create_backend(&config.backend);

        Self {
            controller: Arc::new(
                Mutex::new(
                    WallpaperController::new(backend),
                ),
            ),
            config,
        }
    }

    pub fn run(&mut self) {
        let listener = ipc::create_listener();

        let project_dirs =
        ProjectDirs::from("", "", "wallman")
        .expect("Failed to determine cache directory");

        let cached_wallpaper =
        project_dirs.cache_dir().join("current.png");

        let controller = Arc::clone(&self.controller);

        // Clone values needed by the IPC closure.
        let default_mode = self.config.mode.clone();

        if cached_wallpaper.exists() {
            println!("Restoring cached wallpaper...");

            let mut controller = controller
            .lock()
            .expect("Failed to lock controller");

            controller.set_wallpaper(
                &cached_wallpaper,
                &default_mode,
            );
        }

        let renderer_image = cached_wallpaper.clone();

        thread::spawn(move || {
            crate::renderer::run(renderer_image)
            .expect("Renderer crashed");
        });

        println!("wallman daemon started");

        ipc::serve(listener, move |command| {
            println!("Daemon received: {}", command);

            let mut parts = command.split('|');
            let action = parts.next();

            match action {
                Some("SET") => {
                    let image = parts.next().unwrap();
                    let mode = parts.next().unwrap_or(&default_mode);

                    let mut controller = controller
                    .lock()
                    .expect("Failed to lock controller");

                    controller.set_wallpaper(
                        std::path::Path::new(image),
                                             mode,
                    );
                }

                Some("RELOAD") => {
                    if cached_wallpaper.exists() {
                        let mut controller = controller
                        .lock()
                        .expect("Failed to lock controller");

                        controller.set_wallpaper(
                            &cached_wallpaper,
                            &default_mode,
                        );
                    }
                }

                _ => {
                    eprintln!("Unknown command");
                }
            }
        });
    }
}
