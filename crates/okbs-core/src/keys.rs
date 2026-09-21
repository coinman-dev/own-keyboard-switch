//! Physical keys identified by their position on the keyboard.
//!
//! A [`PhysKey`] does not depend on the active layout: the key right of `Tab`
//! is always [`PhysKey::KeyQ`], whether it prints `q` or `й`. Words are stored
//! as sequences of physical keys so they can be replayed in another layout.
//!
//! Each key carries its Linux evdev code (`linux/input-event-codes.h`) and its
//! Windows set-1 scan code as reported by `WH_KEYBOARD_LL`
//! (`KBDLLHOOKSTRUCT::scanCode` plus the `LLKHF_EXTENDED` flag).

macro_rules! define_keys {
    ($( $name:ident = $evdev:literal, $sc:literal, $ext:literal, $hk:literal; )*) => {
        /// A physical key, independent of the keyboard layout.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum PhysKey {
            $( #[doc = concat!("Hotkey name: `", $hk, "`")] $name, )*
        }

        impl PhysKey {
            /// All known keys in declaration order.
            pub const ALL: &'static [PhysKey] = &[$(PhysKey::$name,)*];
            /// Number of known keys.
            pub const COUNT: usize = Self::ALL.len();

            /// Linux evdev key code (`KEY_*`).
            pub const fn evdev_code(self) -> u16 {
                match self { $(PhysKey::$name => $evdev,)* }
            }

            /// Windows scan code and extended flag as seen by a low-level keyboard hook.
            pub const fn win_scancode(self) -> (u16, bool) {
                match self { $(PhysKey::$name => ($sc, $ext),)* }
            }

            /// Stable identifier, similar to the W3C `KeyboardEvent.code` value.
            pub const fn code_name(self) -> &'static str {
                match self { $(PhysKey::$name => stringify!($name),)* }
            }

            /// Canonical name used in hotkey strings (`"Break"`, `"ScrollLock"`, `"A"`).
            pub const fn hotkey_name(self) -> &'static str {
                match self { $(PhysKey::$name => $hk,)* }
            }

            /// Looks up a key by its Linux evdev code.
            pub const fn from_evdev(code: u16) -> Option<PhysKey> {
                match code {
                    $($evdev => Some(PhysKey::$name),)*
                    _ => None,
                }
            }

            /// Looks up a key by a Windows low-level hook scan code.
            ///
            /// Besides the table values this accepts `Ctrl+Pause` (reported as
            /// extended `0x46`) as [`PhysKey::Pause`], `Alt+PrintScreen`
            /// (reported as `0x54`) as [`PhysKey::PrintScreen`] and a right `Shift`
            /// that some systems report with the extended flag set.
            /// The fake extended `0x2A` shift sent around navigation keys maps to `None`.
            pub const fn from_win_scancode(scancode: u16, extended: bool) -> Option<PhysKey> {
                match (scancode, extended) {
                    $(($sc, $ext) => Some(PhysKey::$name),)*
                    (0x46, true) => Some(PhysKey::Pause),
                    (0x54, false) => Some(PhysKey::PrintScreen),
                    (0x36, true) => Some(PhysKey::ShiftRight),
                    _ => None,
                }
            }
        }
    };
}

define_keys! {
    Escape = 1, 0x01, false, "Esc";
    Digit1 = 2, 0x02, false, "1";
    Digit2 = 3, 0x03, false, "2";
    Digit3 = 4, 0x04, false, "3";
    Digit4 = 5, 0x05, false, "4";
    Digit5 = 6, 0x06, false, "5";
    Digit6 = 7, 0x07, false, "6";
    Digit7 = 8, 0x08, false, "7";
    Digit8 = 9, 0x09, false, "8";
    Digit9 = 10, 0x0A, false, "9";
    Digit0 = 11, 0x0B, false, "0";
    Minus = 12, 0x0C, false, "-";
    Equal = 13, 0x0D, false, "=";
    Backspace = 14, 0x0E, false, "Backspace";
    Tab = 15, 0x0F, false, "Tab";
    KeyQ = 16, 0x10, false, "Q";
    KeyW = 17, 0x11, false, "W";
    KeyE = 18, 0x12, false, "E";
    KeyR = 19, 0x13, false, "R";
    KeyT = 20, 0x14, false, "T";
    KeyY = 21, 0x15, false, "Y";
    KeyU = 22, 0x16, false, "U";
    KeyI = 23, 0x17, false, "I";
    KeyO = 24, 0x18, false, "O";
    KeyP = 25, 0x19, false, "P";
    BracketLeft = 26, 0x1A, false, "[";
    BracketRight = 27, 0x1B, false, "]";
    Enter = 28, 0x1C, false, "Enter";
    ControlLeft = 29, 0x1D, false, "LCtrl";
    KeyA = 30, 0x1E, false, "A";
    KeyS = 31, 0x1F, false, "S";
    KeyD = 32, 0x20, false, "D";
    KeyF = 33, 0x21, false, "F";
    KeyG = 34, 0x22, false, "G";
    KeyH = 35, 0x23, false, "H";
    KeyJ = 36, 0x24, false, "J";
    KeyK = 37, 0x25, false, "K";
    KeyL = 38, 0x26, false, "L";
    Semicolon = 39, 0x27, false, ";";
    Quote = 40, 0x28, false, "'";
    Backquote = 41, 0x29, false, "`";
    ShiftLeft = 42, 0x2A, false, "LShift";
    Backslash = 43, 0x2B, false, "\\";
    KeyZ = 44, 0x2C, false, "Z";
    KeyX = 45, 0x2D, false, "X";
    KeyC = 46, 0x2E, false, "C";
    KeyV = 47, 0x2F, false, "V";
    KeyB = 48, 0x30, false, "B";
    KeyN = 49, 0x31, false, "N";
    KeyM = 50, 0x32, false, "M";
    Comma = 51, 0x33, false, ",";
    Period = 52, 0x34, false, ".";
    Slash = 53, 0x35, false, "/";
    ShiftRight = 54, 0x36, false, "RShift";
    NumpadMultiply = 55, 0x37, false, "NumMultiply";
    AltLeft = 56, 0x38, false, "LAlt";
    Space = 57, 0x39, false, "Space";
    CapsLock = 58, 0x3A, false, "CapsLock";
    F1 = 59, 0x3B, false, "F1";
    F2 = 60, 0x3C, false, "F2";
    F3 = 61, 0x3D, false, "F3";
    F4 = 62, 0x3E, false, "F4";
    F5 = 63, 0x3F, false, "F5";
    F6 = 64, 0x40, false, "F6";
    F7 = 65, 0x41, false, "F7";
    F8 = 66, 0x42, false, "F8";
    F9 = 67, 0x43, false, "F9";
    F10 = 68, 0x44, false, "F10";
    NumLock = 69, 0x45, true, "NumLock";
    ScrollLock = 70, 0x46, false, "ScrollLock";
    Numpad7 = 71, 0x47, false, "Num7";
    Numpad8 = 72, 0x48, false, "Num8";
    Numpad9 = 73, 0x49, false, "Num9";
    NumpadSubtract = 74, 0x4A, false, "NumMinus";
    Numpad4 = 75, 0x4B, false, "Num4";
    Numpad5 = 76, 0x4C, false, "Num5";
    Numpad6 = 77, 0x4D, false, "Num6";
    NumpadAdd = 78, 0x4E, false, "NumPlus";
    Numpad1 = 79, 0x4F, false, "Num1";
    Numpad2 = 80, 0x50, false, "Num2";
    Numpad3 = 81, 0x51, false, "Num3";
    Numpad0 = 82, 0x52, false, "Num0";
    NumpadDecimal = 83, 0x53, false, "NumDecimal";
    IntlBackslash = 86, 0x56, false, "IntlBackslash";
    F11 = 87, 0x57, false, "F11";
    F12 = 88, 0x58, false, "F12";
    NumpadEnter = 96, 0x1C, true, "NumEnter";
    ControlRight = 97, 0x1D, true, "RCtrl";
    NumpadDivide = 98, 0x35, true, "NumDivide";
    PrintScreen = 99, 0x37, true, "PrintScreen";
    AltRight = 100, 0x38, true, "RAlt";
    Home = 102, 0x47, true, "Home";
    ArrowUp = 103, 0x48, true, "Up";
    PageUp = 104, 0x49, true, "PageUp";
    ArrowLeft = 105, 0x4B, true, "Left";
    ArrowRight = 106, 0x4D, true, "Right";
    End = 107, 0x4F, true, "End";
    ArrowDown = 108, 0x50, true, "Down";
    PageDown = 109, 0x51, true, "PageDown";
    Insert = 110, 0x52, true, "Insert";
    Delete = 111, 0x53, true, "Delete";
    NumpadEqual = 117, 0x59, false, "NumEqual";
    Pause = 119, 0x45, false, "Break";
    NumpadComma = 121, 0x7E, false, "NumComma";
    MetaLeft = 125, 0x5B, true, "LWin";
    MetaRight = 126, 0x5C, true, "RWin";
    ContextMenu = 127, 0x5D, true, "Menu";
    F13 = 183, 0x64, false, "F13";
    F14 = 184, 0x65, false, "F14";
    F15 = 185, 0x66, false, "F15";
    F16 = 186, 0x67, false, "F16";
    F17 = 187, 0x68, false, "F17";
    F18 = 188, 0x69, false, "F18";
    F19 = 189, 0x6A, false, "F19";
    F20 = 190, 0x6B, false, "F20";
    F21 = 191, 0x6C, false, "F21";
    F22 = 192, 0x6D, false, "F22";
    F23 = 193, 0x6E, false, "F23";
    F24 = 194, 0x76, false, "F24";
}

impl PhysKey {
    /// Dense index in `0..PhysKey::COUNT`, suitable for lookup tables.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Ctrl, Shift, Alt or Win/Super on either side.
    pub const fn is_modifier(self) -> bool {
        matches!(
            self,
            PhysKey::ControlLeft
                | PhysKey::ControlRight
                | PhysKey::ShiftLeft
                | PhysKey::ShiftRight
                | PhysKey::AltLeft
                | PhysKey::AltRight
                | PhysKey::MetaLeft
                | PhysKey::MetaRight
        )
    }

    /// Keys of the main block whose character depends on the layout
    /// (letters, digits and punctuation, but not `Space`).
    pub const fn is_text_key(self) -> bool {
        matches!(
            self,
            PhysKey::Backquote
                | PhysKey::Digit1
                | PhysKey::Digit2
                | PhysKey::Digit3
                | PhysKey::Digit4
                | PhysKey::Digit5
                | PhysKey::Digit6
                | PhysKey::Digit7
                | PhysKey::Digit8
                | PhysKey::Digit9
                | PhysKey::Digit0
                | PhysKey::Minus
                | PhysKey::Equal
                | PhysKey::KeyQ
                | PhysKey::KeyW
                | PhysKey::KeyE
                | PhysKey::KeyR
                | PhysKey::KeyT
                | PhysKey::KeyY
                | PhysKey::KeyU
                | PhysKey::KeyI
                | PhysKey::KeyO
                | PhysKey::KeyP
                | PhysKey::BracketLeft
                | PhysKey::BracketRight
                | PhysKey::Backslash
                | PhysKey::KeyA
                | PhysKey::KeyS
                | PhysKey::KeyD
                | PhysKey::KeyF
                | PhysKey::KeyG
                | PhysKey::KeyH
                | PhysKey::KeyJ
                | PhysKey::KeyK
                | PhysKey::KeyL
                | PhysKey::Semicolon
                | PhysKey::Quote
                | PhysKey::KeyZ
                | PhysKey::KeyX
                | PhysKey::KeyC
                | PhysKey::KeyV
                | PhysKey::KeyB
                | PhysKey::KeyN
                | PhysKey::KeyM
                | PhysKey::Comma
                | PhysKey::Period
                | PhysKey::Slash
                | PhysKey::IntlBackslash
        )
    }

    /// Arrow keys and `Home`/`End`/`PageUp`/`PageDown`.
    pub const fn is_navigation(self) -> bool {
        matches!(
            self,
            PhysKey::ArrowUp
                | PhysKey::ArrowDown
                | PhysKey::ArrowLeft
                | PhysKey::ArrowRight
                | PhysKey::Home
                | PhysKey::End
                | PhysKey::PageUp
                | PhysKey::PageDown
        )
    }

    /// Keys of the numeric keypad.
    pub const fn is_numpad(self) -> bool {
        matches!(
            self,
            PhysKey::Numpad0
                | PhysKey::Numpad1
                | PhysKey::Numpad2
                | PhysKey::Numpad3
                | PhysKey::Numpad4
                | PhysKey::Numpad5
                | PhysKey::Numpad6
                | PhysKey::Numpad7
                | PhysKey::Numpad8
                | PhysKey::Numpad9
                | PhysKey::NumpadAdd
                | PhysKey::NumpadSubtract
                | PhysKey::NumpadMultiply
                | PhysKey::NumpadDivide
                | PhysKey::NumpadDecimal
                | PhysKey::NumpadEnter
                | PhysKey::NumpadEqual
                | PhysKey::NumpadComma
        )
    }

    /// Parses a key name used in hotkey strings. Case-insensitive; accepts the
    /// canonical [`hotkey_name`](Self::hotkey_name), the [`code_name`](Self::code_name)
    /// and common aliases (`Pause`, `Escape`, `Del`, `PgUp`, `ArrowUp`, ...).
    ///
    /// Generic modifier names (`Ctrl`, `Shift`, `Alt`, `Win`) are not keys and
    /// return `None`; use the sided variants (`LCtrl`, `RShift`, ...).
    pub fn from_name(name: &str) -> Option<PhysKey> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if let Some(key) = PhysKey::ALL.iter().copied().find(|k| {
            k.hotkey_name().eq_ignore_ascii_case(name) || k.code_name().eq_ignore_ascii_case(name)
        }) {
            return Some(key);
        }
        let alias = match name.to_ascii_lowercase().as_str() {
            "pause" | "pausebreak" | "pause/break" => PhysKey::Pause,
            "escape" => PhysKey::Escape,
            "return" => PhysKey::Enter,
            "del" => PhysKey::Delete,
            "ins" => PhysKey::Insert,
            "pgup" => PhysKey::PageUp,
            "pgdn" | "pgdown" => PhysKey::PageDown,
            "prtsc" | "prtscr" | "print" | "sysrq" => PhysKey::PrintScreen,
            "scroll" | "scrlk" => PhysKey::ScrollLock,
            "caps" => PhysKey::CapsLock,
            "apps" | "application" | "contextmenu" => PhysKey::ContextMenu,
            "bksp" | "back" => PhysKey::Backspace,
            "spacebar" => PhysKey::Space,
            "lcontrol" | "leftctrl" | "leftcontrol" => PhysKey::ControlLeft,
            "rcontrol" | "rightctrl" | "rightcontrol" => PhysKey::ControlRight,
            "leftshift" => PhysKey::ShiftLeft,
            "rightshift" => PhysKey::ShiftRight,
            "leftalt" => PhysKey::AltLeft,
            "rightalt" | "altgr" => PhysKey::AltRight,
            "lsuper" | "lmeta" | "leftwin" | "leftsuper" => PhysKey::MetaLeft,
            "rsuper" | "rmeta" | "rightwin" | "rightsuper" => PhysKey::MetaRight,
            "minus" => PhysKey::Minus,
            "equal" | "equals" => PhysKey::Equal,
            "comma" => PhysKey::Comma,
            "period" | "dot" => PhysKey::Period,
            "slash" => PhysKey::Slash,
            "backslash" => PhysKey::Backslash,
            "semicolon" => PhysKey::Semicolon,
            "quote" | "apostrophe" => PhysKey::Quote,
            "grave" | "backquote" | "tilde" => PhysKey::Backquote,
            "bracketleft" | "lbracket" => PhysKey::BracketLeft,
            "bracketright" | "rbracket" => PhysKey::BracketRight,
            _ => return None,
        };
        Some(alias)
    }
}

impl std::fmt::Display for PhysKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.hotkey_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn index_is_dense() {
        for (i, key) in PhysKey::ALL.iter().enumerate() {
            assert_eq!(key.index(), i);
        }
    }

    #[test]
    fn evdev_codes_are_unique_and_roundtrip() {
        let mut seen = HashSet::new();
        for &key in PhysKey::ALL {
            assert!(
                seen.insert(key.evdev_code()),
                "duplicate evdev code for {key:?}"
            );
            assert_eq!(PhysKey::from_evdev(key.evdev_code()), Some(key));
        }
        assert_eq!(PhysKey::from_evdev(0), None);
        assert_eq!(PhysKey::from_evdev(113), None); // KEY_MUTE is not tracked
    }

    #[test]
    fn win_scancodes_are_unique_and_roundtrip() {
        let mut seen = HashSet::new();
        for &key in PhysKey::ALL {
            let (sc, ext) = key.win_scancode();
            assert!(seen.insert((sc, ext)), "duplicate scan code for {key:?}");
            assert_eq!(PhysKey::from_win_scancode(sc, ext), Some(key));
        }
    }

    #[test]
    fn win_special_scancodes() {
        assert_eq!(
            PhysKey::from_win_scancode(0x45, false),
            Some(PhysKey::Pause)
        );
        assert_eq!(
            PhysKey::from_win_scancode(0x45, true),
            Some(PhysKey::NumLock)
        );
        assert_eq!(PhysKey::from_win_scancode(0x46, true), Some(PhysKey::Pause));
        assert_eq!(
            PhysKey::from_win_scancode(0x46, false),
            Some(PhysKey::ScrollLock)
        );
        assert_eq!(
            PhysKey::from_win_scancode(0x54, false),
            Some(PhysKey::PrintScreen)
        );
        assert_eq!(
            PhysKey::from_win_scancode(0x1C, true),
            Some(PhysKey::NumpadEnter)
        );
        assert_eq!(
            PhysKey::from_win_scancode(0x36, true),
            Some(PhysKey::ShiftRight)
        );
        assert_eq!(PhysKey::from_win_scancode(0x2A, true), None);
    }

    #[test]
    fn hotkey_names_are_unique_and_parse_back() {
        let mut seen = HashSet::new();
        for &key in PhysKey::ALL {
            let name = key.hotkey_name().to_ascii_lowercase();
            assert!(seen.insert(name), "duplicate hotkey name for {key:?}");
            assert_eq!(PhysKey::from_name(key.hotkey_name()), Some(key));
            assert_eq!(PhysKey::from_name(key.code_name()), Some(key));
        }
    }

    #[test]
    fn aliases() {
        assert_eq!(PhysKey::from_name("pause"), Some(PhysKey::Pause));
        assert_eq!(PhysKey::from_name("BREAK"), Some(PhysKey::Pause));
        assert_eq!(PhysKey::from_name("scroll lock"), None);
        assert_eq!(PhysKey::from_name("ScrollLock"), Some(PhysKey::ScrollLock));
        assert_eq!(PhysKey::from_name("AltGr"), Some(PhysKey::AltRight));
        assert_eq!(PhysKey::from_name(" pgdn "), Some(PhysKey::PageDown));
        assert_eq!(PhysKey::from_name("Ctrl"), None);
        assert_eq!(PhysKey::from_name(""), None);
    }

    #[test]
    fn classification() {
        assert!(PhysKey::ShiftRight.is_modifier());
        assert!(!PhysKey::CapsLock.is_modifier());
        assert!(PhysKey::KeyQ.is_text_key());
        assert!(PhysKey::Backquote.is_text_key());
        assert!(!PhysKey::Space.is_text_key());
        assert!(PhysKey::End.is_navigation());
        assert!(PhysKey::NumpadEnter.is_numpad());
        let text_keys = PhysKey::ALL.iter().filter(|k| k.is_text_key()).count();
        assert_eq!(text_keys, 48);
    }
}
