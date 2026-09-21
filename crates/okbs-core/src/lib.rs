//! Platform-independent core of Own Keyboard Switch (okbswitch).
//!
//! This crate contains no OS-specific code: physical keys and their platform
//! code tables, hotkey parsing, languages, layout key maps and the user
//! configuration model. Everything here is testable on any OS.

pub mod autoreplace;
pub mod config;
#[cfg(feature = "builtin-data")]
pub mod data;
pub mod detect;
pub mod extra_rules;
pub mod hotkey;
pub mod keymap;
pub mod keys;
pub mod lang;
pub mod layouts;
pub mod lm;
pub mod numwords;
pub mod rules;
#[cfg(feature = "builtin-data")]
mod short_words;
pub mod spell;
pub mod text;
pub mod translit;

pub use hotkey::{Hotkey, ModState, Modifiers, Side};
pub use keymap::KeyMap;
pub use keys::PhysKey;
pub use lang::Lang;

/// Application identifier used for file names, lock files and D-Bus names.
pub const APP_ID: &str = "okbswitch";
/// Human-readable application name.
pub const APP_NAME: &str = "Own Keyboard Switch";
/// Crate version (shared by the whole workspace).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
