use directories::ProjectDirs;
use std::thread;
use crate::{
    ipc,
};

pub struct Daemon {
    config: crate::config::Config,
}

impl Daemon {
    pub fn new() -> Self {
        let config = crate::config::load();

        Self { config }
    }

    pub fn run(&mut self) {
        let listener = ipc::create_listener();

        let project_dirs =
        ProjectDirs::from("", "", "wallman")
        .expect("Failed to determine cache directory");

        let cached_wallpaper =
        project_dirs.cache_dir().join("current.png");

        let default_mode = self.config.mode.clone();

        if cached_wallpaper.exists() {
            println!("Restoring cached wallpaper...");
        }

        let (renderer_sender, renderer_receiver) =
        std::sync::mpsc::channel();

        let renderer_image = cached_wallpaper.clone();

        thread::spawn(move || {
            crate::renderer::run(renderer_image, renderer_receiver)
            .expect("Renderer crashed");
        });

        println!("wallman daemon started");

        ipc::serve(listener, move |command| {
            println!("Daemon received: {}", command);

            let mut parts = command.split('|');
            let action = parts.next();

            match action {
                Some("SET") => {
                    let image = parts.next().unwrap();
                    let _mode = parts.next().unwrap_or(&default_mode);


                    renderer_sender
                    .send(crate::renderer::RendererCommand::SetWallpaper {
                        image: std::path::PathBuf::from(image),
                    })
                    .expect("Failed to send wallpaper update to renderer");
                }

                Some("RELOAD") => {
                    renderer_sender
                    .send(crate::renderer::RendererCommand::Reload)
                    .expect("Failed to send reload command to renderer");
                }

                _ => {
                    eprintln!("Unknown command");
                }
            }
        });
    }
}
