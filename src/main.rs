mod cli;
mod commands;
mod backend;
mod daemon;
mod ipc;

use clap::Parser;
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Set { image } => {
            commands::set::run(image);
        }

        Commands::Daemon => {
            let mut daemon = daemon::Daemon::new();
            daemon.run();
        }
    }
}
