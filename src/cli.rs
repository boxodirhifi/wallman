use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "wallman",
    about = "A lightweight Wayland wallpaper manager"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Set {
        image: std::path::PathBuf,
    },
}
