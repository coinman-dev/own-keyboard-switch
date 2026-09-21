//! Windows backend of Own Keyboard Switch.
//!
//! * input: `WH_KEYBOARD_LL` and `WH_MOUSE_LL` hooks on a dedicated thread;
//! * injection: `SendInput` with scan codes, tagged so the hook ignores them;
//! * layouts: `GetKeyboardLayout`, `WM_INPUTLANGCHANGEREQUEST`, `ToUnicodeEx`;
//! * clipboard: `arboard`; sound: `PlaySound`; focus: `SetWinEventHook`.
#![cfg(windows)]

pub mod autoreplace_ui;
pub mod autostart;
pub mod clipboard;
pub mod console;
pub mod diagnose;
pub mod elevation;
pub mod focus;
pub mod hook;
pub mod indicator;
pub mod inject;
pub mod layouts;
pub mod locale;
pub mod sound;
pub mod window_control;

mod message_thread;

pub use message_thread::pump_messages;

pub use autoreplace_ui::WinAutoreplaceUi;
pub use autostart::WinAutostart;
pub use clipboard::WinClipboard;
pub use diagnose::diagnose;
pub use elevation::WinElevation;
pub use focus::WinFocus;
pub use hook::{HookFilter, HookSource};
pub use indicator::WinIndicator;
pub use inject::SendInputInjector;
pub use layouts::WinLayouts;
pub use sound::WinSound;
pub use window_control::WinWindowControl;

/// Marker in `dwExtraInfo` of events injected by this program ("OKBS").
pub const INJECT_TAG: usize = 0x4F4B_4253;
