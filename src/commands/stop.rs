use std::process::Command;

pub fn run() {
    let status = Command::new("systemctl")
    .args(["--user", "stop", "wallman.service"])
    .status()
    .expect("Failed to execute systemctl");

    if !status.success() {
        eprintln!("Failed to stop wallman.service");
    }
}
