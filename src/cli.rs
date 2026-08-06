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

    #[command(
    about = "Manage configuration",
    visible_alias = "cfg"
    )]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    #[command(about = "Show current configuration")]
    Show,

    #[command(about = "Modify configuration")]
    Set {
        #[command(subcommand)]
        setting: ConfigSetCommands,
    },
}

#[derive(Subcommand)]
pub enum ConfigSetCommands {
    #[command(about = "Set default wallpaper mode")]
    Mode {
        #[arg(value_parser = clap::builder::PossibleValuesParser::new([
        "fill",
        "fit",
        "stretch",
        "center",
        "tile",
        ]))]
        mode: String,
    },

    #[command(about = "Set wallpaper backend")]
    Backend {
        backend: String,
    },
}
