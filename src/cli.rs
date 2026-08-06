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
    #[command(about = "Set a wallpaper")]
    Set {
        image: std::path::PathBuf,

        #[arg(
        long,
        value_parser = clap::builder::PossibleValuesParser::new([
            "fill",
            "fit",
            "stretch",
            "center",
            "tile",
        ])
        )]
        mode: Option<String>,
    },

    #[command(about = "Run the wallpaper daemon")]
    Daemon,

    #[command(about = "Show daemon status")]
    Status,

    #[command(about = "Reload current wallpaper")]
    Reload,

    #[command(about = "Stop daemon")]
    Stop,
}
