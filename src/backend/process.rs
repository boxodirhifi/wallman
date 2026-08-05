use std::process::Child;

pub struct ProcessManager {
    child: Option<Child>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            child: None,
        }
    }

    pub fn replace(&mut self, child: Child) {
        self.stop();
        self.child = Some(child);
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
