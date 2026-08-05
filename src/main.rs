mod cli;
mod commands;

use clap::Parser;
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Set { image } => {
            commands::set::run(image);
        }
    }
}
