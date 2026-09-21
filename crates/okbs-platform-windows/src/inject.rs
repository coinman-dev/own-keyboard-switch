//! Keyboard injection with `SendInput`.
#![allow(unsafe_code)]

use crate::INJECT_TAG;
use okbs_platform::{Injector, KeyStroke, PlatformError, Result};
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_PACKET,
};

/// Injects keys as hardware scan codes, tagged with [`INJECT_TAG`].
#[derive(Debug, Clone, Default)]
pub struct SendInputInjector {
    /// Pause between events; zero sends everything in one atomic call.
    pub key_delay: Duration,
}

fn keyboard_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECT_TAG,
            },
        },
    }
}

/// Converts a stroke into a scan-code `INPUT`.
pub fn stroke_input(stroke: KeyStroke) -> INPUT {
    let (scan, extended) = stroke.key.win_scancode();
    let mut flags = KEYEVENTF_SCANCODE;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !stroke.pressed {
        flags |= KEYEVENTF_KEYUP;
    }
    keyboard_input(0, scan, flags)
}

fn send_all(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    // SAFETY: `inputs` is a valid slice of INPUT structures of the declared size.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(PlatformError::Other(format!(
            "SendInput injected {sent} of {} events (blocked by UIPI or another program)",
            inputs.len()
        )))
    }
}

impl Injector for SendInputInjector {
    fn send_unmapped(&mut self, code: u32, pressed: bool) -> Result<()> {
        let vk = (code & 0xff) as u16;
        let scan = (code >> 16) as u16;
        let mut flags = if pressed {
            KEYBD_EVENT_FLAGS(0)
        } else {
            KEYEVENTF_KEYUP
        };
        if vk == VK_PACKET.0 {
            send_all(&[keyboard_input(0, scan, flags | KEYEVENTF_UNICODE)])
        } else {
            if code & 0x100 != 0 {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            send_all(&[keyboard_input(vk, scan, flags)])
        }
    }
    fn send(&mut self, strokes: &[KeyStroke]) -> Result<()> {
        let inputs: Vec<INPUT> = strokes.iter().map(|&s| stroke_input(s)).collect();
        if self.key_delay.is_zero() {
            return send_all(&inputs);
        }
        for input in inputs.chunks(1) {
            send_all(input)?;
            std::thread::sleep(self.key_delay);
        }
        Ok(())
    }

    fn type_unicode(&mut self, text: &str) -> Result<()> {
        let inputs: Vec<INPUT> = text
            .encode_utf16()
            .flat_map(|unit| {
                [
                    keyboard_input(0, unit, KEYEVENTF_UNICODE),
                    keyboard_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
                ]
            })
            .collect();
        send_all(&inputs)
    }
}
