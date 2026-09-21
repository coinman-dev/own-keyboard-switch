//! Linux backend of Own Keyboard Switch.
//!
//! Input is read passively from evdev devices (no exclusive grab) and injected
//! through a uinput virtual keyboard, which works identically on X11, Wayland
//! and any desktop. The active layout is read and changed through a
//! desktop-specific backend (KDE D-Bus, X11 XKB, GNOME Shell extension).
//!
//! Stage 0 contains session detection and the environment diagnostics.
#![cfg(target_os = "linux")]

pub mod access;
pub mod diagnose;
pub mod session;

pub use diagnose::diagnose;
pub use session::{Desktop, SessionInfo, SessionType, resolve_layout_backend};

#[cfg(test)]
mod evdev_codes {
    use okbs_core::PhysKey;
    use std::str::FromStr;

    fn expected_evdev_name(key: PhysKey) -> String {
        let special = match key {
            PhysKey::Escape => "KEY_ESC",
            PhysKey::Equal => "KEY_EQUAL",
            PhysKey::BracketLeft => "KEY_LEFTBRACE",
            PhysKey::BracketRight => "KEY_RIGHTBRACE",
            PhysKey::ControlLeft => "KEY_LEFTCTRL",
            PhysKey::ControlRight => "KEY_RIGHTCTRL",
            PhysKey::ShiftLeft => "KEY_LEFTSHIFT",
            PhysKey::ShiftRight => "KEY_RIGHTSHIFT",
            PhysKey::AltLeft => "KEY_LEFTALT",
            PhysKey::AltRight => "KEY_RIGHTALT",
            PhysKey::MetaLeft => "KEY_LEFTMETA",
            PhysKey::MetaRight => "KEY_RIGHTMETA",
            PhysKey::Quote => "KEY_APOSTROPHE",
            PhysKey::Backquote => "KEY_GRAVE",
            PhysKey::Period => "KEY_DOT",
            PhysKey::NumpadMultiply => "KEY_KPASTERISK",
            PhysKey::NumpadSubtract => "KEY_KPMINUS",
            PhysKey::NumpadAdd => "KEY_KPPLUS",
            PhysKey::NumpadDecimal => "KEY_KPDOT",
            PhysKey::NumpadDivide => "KEY_KPSLASH",
            PhysKey::NumpadEnter => "KEY_KPENTER",
            PhysKey::NumpadEqual => "KEY_KPEQUAL",
            PhysKey::NumpadComma => "KEY_KPCOMMA",
            PhysKey::IntlBackslash => "KEY_102ND",
            PhysKey::PrintScreen => "KEY_SYSRQ",
            PhysKey::ArrowUp => "KEY_UP",
            PhysKey::ArrowDown => "KEY_DOWN",
            PhysKey::ArrowLeft => "KEY_LEFT",
            PhysKey::ArrowRight => "KEY_RIGHT",
            PhysKey::ContextMenu => "KEY_COMPOSE",
            _ => "",
        };
        if !special.is_empty() {
            return special.to_string();
        }
        let name = key.code_name();
        let base = name
            .strip_prefix("Key")
            .or_else(|| name.strip_prefix("Digit"))
            .map(str::to_string)
            .or_else(|| name.strip_prefix("Numpad").map(|n| format!("KP{n}")))
            .unwrap_or_else(|| name.to_string());
        format!("KEY_{}", base.to_ascii_uppercase())
    }

    #[test]
    fn phys_key_evdev_codes_match_evdev_crate() {
        for &key in PhysKey::ALL {
            let name = expected_evdev_name(key);
            let code = evdev::KeyCode::from_str(&name)
                .unwrap_or_else(|_| panic!("evdev has no {name} for {key:?}"));
            assert_eq!(code.code(), key.evdev_code(), "{key:?} vs {name}");
        }
    }
}
