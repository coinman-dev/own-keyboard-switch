//! Clipboard text through `arboard`, plus change notifications for
//! «Следить за буфером обмена».
#![allow(unsafe_code)]

use crate::hook::os_error;
use crossbeam_channel::Sender;
use okbs_platform::{Clipboard, PlatformError, Result, StopGuard};
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, RegisterClassExW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSEXW,
};
use windows::core::{PCWSTR, w};

/// System clipboard. A new handle is opened per operation, so the value is `Send`.
#[derive(Debug, Default)]
pub struct WinClipboard;

fn open() -> Result<arboard::Clipboard> {
    arboard::Clipboard::new().map_err(|e| PlatformError::Other(format!("clipboard: {e}")))
}

const LISTENER_CLASS: PCWSTR = w!("OwnKeyboardSwitch.ClipboardListener");

thread_local! {
    static SINK: RefCell<Option<Sender<()>>> = const { RefCell::new(None) };
}

unsafe extern "system" fn listener_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLIPBOARDUPDATE {
        SINK.with_borrow(|sink| {
            if let Some(sink) = sink {
                let _ = sink.send(());
            }
        });
        return LRESULT(0);
    }
    // SAFETY: the default handler for messages this window does not implement.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

impl Clipboard for WinClipboard {
    fn text(&mut self) -> Result<Option<String>> {
        match open()?.get_text() {
            Ok(text) => Ok(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(PlatformError::Other(format!("clipboard: {e}"))),
        }
    }

    fn set_text(&mut self, text: &str) -> Result<()> {
        open()?
            .set_text(text)
            .map_err(|e| PlatformError::Other(format!("clipboard: {e}")))
    }

    fn clear(&mut self) -> Result<()> {
        open()?
            .clear()
            .map_err(|e| PlatformError::Other(format!("clipboard: {e}")))
    }

    /// A message-only window receives `WM_CLIPBOARDUPDATE`; the notification
    /// carries no text, so nothing sensitive crosses the channel.
    fn subscribe(&mut self, sink: Sender<()>) -> Result<Box<dyn StopGuard>> {
        crate::message_thread::spawn("okbs-clipboard", move || {
            SINK.with_borrow_mut(|s| *s = Some(sink));
            // SAFETY: the class is registered once per process and the window
            // is destroyed by the cleanup closure below.
            let window = unsafe {
                let instance = GetModuleHandleW(None).unwrap_or_default();
                let class = WNDCLASSEXW {
                    cbSize: size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(listener_proc),
                    hInstance: instance.into(),
                    lpszClassName: LISTENER_CLASS,
                    ..Default::default()
                };
                RegisterClassExW(&class);
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    LISTENER_CLASS,
                    PCWSTR::null(),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    Some(instance.into()),
                    None,
                )
                .map_err(|e| os_error("create clipboard listener", &e))?
            };
            // SAFETY: a live message-only window of this thread.
            unsafe { AddClipboardFormatListener(window) }
                .map_err(|e| os_error("AddClipboardFormatListener", &e))?;
            Ok(move || {
                // SAFETY: the listener and the window were created above.
                unsafe {
                    let _ = RemoveClipboardFormatListener(window);
                    let _ = DestroyWindow(window);
                }
                SINK.with_borrow_mut(|s| *s = None);
            })
        })
    }
}
