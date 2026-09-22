//! User interface of Own Keyboard Switch.
//!
//! * [`i18n`]: interface texts in Russian and English;
//! * [`flags`]: country flags for the tray icon;
//! * [`icon`]: tray icon images;
//! * [`clipboard_history`]: the remembered clipboard texts;
//! * [`settings`]: contents of the settings window;
//! * [`tray`]: tray icon with menu (Windows, Linux);
//! * [`window`]: the settings window thread (Windows, Linux).

mod appearance;
#[cfg(any(windows, target_os = "linux"))]
pub mod autoreplace_list;
mod branding;
#[cfg(any(windows, target_os = "linux"))]
pub mod clipboard_history;
pub mod flags;
pub mod i18n;
pub mod icon;
pub mod settings;
pub mod text_result;
#[cfg(any(windows, target_os = "linux"))]
pub mod tray;
#[cfg(any(windows, target_os = "linux"))]
pub mod window;
mod window_position;

pub use i18n::{Text, hotkey_action_text, tr};
