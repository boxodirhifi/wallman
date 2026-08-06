use directories::ProjectDirs;
use std::sync::{Arc, Mutex};

use crate::{
    backend::controller::WallpaperController,
    ipc,
};

pub struct Daemon {
    controller: Arc<Mutex<WallpaperController>>,
}

impl Daemon {
    pub fn new() -> Self {
        Self {
            controller: Arc::new(Mutex::new(WallpaperController::new())),
        }
    }
    pub fn run(&mut self) {
        let listener = ipc::create_listener();
        let config = crate::config::load();

        crate::backend::swaybg::stop_existing();

        let project_dirs =
        ProjectDirs::from("", "", "wallman")
        .expect("Failed to determine cache directory");

        let cached_wallpaper =
        project_dirs.cache_dir().join("current.png");

        let controller = Arc::clone(&self.controller);

        if cached_wallpaper.exists() {
            println!("Restoring cached wallpaper...");

            let mut controller = controller
            .lock()
            .expect("Failed to lock controller");

            controller.set_wallpaper(
                &cached_wallpaper,
                &config.mode,
            );
        }

        println!("wallman daemon started");



        ipc::serve(listener, move |command| {
            println!("Daemon received: {}", command);

            let mut parts = command.split('|');

            let action = parts.next();

            match action {
                Some("SET") => {
                    let image = parts.next().unwrap();
                    let mode = parts.next().unwrap_or(&config.mode);

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
                            &config.mode,
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
