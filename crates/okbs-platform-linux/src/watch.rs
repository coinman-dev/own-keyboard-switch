use okbs_platform::StopGuard;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct Watch {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Watch {
    pub(crate) fn spawn(
        mut step: impl FnMut() -> bool + Send + 'static,
        interval: Duration,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let thread = std::thread::spawn(move || {
            while !signal.load(Ordering::Acquire) && step() {
                std::thread::sleep(interval);
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl StopGuard for Watch {
    fn stop(self: Box<Self>) {
        drop(self);
    }
}
