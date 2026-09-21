//! «Свернуть активное окно» and «Развернуть/восстановить активное окно».
#![allow(unsafe_code)]

use okbs_platform::{PlatformError, Result, WindowControl};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsZoomed,
    SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, ShowWindow,
};

/// The desktop and the taskbar belong to the shell: a hotkey must never
/// minimize or maximize them.
const SHELL_CLASSES: [&str; 5] = [
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "Progman",
    "WorkerW",
];

const WS_MINIMIZEBOX: isize = 0x0002_0000;
const WS_MAXIMIZEBOX: isize = 0x0001_0000;

/// Commands for the foreground window through `ShowWindow`.
#[derive(Debug, Default)]
pub struct WinWindowControl;

/// The foreground window, unless it belongs to this program or to the shell.
fn target() -> Result<HWND> {
    // SAFETY: no arguments; the handle is only compared and queried below.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return Err(PlatformError::Other("no foreground window".into()));
    }
    let mut pid = 0;
    // SAFETY: `pid` is a valid out pointer for a live window handle.
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == std::process::id() {
        return Err(PlatformError::Other("the foreground window is ours".into()));
    }
    if SHELL_CLASSES.contains(&crate::layouts::window_class(window).as_str()) {
        return Err(PlatformError::Other(
            "the foreground window belongs to the shell".into(),
        ));
    }
    Ok(window)
}

fn style(window: HWND) -> isize {
    // SAFETY: a live window handle checked by `target`.
    unsafe { GetWindowLongPtrW(window, GWL_STYLE) }
}

impl WindowControl for WinWindowControl {
    fn minimize_active(&self) -> Result<()> {
        let window = target()?;
        if style(window) & WS_MINIMIZEBOX == 0 {
            return Err(PlatformError::Other(
                "this window cannot be minimized".into(),
            ));
        }
        // SAFETY: a live window handle; the return value only reports the
        // previous visibility.
        unsafe {
            let _ = ShowWindow(window, SW_MINIMIZE);
        }
        Ok(())
    }

    fn toggle_maximize_active(&self) -> Result<()> {
        let window = target()?;
        // SAFETY: a live window handle checked by `target`.
        let maximized = unsafe { IsZoomed(window) }.as_bool();
        if !maximized && style(window) & WS_MAXIMIZEBOX == 0 {
            return Err(PlatformError::Other(
                "this window cannot be maximized".into(),
            ));
        }
        // SAFETY: a live window handle; the return value only reports the
        // previous visibility.
        unsafe {
            let _ = ShowWindow(window, if maximized { SW_RESTORE } else { SW_MAXIMIZE });
        }
        Ok(())
    }
}
