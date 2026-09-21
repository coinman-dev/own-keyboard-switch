//! Built-in RU and EN keyboard layouts, rendering of typed keys and
//! conversion of text between layouts.

use crate::keymap::KeyMap;
use crate::keys::PhysKey;
use crate::lang::Lang;
use std::sync::OnceLock;

/// One typed key with the modifier state that affects the produced character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPress {
    /// Physical key.
    pub key: PhysKey,
    /// Shift was held.
    pub shift: bool,
    /// Caps Lock was on.
    pub caps: bool,
}

impl KeyPress {
    /// A key without Shift and Caps Lock.
    pub const fn plain(key: PhysKey) -> Self {
        Self {
            key,
            shift: false,
            caps: false,
        }
    }

    /// A key with Shift.
    pub const fn shifted(key: PhysKey) -> Self {
        Self {
            key,
            shift: true,
            caps: false,
        }
    }
}

/// Keys of the main block in the order of the layout strings below:
/// the digit row from Backquote to Equal, then the three letter rows.
const MAIN_KEYS: [PhysKey; 47] = [
    PhysKey::Backquote,
    PhysKey::Digit1,
    PhysKey::Digit2,
    PhysKey::Digit3,
    PhysKey::Digit4,
    PhysKey::Digit5,
    PhysKey::Digit6,
    PhysKey::Digit7,
    PhysKey::Digit8,
    PhysKey::Digit9,
    PhysKey::Digit0,
    PhysKey::Minus,
    PhysKey::Equal,
    PhysKey::KeyQ,
    PhysKey::KeyW,
    PhysKey::KeyE,
    PhysKey::KeyR,
    PhysKey::KeyT,
    PhysKey::KeyY,
    PhysKey::KeyU,
    PhysKey::KeyI,
    PhysKey::KeyO,
    PhysKey::KeyP,
    PhysKey::BracketLeft,
    PhysKey::BracketRight,
    PhysKey::Backslash,
    PhysKey::KeyA,
    PhysKey::KeyS,
    PhysKey::KeyD,
    PhysKey::KeyF,
    PhysKey::KeyG,
    PhysKey::KeyH,
    PhysKey::KeyJ,
    PhysKey::KeyK,
    PhysKey::KeyL,
    PhysKey::Semicolon,
    PhysKey::Quote,
    PhysKey::KeyZ,
    PhysKey::KeyX,
    PhysKey::KeyC,
    PhysKey::KeyV,
    PhysKey::KeyB,
    PhysKey::KeyN,
    PhysKey::KeyM,
    PhysKey::Comma,
    PhysKey::Period,
    PhysKey::Slash,
];

const EN_NORMAL: &str = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./";
const EN_SHIFTED: &str = "~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?";
const RU_NORMAL: &str = "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю.";
const RU_SHIFTED: &str = "Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,";

fn build(normal: &str, shifted: &str, intl: (char, char)) -> KeyMap {
    let mut map = KeyMap::new();
    for ((key, n), s) in MAIN_KEYS.iter().zip(normal.chars()).zip(shifted.chars()) {
        map.set(*key, Some(n), Some(s));
    }
    map.set(PhysKey::IntlBackslash, Some(intl.0), Some(intl.1));
    map.set(PhysKey::Space, Some(' '), Some(' '));
    let numpad = [
        (PhysKey::Numpad0, '0'),
        (PhysKey::Numpad1, '1'),
        (PhysKey::Numpad2, '2'),
        (PhysKey::Numpad3, '3'),
        (PhysKey::Numpad4, '4'),
        (PhysKey::Numpad5, '5'),
        (PhysKey::Numpad6, '6'),
        (PhysKey::Numpad7, '7'),
        (PhysKey::Numpad8, '8'),
        (PhysKey::Numpad9, '9'),
        (PhysKey::NumpadDivide, '/'),
        (PhysKey::NumpadMultiply, '*'),
        (PhysKey::NumpadSubtract, '-'),
        (PhysKey::NumpadAdd, '+'),
    ];
    for (key, c) in numpad {
        map.set(key, Some(c), Some(c));
    }
    map
}

/// Built-in key map of `lang`: US QWERTY for English, ЙЦУКЕН for Russian.
pub fn builtin_keymap(lang: Lang) -> &'static KeyMap {
    static RU: OnceLock<KeyMap> = OnceLock::new();
    static EN: OnceLock<KeyMap> = OnceLock::new();
    match lang {
        Lang::Ru => RU.get_or_init(|| build(RU_NORMAL, RU_SHIFTED, ('\\', '/'))),
        Lang::En => EN.get_or_init(|| build(EN_NORMAL, EN_SHIFTED, ('\\', '|'))),
    }
}

/// Character produced by `press` in `map`. Caps Lock inverts Shift only for letters.
pub fn char_for(map: &KeyMap, press: KeyPress) -> Option<char> {
    let normal = map.get(press.key, false);
    let letter = normal.is_some_and(char::is_alphabetic);
    let shift = if letter {
        press.shift != press.caps
    } else {
        press.shift
    };
    map.get(press.key, shift)
}

/// Text produced by typing `keys` in `map`; keys without a character are skipped.
pub fn render(keys: &[KeyPress], map: &KeyMap) -> String {
    keys.iter().filter_map(|&k| char_for(map, k)).collect()
}

/// Key presses that type `text` in `map` (without Caps Lock), or `None` if a
/// character cannot be typed.
pub fn keys_for_text(text: &str, map: &KeyMap) -> Option<Vec<KeyPress>> {
    text.chars()
        .map(|c| {
            map.find(c).map(|(key, shift)| KeyPress {
                key,
                shift,
                caps: false,
            })
        })
        .collect()
}

/// Whether `press` produces a letter in either of the two layouts.
pub fn is_word_key(press: KeyPress) -> bool {
    Lang::ALL.iter().any(|&lang| {
        char_for(builtin_keymap(lang), press)
            .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '\'')
    })
}

/// Retypes `text` from layout `from` to layout `to` character by character:
/// `ghbdtn` → `привет`. Characters absent from `from` are kept.
pub fn convert_text(text: &str, from: &KeyMap, to: &KeyMap) -> String {
    text.chars()
        .map(|c| {
            if c == ' ' || c.is_ascii_digit() {
                return c;
            }
            from.find(c)
                .and_then(|(key, shift)| to.get(key, shift))
                .unwrap_or(c)
        })
        .collect()
}

/// Guesses the layout `text` was typed in from its letters (Cyrillic vs Latin).
pub fn guess_text_lang(text: &str) -> Option<Lang> {
    let (mut ru, mut en) = (0usize, 0usize);
    for c in text.chars() {
        let lower = crate::lm::to_lower(c);
        if crate::lm::letter_symbol(Lang::Ru, lower).is_some() {
            ru += 1;
        } else if crate::lm::letter_symbol(Lang::En, lower).is_some() {
            en += 1;
        }
    }
    match ru.cmp(&en) {
        std::cmp::Ordering::Greater => Some(Lang::Ru),
        std::cmp::Ordering::Less => Some(Lang::En),
        std::cmp::Ordering::Equal => None,
    }
}

/// Converts text to the other layout of the RU/EN pair, guessing its current layout.
pub fn switch_text_layout(text: &str) -> String {
    let from = guess_text_lang(text).unwrap_or(Lang::En);
    convert_text(text, builtin_keymap(from), builtin_keymap(from.other()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ru() -> &'static KeyMap {
        builtin_keymap(Lang::Ru)
    }

    fn en() -> &'static KeyMap {
        builtin_keymap(Lang::En)
    }

    #[test]
    fn layout_strings_are_complete() {
        for s in [EN_NORMAL, EN_SHIFTED, RU_NORMAL, RU_SHIFTED] {
            assert_eq!(s.chars().count(), MAIN_KEYS.len(), "{s}");
        }
        assert_eq!(en().get(PhysKey::KeyQ, false), Some('q'));
        assert_eq!(ru().get(PhysKey::KeyQ, true), Some('Й'));
        assert_eq!(ru().get(PhysKey::Slash, false), Some('.'));
        assert_eq!(ru().get(PhysKey::Slash, true), Some(','));
        assert_eq!(ru().get(PhysKey::Digit3, true), Some('№'));
        assert_eq!(ru().get(PhysKey::Backquote, false), Some('ё'));
    }

    #[test]
    fn every_russian_letter_is_typeable() {
        for c in "абвгдеёжзийклмнопрстуфхцчшщъыьэюяАБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ".chars()
        {
            assert!(ru().find(c).is_some(), "{c}");
        }
        for c in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".chars() {
            assert!(en().find(c).is_some(), "{c}");
        }
    }

    #[test]
    fn render_with_shift_and_caps() {
        let keys = keys_for_text("Привет", ru()).unwrap();
        assert_eq!(render(&keys, en()), "Ghbdtn");
        assert_eq!(render(&keys, ru()), "Привет");

        let caps: Vec<KeyPress> = keys.iter().map(|k| KeyPress { caps: true, ..*k }).collect();
        assert_eq!(render(&caps, ru()), "пРИВЕТ");
        assert_eq!(render(&caps, en()), "gHBDTN");

        let comma_caps = [KeyPress {
            key: PhysKey::Comma,
            shift: false,
            caps: true,
        }];
        assert_eq!(render(&comma_caps, ru()), "Б");
        assert_eq!(render(&comma_caps, en()), ",");
    }

    #[test]
    fn conversion_roundtrip() {
        assert_eq!(convert_text("ghbdtn? vbh!", en(), ru()), "привет, мир!");
        assert_eq!(convert_text("ghbdtn,", en(), ru()), "приветб");
        assert_eq!(convert_text("руддщ цщкдв", ru(), en()), "hello world");
        assert_eq!(convert_text("[jhjij", en(), ru()), "хорошо");
        assert_eq!(convert_text("j,]zdktybt", en(), ru()), "объявление");
        assert_eq!(switch_text_layout("Ghbdtn"), "Привет");
        assert_eq!(switch_text_layout("Руддщ"), "Hello");
        assert_eq!(guess_text_lang("123"), None);
    }

    #[test]
    fn word_keys() {
        assert!(is_word_key(KeyPress::plain(PhysKey::KeyA)));
        assert!(is_word_key(KeyPress::plain(PhysKey::Comma)));
        assert!(is_word_key(KeyPress::plain(PhysKey::Minus)));
        assert!(!is_word_key(KeyPress::plain(PhysKey::Space)));
        assert!(!is_word_key(KeyPress::plain(PhysKey::Slash)));
        assert!(!is_word_key(KeyPress::shifted(PhysKey::Slash)));
        assert!(!is_word_key(KeyPress::shifted(PhysKey::Digit1)));
        assert!(is_word_key(KeyPress::shifted(PhysKey::Semicolon)));
    }
}
