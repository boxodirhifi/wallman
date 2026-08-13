use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};
use std::os::unix::fs::PermissionsExt;
use serde::{Deserialize, Serialize};

// Define our strictly-typed IPC protocol
#[derive(Serialize, Deserialize, Debug)]
pub enum Command {
    Set {
        image: String,
        mode: String,
        monitor: String,
        blur: u32,
    },
    Reload,
    Stop,
}

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR")
    .expect("XDG_RUNTIME_DIR is not set");

    PathBuf::from(runtime).join("wallman.sock")
}

pub fn create_listener() -> UnixListener {
    let socket = socket_path();

    if UnixStream::connect(&socket).is_ok() {
        eprintln!("wallman is already running.");
        std::process::exit(1);
    }

    let _ = fs::remove_file(&socket);

    let listener = UnixListener::bind(&socket)
    .expect("Failed to create socket");

    // Restrict socket permissions to owner only
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
    .expect("Failed to set socket permissions");

    println!("wallman IPC listening on {}", socket.display());

    listener
}

pub fn serve<F>(listener: UnixListener, mut handler: F)
where
F: FnMut(Command) + 'static,
{
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    if let Ok(line_str) = line {
                        if let Ok(command) = serde_json::from_str::<Command>(&line_str) {
                            handler(command);
                        } else {
                            eprintln!("Malformed JSON IPC command: {}", line_str);
                        }
                    }
                }
            }
            Err(error) => {
                eprintln!("Socket error: {}", error);
            }
        }
    }
}

pub fn send_command(command: &Command) {
    let socket = socket_path();

    let mut stream = UnixStream::connect(&socket)
    .expect("Could not connect to wallman daemon");

    let payload = serde_json::to_string(command).expect("Failed to serialize command");
    writeln!(stream, "{}", payload).expect("Failed to send command");
}
