use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};

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

    println!("wallman IPC listening on {}", socket.display());

    listener
}

pub fn serve<F>(listener: UnixListener, mut handler: F)
where
F: FnMut(String) + 'static,
{
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {

                let reader = BufReader::new(stream);

                for line in reader.lines() {
                    if let Ok(command) = line {
                        handler(command);
                    }
                }
            }

            Err(error) => {
                eprintln!("Socket error: {}", error);
            }
        }
    }
}

pub fn send_command(command: &str) {
    let socket = socket_path();

    let mut stream =
    UnixStream::connect(&socket)
    .expect("Could not connect to wallman daemon");

    writeln!(stream, "{command}")
    .expect("Failed to send command");
}
