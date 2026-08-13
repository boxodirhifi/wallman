pub fn run() {
    crate::ipc::send_command(&crate::ipc::Command::Stop);
}
