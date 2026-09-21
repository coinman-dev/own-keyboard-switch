//! Hotkeys such as `Shift+Break` or `Ctrl+Alt+V`.
//!
//! String format: modifiers followed by one key, separated by `+`.
//! Modifiers are `Ctrl`, `Shift`, `Alt`, `Win` (either side) or their sided
//! variants `LCtrl`, `RCtrl`, `LShift`, `RShift`, `LAlt`, `RAlt`, `LWin`, `RWin`.
//! Aliases `Control`, `Super`, `Meta` and `Cmd` are accepted. Parsing is
//! case-insensitive; [`Hotkey`]'s `Display` produces the canonical form.

use crate::keys::PhysKey;
use std::fmt;
use std::str::FromStr;

/// Which physical side of a modifier is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Left or right.
    Any,
    /// Only the left key.
    Left,
    /// Only the right key.
    Right,
}

/// Modifier requirements of a hotkey. `None` means the modifier must not be held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Control.
    pub ctrl: Option<Side>,
    /// Shift.
    pub shift: Option<Side>,
    /// Alt.
    pub alt: Option<Side>,
    /// Windows / Super.
    pub win: Option<Side>,
}

impl Modifiers {
    /// No modifiers.
    pub const NONE: Modifiers = Modifiers {
        ctrl: None,
        shift: None,
        alt: None,
        win: None,
    };
    /// `Ctrl` on either side.
    pub const CTRL: Modifiers = Modifiers {
        ctrl: Some(Side::Any),
        ..Modifiers::NONE
    };
    /// `Shift` on either side.
    pub const SHIFT: Modifiers = Modifiers {
        shift: Some(Side::Any),
        ..Modifiers::NONE
    };
    /// `Alt` on either side.
    pub const ALT: Modifiers = Modifiers {
        alt: Some(Side::Any),
        ..Modifiers::NONE
    };
    /// `Win` on either side.
    pub const WIN: Modifiers = Modifiers {
        win: Some(Side::Any),
        ..Modifiers::NONE
    };

    /// Union of two modifier sets (the right-hand side wins on conflicts).
    pub const fn with(self, other: Modifiers) -> Modifiers {
        Modifiers {
            ctrl: if other.ctrl.is_some() {
                other.ctrl
            } else {
                self.ctrl
            },
            shift: if other.shift.is_some() {
                other.shift
            } else {
                self.shift
            },
            alt: if other.alt.is_some() {
                other.alt
            } else {
                self.alt
            },
            win: if other.win.is_some() {
                other.win
            } else {
                self.win
            },
        }
    }

    /// `true` when no modifier is required.
    pub const fn is_empty(self) -> bool {
        self.ctrl.is_none() && self.shift.is_none() && self.alt.is_none() && self.win.is_none()
    }
}

/// Physical modifier keys currently held down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ModState {
    bits: u8,
}

impl ModState {
    const KEYS: [PhysKey; 8] = [
        PhysKey::ControlLeft,
        PhysKey::ControlRight,
        PhysKey::ShiftLeft,
        PhysKey::ShiftRight,
        PhysKey::AltLeft,
        PhysKey::AltRight,
        PhysKey::MetaLeft,
        PhysKey::MetaRight,
    ];

    fn bit(key: PhysKey) -> Option<u8> {
        Self::KEYS.iter().position(|&k| k == key).map(|i| 1u8 << i)
    }

    /// Records a press or release. Non-modifier keys are ignored.
    pub fn set(&mut self, key: PhysKey, pressed: bool) {
        if let Some(bit) = Self::bit(key) {
            if pressed {
                self.bits |= bit;
            } else {
                self.bits &= !bit;
            }
        }
    }

    /// Whether the given modifier key is held.
    pub fn is_pressed(self, key: PhysKey) -> bool {
        Self::bit(key).is_some_and(|bit| self.bits & bit != 0)
    }

    /// Either `Ctrl` is held.
    pub fn ctrl(self) -> bool {
        self.is_pressed(PhysKey::ControlLeft) || self.is_pressed(PhysKey::ControlRight)
    }

    /// Either `Shift` is held.
    pub fn shift(self) -> bool {
        self.is_pressed(PhysKey::ShiftLeft) || self.is_pressed(PhysKey::ShiftRight)
    }

    /// Either `Alt` is held.
    pub fn alt(self) -> bool {
        self.is_pressed(PhysKey::AltLeft) || self.is_pressed(PhysKey::AltRight)
    }

    /// Either `Win`/`Super` is held.
    pub fn win(self) -> bool {
        self.is_pressed(PhysKey::MetaLeft) || self.is_pressed(PhysKey::MetaRight)
    }

    /// No modifier is held.
    pub fn is_empty(self) -> bool {
        self.bits == 0
    }
}

/// A key combination: modifiers plus one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    /// Required modifiers.
    pub mods: Modifiers,
    /// The key that triggers the hotkey.
    pub key: PhysKey,
}

impl Hotkey {
    /// Creates a hotkey.
    pub const fn new(mods: Modifiers, key: PhysKey) -> Self {
        Self { mods, key }
    }

    /// A hotkey without modifiers.
    pub const fn key(key: PhysKey) -> Self {
        Self::new(Modifiers::NONE, key)
    }

    /// Whether pressing `key` while `state` modifiers are held triggers this hotkey.
    ///
    /// Modifiers that are not part of the hotkey must not be held, so
    /// `Shift+Break` does not trigger a plain `Break` binding. When the hotkey
    /// key is itself a modifier (`RCtrl`), its own state is ignored.
    pub fn matches(&self, key: PhysKey, state: ModState) -> bool {
        if key != self.key {
            return false;
        }
        let mut state = state;
        state.set(self.key, false);
        let group = |req: Option<Side>, left: PhysKey, right: PhysKey| {
            let (l, r) = (state.is_pressed(left), state.is_pressed(right));
            match req {
                None => !l && !r,
                Some(Side::Any) => l || r,
                Some(Side::Left) => l,
                Some(Side::Right) => r,
            }
        };
        group(self.mods.ctrl, PhysKey::ControlLeft, PhysKey::ControlRight)
            && group(self.mods.shift, PhysKey::ShiftLeft, PhysKey::ShiftRight)
            && group(self.mods.alt, PhysKey::AltLeft, PhysKey::AltRight)
            && group(self.mods.win, PhysKey::MetaLeft, PhysKey::MetaRight)
    }
}

/// Error returned when a hotkey string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyParseError {
    /// The string is empty.
    #[error("hotkey is empty")]
    Empty,
    /// A part between `+` separators is empty, e.g. `Ctrl++A`.
    #[error("hotkey `{0}` contains an empty part")]
    EmptyPart(String),
    /// Unknown key or modifier name.
    #[error("unknown key `{0}`")]
    UnknownKey(String),
    /// The same modifier is listed twice.
    #[error("modifier `{0}` is repeated")]
    DuplicateModifier(String),
    /// Only modifiers, no key: `Ctrl+Alt`.
    #[error("hotkey `{0}` has no key, only modifiers")]
    MissingKey(String),
}

#[derive(Clone, Copy)]
enum ModKind {
    Ctrl,
    Shift,
    Alt,
    Win,
}

fn parse_modifier(token: &str) -> Option<(ModKind, Side)> {
    let t = token.to_ascii_lowercase();
    let (side, base) = if let Some(rest) = t.strip_prefix('l').filter(|r| is_mod_base(r)) {
        (Side::Left, rest.to_string())
    } else if let Some(rest) = t.strip_prefix('r').filter(|r| is_mod_base(r)) {
        (Side::Right, rest.to_string())
    } else {
        (Side::Any, t)
    };
    let kind = match base.as_str() {
        "ctrl" | "control" => ModKind::Ctrl,
        "shift" => ModKind::Shift,
        "alt" => ModKind::Alt,
        "win" | "super" | "meta" | "cmd" => ModKind::Win,
        _ => return None,
    };
    Some((kind, side))
}

fn is_mod_base(s: &str) -> bool {
    matches!(
        s,
        "ctrl" | "control" | "shift" | "alt" | "win" | "super" | "meta" | "cmd"
    )
}

fn modifier_key(kind: ModKind, side: Side) -> Option<PhysKey> {
    Some(match (kind, side) {
        (_, Side::Any) => return None,
        (ModKind::Ctrl, Side::Left) => PhysKey::ControlLeft,
        (ModKind::Ctrl, Side::Right) => PhysKey::ControlRight,
        (ModKind::Shift, Side::Left) => PhysKey::ShiftLeft,
        (ModKind::Shift, Side::Right) => PhysKey::ShiftRight,
        (ModKind::Alt, Side::Left) => PhysKey::AltLeft,
        (ModKind::Alt, Side::Right) => PhysKey::AltRight,
        (ModKind::Win, Side::Left) => PhysKey::MetaLeft,
        (ModKind::Win, Side::Right) => PhysKey::MetaRight,
    })
}

impl FromStr for Hotkey {
    type Err = HotkeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(HotkeyParseError::Empty);
        }
        let tokens: Vec<&str> = s.split('+').map(str::trim).collect();
        if tokens.iter().any(|t| t.is_empty()) {
            return Err(HotkeyParseError::EmptyPart(s.to_string()));
        }
        let (last, mod_tokens) = tokens.split_last().ok_or(HotkeyParseError::Empty)?;

        let mut mods = Modifiers::NONE;
        for token in mod_tokens {
            let (kind, side) = parse_modifier(token)
                .ok_or_else(|| HotkeyParseError::UnknownKey(token.to_string()))?;
            let slot = match kind {
                ModKind::Ctrl => &mut mods.ctrl,
                ModKind::Shift => &mut mods.shift,
                ModKind::Alt => &mut mods.alt,
                ModKind::Win => &mut mods.win,
            };
            if slot.is_some() {
                return Err(HotkeyParseError::DuplicateModifier(token.to_string()));
            }
            *slot = Some(side);
        }

        let key = match PhysKey::from_name(last) {
            Some(key) => key,
            None => match parse_modifier(last) {
                Some((kind, side)) => modifier_key(kind, side)
                    .ok_or_else(|| HotkeyParseError::MissingKey(s.to_string()))?,
                None => return Err(HotkeyParseError::UnknownKey(last.to_string())),
            },
        };
        Ok(Hotkey { mods, key })
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = [
            (self.mods.ctrl, "Ctrl"),
            (self.mods.shift, "Shift"),
            (self.mods.alt, "Alt"),
            (self.mods.win, "Win"),
        ];
        for (side, name) in parts {
            match side {
                None => {}
                Some(Side::Any) => write!(f, "{name}+")?,
                Some(Side::Left) => write!(f, "L{name}+")?,
                Some(Side::Right) => write!(f, "R{name}+")?,
            }
        }
        f.write_str(self.key.hotkey_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(s: &str) -> Hotkey {
        s.parse().unwrap()
    }

    fn state(keys: &[PhysKey]) -> ModState {
        let mut st = ModState::default();
        for &k in keys {
            st.set(k, true);
        }
        st
    }

    #[test]
    fn parse_default_hotkeys() {
        assert_eq!(hk("Break"), Hotkey::key(PhysKey::Pause));
        assert_eq!(
            hk("Shift+Break"),
            Hotkey::new(Modifiers::SHIFT, PhysKey::Pause)
        );
        assert_eq!(hk("Alt+Break"), Hotkey::new(Modifiers::ALT, PhysKey::Pause));
        assert_eq!(
            hk("Alt+Scroll"),
            Hotkey::new(Modifiers::ALT, PhysKey::ScrollLock)
        );
        assert_eq!(
            hk("ctrl + alt + v"),
            Hotkey::new(Modifiers::CTRL.with(Modifiers::ALT), PhysKey::KeyV)
        );
        assert_eq!(
            hk("Ctrl+Win+Alt+P"),
            Hotkey::new(
                Modifiers::CTRL.with(Modifiers::WIN).with(Modifiers::ALT),
                PhysKey::KeyP
            )
        );
    }

    #[test]
    fn parse_sided_and_bare_modifiers() {
        let h = hk("RCtrl+LShift+F12");
        assert_eq!(h.mods.ctrl, Some(Side::Right));
        assert_eq!(h.mods.shift, Some(Side::Left));
        assert_eq!(h.key, PhysKey::F12);
        assert_eq!(hk("RCtrl"), Hotkey::key(PhysKey::ControlRight));
        assert_eq!(hk("Super+Space").mods.win, Some(Side::Any));
        assert_eq!(hk("Ctrl+=").key, PhysKey::Equal);
    }

    #[test]
    fn parse_errors() {
        assert_eq!("".parse::<Hotkey>(), Err(HotkeyParseError::Empty));
        assert_eq!("  ".parse::<Hotkey>(), Err(HotkeyParseError::Empty));
        assert!(matches!(
            "Ctrl++A".parse::<Hotkey>(),
            Err(HotkeyParseError::EmptyPart(_))
        ));
        assert!(matches!(
            "Ctrl+".parse::<Hotkey>(),
            Err(HotkeyParseError::EmptyPart(_))
        ));
        assert!(matches!(
            "Ctrl+Foo".parse::<Hotkey>(),
            Err(HotkeyParseError::UnknownKey(_))
        ));
        assert!(matches!(
            "Hyper+A".parse::<Hotkey>(),
            Err(HotkeyParseError::UnknownKey(_))
        ));
        assert!(matches!(
            "Ctrl+LCtrl+A".parse::<Hotkey>(),
            Err(HotkeyParseError::DuplicateModifier(_))
        ));
        assert!(matches!(
            "Ctrl+Alt".parse::<Hotkey>(),
            Err(HotkeyParseError::MissingKey(_))
        ));
        assert!(matches!(
            "Shift".parse::<Hotkey>(),
            Err(HotkeyParseError::MissingKey(_))
        ));
    }

    #[test]
    fn display_roundtrip() {
        for s in [
            "Break",
            "Shift+Break",
            "Ctrl+Alt+V",
            "LCtrl+RShift+Alt+Win+F24",
            "Alt+ScrollLock",
            "RCtrl",
            "Ctrl+`",
            "Win+\\",
        ] {
            let h = hk(s);
            assert_eq!(h.to_string(), s);
            assert_eq!(hk(&h.to_string()), h);
        }
        assert_eq!(hk("control+pause").to_string(), "Ctrl+Break");
    }

    #[test]
    fn matching() {
        let brk = hk("Break");
        let shift_brk = hk("Shift+Break");
        let rshift_brk = hk("RShift+Break");
        let none = ModState::default();
        let lshift = state(&[PhysKey::ShiftLeft]);
        let rshift = state(&[PhysKey::ShiftRight]);

        assert!(brk.matches(PhysKey::Pause, none));
        assert!(!brk.matches(PhysKey::Pause, lshift));
        assert!(!brk.matches(PhysKey::ScrollLock, none));
        assert!(shift_brk.matches(PhysKey::Pause, lshift));
        assert!(shift_brk.matches(PhysKey::Pause, rshift));
        assert!(!shift_brk.matches(PhysKey::Pause, none));
        assert!(rshift_brk.matches(PhysKey::Pause, rshift));
        assert!(!rshift_brk.matches(PhysKey::Pause, lshift));

        let ctrl_alt_v = hk("Ctrl+Alt+V");
        assert!(ctrl_alt_v.matches(
            PhysKey::KeyV,
            state(&[PhysKey::ControlLeft, PhysKey::AltRight])
        ));
        assert!(!ctrl_alt_v.matches(
            PhysKey::KeyV,
            state(&[PhysKey::ControlLeft, PhysKey::AltLeft, PhysKey::ShiftLeft])
        ));
    }

    #[test]
    fn bare_modifier_ignores_own_state() {
        let rctrl = hk("RCtrl");
        assert!(rctrl.matches(PhysKey::ControlRight, state(&[PhysKey::ControlRight])));
        assert!(!rctrl.matches(
            PhysKey::ControlRight,
            state(&[PhysKey::ControlRight, PhysKey::ControlLeft])
        ));
    }

    #[test]
    fn mod_state() {
        let mut st = ModState::default();
        assert!(st.is_empty());
        st.set(PhysKey::AltRight, true);
        st.set(PhysKey::KeyA, true);
        assert!(st.alt() && !st.ctrl() && !st.shift() && !st.win());
        st.set(PhysKey::AltRight, false);
        assert!(st.is_empty());
    }
}
