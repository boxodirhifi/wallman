mod cli;

use clap::Parser;
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Set { image } => {
            if !image.exists() {
                eprintln!("Error: '{}' does not exist.", image.display());
                std::process::exit(1);
            }

            println!("Setting wallpaper: {}", image.display());
        }
    }
}
