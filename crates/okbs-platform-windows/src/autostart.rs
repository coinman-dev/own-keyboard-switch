//! «Запускаться при старте»: the `Run` registry key of the current user.
//!
//! `Run` always starts the program without administrator rights. When
//! «Запускать с правами Администратора» is on, a Task Scheduler entry with the
//! highest run level takes its place, so the program keeps its rights at logon
//! without a UAC prompt every time. Registering such a task itself requires
//! administrator rights, so it is only written from an elevated process.
#![allow(unsafe_code)]

use okbs_platform::{Autostart, PlatformError, Result};
use std::path::Path;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "OwnKeyboardSwitch";
/// Name of the Task Scheduler entry used for the elevated registration.
const TASK_NAME: &str = "Own Keyboard Switch";

/// Start at login through `HKCU\...\Run` or a scheduled task.
#[derive(Debug, Default)]
pub struct WinAutostart;

fn check(err: WIN32_ERROR, what: &str) -> Result<()> {
    if err == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(PlatformError::Os {
            code: i64::from(err.0),
            context: what.to_string(),
        })
    }
}

struct Key(HKEY);

impl Key {
    fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<Self> {
        let mut key = HKEY::default();
        let path = HSTRING::from(RUN_KEY);
        // SAFETY: valid NUL-terminated path and out pointer.
        check(
            unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(path.as_ptr()),
                    None,
                    access,
                    &mut key,
                )
            },
            "RegOpenKeyExW(Run)",
        )?;
        Ok(Self(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by RegOpenKeyExW.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// Command line stored in the registry for `executable`.
pub fn command_line(executable: &Path) -> String {
    format!("\"{}\"", executable.display())
}

fn run_key_enabled() -> Result<bool> {
    let key = Key::open(KEY_QUERY_VALUE)?;
    let name = HSTRING::from(VALUE_NAME);
    // SAFETY: querying only whether the value exists.
    let err = unsafe { RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, None, None, None) };
    if err == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    check(err, "RegQueryValueExW(Run)")?;
    Ok(true)
}

fn set_run_key(enabled: bool, executable: &Path) -> Result<()> {
    let key = Key::open(KEY_SET_VALUE)?;
    let name = HSTRING::from(VALUE_NAME);
    if enabled {
        let value: Vec<u8> = command_line(executable)
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        // SAFETY: `value` is a NUL-terminated UTF-16 string as REG_SZ requires.
        check(
            unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&value)) },
            "RegSetValueExW(Run)",
        )
    } else {
        // SAFETY: valid key and value name.
        let err = unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) };
        if err == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(err, "RegDeleteValueW(Run)")
    }
}

/// Runs `schtasks.exe` without flashing a console window.
fn schtasks(arguments: &[&str]) -> Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`: the tray application has no console to inherit.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("schtasks.exe")
        .args(arguments)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|err| PlatformError::Other(format!("cannot run schtasks.exe: {err}")))
}

fn task_exists() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME]).is_ok_and(|out| out.status.success())
}

/// `DOMAIN\user` of the current session, as `schtasks /RU` expects it.
fn current_user() -> Option<String> {
    let name = std::env::var("USERNAME").ok().filter(|n| !n.is_empty())?;
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => Some(format!("{domain}\\{name}")),
        _ => Some(name),
    }
}

fn set_task(enabled: bool, executable: &Path) -> Result<()> {
    if !enabled {
        if !task_exists() {
            return Ok(());
        }
        let output = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?;
        return task_result(&output, "schtasks /Delete");
    }
    let user = current_user()
        .ok_or_else(|| PlatformError::Other("cannot determine the current user".into()))?;
    let target = command_line(executable);
    // `/IT` runs the task in the interactive session of `/RU` and needs no
    // stored password; `/RL HIGHEST` is what keeps the rights at logon.
    let output = schtasks(&[
        "/Create", "/TN", TASK_NAME, "/TR", &target, "/SC", "ONLOGON", "/RL", "HIGHEST", "/RU",
        &user, "/IT", "/F",
    ])?;
    task_result(&output, "schtasks /Create")
}

fn task_result(output: &std::process::Output, what: &str) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr);
    let message = message.trim();
    let message = if message.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        message.to_string()
    };
    Err(PlatformError::Other(format!("{what}: {message}")))
}

impl Autostart for WinAutostart {
    fn is_enabled(&self) -> Result<bool> {
        if task_exists() {
            return Ok(true);
        }
        run_key_enabled()
    }

    fn elevated_at_login(&self) -> Result<bool> {
        Ok(task_exists())
    }

    fn set_enabled(&self, enabled: bool, executable: &Path, elevated: bool) -> Result<()> {
        // Only an elevated process may write or remove a highest-run-level
        // task; an ordinary one keeps the `Run` value and reports why.
        let can_use_task = crate::elevation::is_elevated();
        if !enabled {
            set_run_key(false, executable)?;
            return if can_use_task {
                set_task(false, executable)
            } else if task_exists() {
                Err(PlatformError::Other(
                    "administrator rights are required to remove the logon task".into(),
                ))
            } else {
                Ok(())
            };
        }
        if elevated && can_use_task {
            set_task(true, executable)?;
            return set_run_key(false, executable);
        }
        if !elevated && can_use_task {
            set_task(false, executable)?;
        }
        set_run_key(true, executable)
    }
}
