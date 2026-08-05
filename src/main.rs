mod cli;

use clap::Parser;
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Set { image } => {
            println!("Setting wallpaper: {}", image.display());
        }
    }
}
