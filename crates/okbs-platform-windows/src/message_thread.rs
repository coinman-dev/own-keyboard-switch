//! A thread with a Win32 message loop, stopped by posting `WM_QUIT`.
#![allow(unsafe_code)]

use okbs_platform::{PlatformError, Result, StopGuard};
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostThreadMessageW, TranslateMessage, WM_QUIT,
};

/// Processes pending window messages of the current thread, waiting up to
/// `timeout` for new ones. Returns `false` when `WM_QUIT` was received.
pub fn pump_messages(timeout: std::time::Duration) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    };
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: no handles are passed; the call only waits for queue input.
    unsafe { MsgWaitForMultipleObjects(None, false, ms, QS_ALLINPUT) };
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid buffer; messages are dispatched on the owning thread.
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            return false;
        }
        // SAFETY: `msg` was filled by PeekMessageW.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

/// Runs `setup` on a new thread, then pumps messages until stopped.
/// `setup` returns a cleanup closure executed after the loop ends.
pub(crate) fn spawn<S, C>(name: &str, setup: S) -> Result<Box<dyn StopGuard>>
where
    S: FnOnce() -> Result<C> + Send + 'static,
    C: FnOnce(),
{
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32>>();
    let thread = std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let cleanup = match setup() {
                Ok(cleanup) => cleanup,
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };
            // SAFETY: GetCurrentThreadId has no preconditions.
            let _ = ready_tx.send(Ok(unsafe { GetCurrentThreadId() }));
            let mut msg = MSG::default();
            // SAFETY: `msg` is a valid MSG buffer; the loop ends on WM_QUIT (0) or error (-1).
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
                // SAFETY: `msg` was filled by GetMessageW.
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            cleanup();
        })?;
    let thread_id = ready_rx
        .recv()
        .map_err(|_| PlatformError::Other("message thread exited during setup".into()))??;
    Ok(Box::new(MessageThread {
        thread_id,
        thread: Some(thread),
    }))
}

struct MessageThread {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl MessageThread {
    fn quit(&mut self) {
        // SAFETY: posting WM_QUIT to a thread id has no memory-safety requirements.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl StopGuard for MessageThread {
    fn stop(mut self: Box<Self>) {
        self.quit();
    }
}

impl Drop for MessageThread {
    fn drop(&mut self) {
        self.quit();
    }
}
