use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub struct StopServiceHandle(Arc<AtomicBool>);

impl StopServiceHandle {
    pub fn new() -> Self {
        StopServiceHandle(Arc::new(AtomicBool::new(false)))
    }
    pub fn should_stop(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        self.0.store(true, Ordering::Release)
    }
}