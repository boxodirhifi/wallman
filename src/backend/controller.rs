use std::path::Path;

use super::WallpaperBackend;

pub struct WallpaperController {
    backend: Box<dyn WallpaperBackend>,
}

impl WallpaperController {

    pub fn new(backend: Box<dyn WallpaperBackend>) -> Self {
        Self {
            backend,
        }
    }

    pub fn set_wallpaper(
        &mut self,
        image: &Path,
        mode: &str,
    ) {
        self.backend.set(image, mode);
    }
}
