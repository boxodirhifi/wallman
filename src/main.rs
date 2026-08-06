mod cli;
mod commands;
mod backend;
mod daemon;
mod ipc;
mod config;

use clap::Parser;
use cli::{Cli, Commands, ConfigCommands};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Set { image, mode } => {
            commands::set::run(image, mode);
        }

        Commands::Daemon => {
            let mut daemon = daemon::Daemon::new();
            daemon.run();
        }

        Commands::Status => {
            commands::status::run();
        }

        Commands::Reload => {
            commands::reload::run();
        }

        Commands::Stop => {
            commands::stop::run();
        }

        Commands::Config { command } => {
            match command {
                ConfigCommands::Show => {
                    commands::config::show();
                }

                ConfigCommands::SetMode { mode } => {
                    commands::config::set_mode(mode);
                }

                ConfigCommands::SetBackend { backend } => {
                    commands::config::set_backend(backend);
                }
            }
        }

    }

}
