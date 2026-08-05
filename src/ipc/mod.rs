use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
};

const SOCKET_PATH: &str = "/tmp/wallman.sock";

pub fn start_server<F>(mut handler: F)
where
    F: FnMut(String) + 'static,
{
    let _ = fs::remove_file(SOCKET_PATH);

    let listener =
        UnixListener::bind(SOCKET_PATH)
            .expect("Failed to create socket");

    println!("wallman IPC listening on {}", SOCKET_PATH);

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
    let mut stream =
    UnixStream::connect(SOCKET_PATH)
    .expect("Could not connect to wallman daemon");

    stream
    .write_all(command.as_bytes())
    .expect("Failed to send command");
}
