use std::path::Path;

use super::{
    process::ProcessManager,
    swaybg,
};

pub struct WallpaperController {
    manager: ProcessManager,
}

impl WallpaperController {
    pub fn new() -> Self {
        Self {
            manager: ProcessManager::new(),
        }
    }

    pub fn set_wallpaper(
        &mut self,
        image: &Path,
        mode: &str,
    ) {
        let child = swaybg::start_wallpaper(image, mode);
        self.manager.replace(child);
    }
}
