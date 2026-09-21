//! Sound output with `PlaySound`.
#![allow(unsafe_code)]

use okbs_platform::{PlatformError, Result, SoundPlayer};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::Win32::Media::Audio::{
    PlaySoundW, SND_ASYNC, SND_FILENAME, SND_MEMORY, SND_NODEFAULT,
};
use windows::Win32::System::Diagnostics::Debug::Beep;
use windows::core::PCWSTR;

/// Asynchronous WAV playback.
#[derive(Debug, Default)]
pub struct WinSound;

impl SoundPlayer for WinSound {
    fn play_wav(&self, data: &'static [u8]) -> Result<()> {
        // SAFETY: with SND_MEMORY the pointer refers to a complete WAV image that lives
        // for the whole program ('static), as required by SND_ASYNC.
        let ok = unsafe {
            PlaySoundW(
                PCWSTR(data.as_ptr().cast()),
                None,
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            )
        };
        if ok.as_bool() {
            Ok(())
        } else {
            Err(PlatformError::Other("PlaySound failed".into()))
        }
    }

    fn play_file(&self, path: &Path) -> Result<()> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `wide` is NUL-terminated and PlaySound copies the file name before returning.
        let ok = unsafe {
            PlaySoundW(
                PCWSTR(wide.as_ptr()),
                None,
                SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
            )
        };
        if ok.as_bool() {
            Ok(())
        } else {
            Err(PlatformError::Other(format!(
                "cannot play {}",
                path.display()
            )))
        }
    }

    fn beep(&self) -> Result<()> {
        std::thread::spawn(|| {
            // SAFETY: Beep has no pointer arguments.
            let _ = unsafe { Beep(750, 60) };
        });
        Ok(())
    }
}
