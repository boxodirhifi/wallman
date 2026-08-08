pub fn show() {
    let config = crate::config::load();

    println!("mode = \"{}\"", config.mode);
}

pub fn set_mode(mode: String) {
    let mut config = crate::config::load();
    config.mode = mode;

    crate::config::save(&config);

    println!("Default mode set to '{}'.", config.mode);
}
