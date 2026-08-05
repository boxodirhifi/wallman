use std::{
    path::Path,
    process::Command,
};

pub fn set_wallpaper(image: &Path) {
    // Kill existing swaybg instances.
    let _ = Command::new("pkill")
    .arg("swaybg")
    .status();

    // Start a new swaybg.
    Command::new("swaybg")
    .args(["-i", image.to_str().unwrap(), "-m", "fill"])
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .spawn()
    .expect("Failed to start swaybg");
}
