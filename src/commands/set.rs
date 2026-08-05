use std::path::PathBuf;

pub fn run(image: PathBuf) {
    if !image.exists() {
        eprintln!("Error: '{}' does not exist.", image.display());
        std::process::exit(1);
    }

    println!("Setting wallpaper: {}", image.display());
}
