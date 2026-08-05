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
        println!("wallman daemon started");

        let controller = Arc::clone(&self.controller);

        ipc::start_server(move |command| {
            println!("Daemon received: {}", command);

            let mut controller = controller
            .lock()
            .expect("Failed to lock controller");

            controller.set_wallpaper(
                std::path::Path::new(&command)
            );
        });
    }
}
