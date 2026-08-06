use super::{
    swaybg::SwaybgBackend,
    WallpaperBackend,
};

pub fn create_backend(name: &str) -> Box<dyn WallpaperBackend> {
    match name {
        "swaybg" => Box::new(SwaybgBackend::new()),

        other => {
            eprintln!(
                "Unknown backend '{}', falling back to swaybg",
                other
            );

            Box::new(SwaybgBackend::new())
        }
    }
}

