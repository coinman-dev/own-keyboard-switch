//! «Запускать с правами Администратора».
//!
//! Without administrator rights a low-level hook never sees keys typed in an
//! elevated window, so nothing is converted there. The option restarts the
//! program through the `runas` verb, which asks the user once through UAC.
#![allow(unsafe_code)]

use okbs_platform::{Elevation, PlatformError, Result};
use std::path::Path;
use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

/// `ShellExecuteW` reports failure as a value of 32 or less.
const SHELL_EXECUTE_MIN_SUCCESS: isize = 32;

/// Administrator rights of this process.
#[derive(Debug, Default)]
pub struct WinElevation;

/// Whether this process runs with administrator rights.
pub fn is_elevated() -> bool {
    // SAFETY: IsUserAnAdmin takes no arguments.
    unsafe { IsUserAnAdmin() }.as_bool()
}

impl Elevation for WinElevation {
    fn is_elevated(&self) -> bool {
        is_elevated()
    }

    fn restart_elevated(&self, executable: &Path, arguments: &[String]) -> Result<()> {
        let file = HSTRING::from(executable.as_os_str());
        // The command line is built by the program itself and only contains
        // paths taken from the configuration, so quoting each argument is enough.
        let arguments = HSTRING::from(
            arguments
                .iter()
                .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
                .collect::<Vec<_>>()
                .join(" "),
        );
        let directory = executable
            .parent()
            .map(|dir| HSTRING::from(dir.as_os_str()));
        // SAFETY: all strings are NUL-terminated and live until the call returns.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("runas"),
                PCWSTR(file.as_ptr()),
                PCWSTR(arguments.as_ptr()),
                directory
                    .as_ref()
                    .map_or(PCWSTR::null(), |dir| PCWSTR(dir.as_ptr())),
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize > SHELL_EXECUTE_MIN_SUCCESS {
            return Ok(());
        }
        // The user declining the UAC prompt is the common case and is not an
        // error of the program; the caller reports it without restarting.
        Err(PlatformError::Os {
            code: result.0 as isize as i64,
            context: "ShellExecuteW(runas)".into(),
        })
    }
}
