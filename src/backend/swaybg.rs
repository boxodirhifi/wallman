use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

pub fn stop_existing() {
    let _ = Command::new("pkill")
    .arg("swaybg")
    .status();
}

pub fn start_wallpaper(
    image: &Path,
    mode: &str,
) -> Child {
    Command::new("swaybg")
    .arg("-i")
    .arg(image)
    .arg("-m")
    .arg(mode)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("Failed to start swaybg")
}
