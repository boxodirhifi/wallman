use crate::ipc;

pub fn run() {
    if std::os::unix::net::UnixStream::connect(ipc::socket_path()).is_ok() {
        println!("wallman daemon is running");
    } else {
        println!("wallman daemon is not running");
    }
}
