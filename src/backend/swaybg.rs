use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

pub fn start_wallpaper(image: &Path) -> Child {
    Command::new("swaybg")
    .args(["-i", image.to_str().unwrap(), "-m", "fill"])
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("Failed to start swaybg")
}
