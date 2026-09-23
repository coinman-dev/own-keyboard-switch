//! Hotkey capture from the settings window's own key messages.
//!
//! While a window of this program is active, Windows does not call the
//! program's low-level keyboard hook, so the engine never sees the
//! combination pressed in the «Назначить комбинацию клавиш» dialog. The GUI
//! event loop reads it from `WM_KEYDOWN`/`WM_SYSKEYDOWN` instead.
#![allow(unsafe_code)]

use okbs_core::{Hotkey, ModState, PhysKey};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_CANCEL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_PAUSE,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{MSG, WM_KEYDOWN, WM_SYSKEYDOWN};

/// What was pressed while the dialog waited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Captured {
    Hotkey(Hotkey),
    /// Esc without modifiers.
    Cancelled,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static RESULT: Mutex<Option<Captured>> = Mutex::new(None);

/// Starts or stops waiting; a result left from an earlier wait is dropped.
pub(crate) fn set_active(on: bool) {
    if ACTIVE.swap(on, Ordering::SeqCst) != on
        && let Ok(mut result) = RESULT.lock()
    {
        *result = None;
    }
}

/// The combination read since the last call.
pub(crate) fn take() -> Option<Captured> {
    RESULT.lock().ok()?.take()
}

/// Message hook of the GUI event loop: looks at every message before it is
/// dispatched and never consumes it. `wake` requests the next frame.
pub(crate) fn observe(msg: *const std::ffi::c_void, wake: impl Fn()) {
    if msg.is_null() || !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    // SAFETY: winit passes the MSG it is about to dispatch, valid for this call.
    let msg = unsafe { &*msg.cast::<MSG>() };
    if !matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
        return;
    }
    let Some(key) = pressed_key(msg.wParam.0, msg.lParam.0) else {
        return;
    };
    if key.is_modifier() {
        return;
    }
    let held = held_modifiers();
    let captured = if key == PhysKey::Escape && held.is_empty() {
        Captured::Cancelled
    } else {
        Captured::Hotkey(Hotkey::pressed(key, held))
    };
    if let Ok(mut result) = RESULT.lock() {
        *result = Some(captured);
    }
    ACTIVE.store(false, Ordering::SeqCst);
    tracing::debug!(target: "okbs_input", ?captured, "hotkey read by the settings window");
    wake();
}

/// The first press of a key (not an auto-repeat) described by a key message.
fn pressed_key(wparam: usize, lparam: isize) -> Option<PhysKey> {
    let flags = lparam as u32;
    if flags & (1 << 30) != 0 {
        return None;
    }
    let vk = u16::try_from(wparam).ok()?;
    // Pause shares its scan code with Num Lock; Ctrl+Pause arrives as Cancel.
    if vk == VK_PAUSE.0 || vk == VK_CANCEL.0 {
        return Some(PhysKey::Pause);
    }
    let scan = ((flags >> 16) & 0xff) as u16;
    PhysKey::from_win_scancode(scan, flags & (1 << 24) != 0)
}

fn held_modifiers() -> ModState {
    const KEYS: [(VIRTUAL_KEY, PhysKey); 8] = [
        (VK_LCONTROL, PhysKey::ControlLeft),
        (VK_RCONTROL, PhysKey::ControlRight),
        (VK_LSHIFT, PhysKey::ShiftLeft),
        (VK_RSHIFT, PhysKey::ShiftRight),
        (VK_LMENU, PhysKey::AltLeft),
        (VK_RMENU, PhysKey::AltRight),
        (VK_LWIN, PhysKey::MetaLeft),
        (VK_RWIN, PhysKey::MetaRight),
    ];
    let mut held = ModState::default();
    for (vk, key) in KEYS {
        // SAFETY: GetKeyState has no pointer arguments; it reports the state
        // as of the message being processed by this thread.
        held.set(key, unsafe { GetKeyState(i32::from(vk.0)) } < 0);
    }
    held
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lparam(scan: u32, extended: bool, repeat: bool) -> isize {
        (1 | scan << 16 | u32::from(extended) << 24 | u32::from(repeat) << 30) as isize
    }

    #[test]
    fn key_messages_map_to_physical_keys() {
        // (virtual key, scan code, extended, auto-repeat) -> key
        let cases = [
            (0x4B, 0x25, false, false, Some(PhysKey::KeyK)),
            (0x7A, 0x57, false, false, Some(PhysKey::F11)),
            (0x2E, 0x53, true, false, Some(PhysKey::Delete)),
            (0x13, 0x45, false, false, Some(PhysKey::Pause)),
            (0x03, 0x46, true, false, Some(PhysKey::Pause)),
            (0x90, 0x45, true, false, Some(PhysKey::NumLock)),
            (0x4B, 0x25, false, true, None),
        ];
        for (vk, scan, extended, repeat, key) in cases {
            let pressed = pressed_key(vk, lparam(scan, extended, repeat));
            assert_eq!(pressed, key, "{vk:#x}");
        }
    }

    #[test]
    fn waiting_again_forgets_the_previous_result() {
        set_active(true);
        if let Ok(mut result) = RESULT.lock() {
            *result = Some(Captured::Cancelled);
        }
        set_active(false);
        assert_eq!(take(), None);
    }
}
