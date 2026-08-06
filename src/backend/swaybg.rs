use std::{
    path::Path,
    process::{Child, Command},
};

use super::WallpaperBackend;

pub struct SwaybgBackend {
    process: Option<Child>,
}

impl SwaybgBackend {
    pub fn new() -> Self {
        Self {
            process: None,
        }
    }
}

impl WallpaperBackend for SwaybgBackend {

    fn set(&mut self, image: &Path, mode: &str) {
        self.stop();

        let child = Command::new("swaybg")
        .arg("-i")
        .arg(image)
        .arg("-m")
        .arg(mode)
        .spawn()
        .expect("Failed to start swaybg");

        self.process = Some(child);
    }

    fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
        }
    }
}
