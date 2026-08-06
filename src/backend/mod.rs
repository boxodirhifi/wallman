pub mod controller;
pub mod swaybg;
pub mod factory;

use std::path::Path;

pub trait WallpaperBackend: Send {
    fn set(&mut self, image: &Path, mode: &str);
    fn stop(&mut self);
}
