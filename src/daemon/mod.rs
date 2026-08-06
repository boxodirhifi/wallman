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
        use directories::ProjectDirs;
        let config = crate::config::load();
        println!("wallman daemon started");

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

        ipc::start_server(move |command| {
            println!("Daemon received: {}", command);

            let mut controller = controller
            .lock()
            .expect("Failed to lock controller");

            controller.set_wallpaper(
                std::path::Path::new(&command),
                &config.mode,
            );
        });
    }
}
