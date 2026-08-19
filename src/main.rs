mod cli;
mod commands;
mod daemon;
mod ipc;
mod config;
mod renderer;
mod state;

use clap::Parser;
use cli::{Cli, Commands, ConfigCommands, ConfigSetCommands};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Set { image, mode, monitor, blur } => {
            commands::set::run(image, mode, monitor, blur);
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

                ConfigCommands::Set { setting } => {
                    match setting {
                        ConfigSetCommands::Mode { mode } => {
                            commands::config::set_mode(mode);
                        }
                    }
                }
            }
        }

    }

}
