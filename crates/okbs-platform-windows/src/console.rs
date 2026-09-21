//! Console handling for a GUI-subsystem executable.
#![allow(unsafe_code)]

use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

/// Attaches to the console of the parent process (e.g. `cmd.exe`), so that
/// `--help` and `--diagnose` print text when the release build has no console
/// of its own. Returns `false` when started without a parent console.
pub fn attach_parent_console() -> bool {
    // SAFETY: AttachConsole has no pointer arguments; failure is reported as an error value.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok()
}

/// Errors during an Explorer launch must remain visible even before logging starts.
pub fn show_startup_error(message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    // SAFETY: both strings live for the synchronous dialog call.
    unsafe {
        MessageBoxW(
            None,
            &windows::core::HSTRING::from(message),
            windows::core::w!("Own Keyboard Switch"),
            MB_OK | MB_ICONERROR,
        );
    }
}
