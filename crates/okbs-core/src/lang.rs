//! Languages supported by the layout detector.

use serde::{Deserialize, Serialize};

/// A language whose keyboard layout the program can detect and switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    /// Russian (ЙЦУКЕН).
    Ru,
    /// English (QWERTY US).
    En,
}

impl Lang {
    /// All supported languages.
    pub const ALL: [Lang; 2] = [Lang::Ru, Lang::En];

    /// Two-letter ISO 639-1 code.
    pub const fn code(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    /// The other language of the RU/EN pair.
    pub const fn other(self) -> Lang {
        match self {
            Lang::Ru => Lang::En,
            Lang::En => Lang::Ru,
        }
    }

    /// Parses a BCP 47 tag or XKB layout name: `ru`, `ru-RU`, `en_US`, `us`, `gb`.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let primary = tag
            .trim()
            .split(['-', '_', '(', ':', '+'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match primary.as_str() {
            "ru" | "rus" | "russian" => Some(Lang::Ru),
            "en" | "eng" | "english" | "us" | "gb" | "uk" => Some(Lang::En),
            _ => None,
        }
    }

    /// Maps a Windows `LANGID` (low word of an `HKL`) to a language.
    pub const fn from_windows_langid(langid: u16) -> Option<Lang> {
        match langid & 0x03FF {
            0x19 => Some(Lang::Ru),
            0x09 => Some(Lang::En),
            _ => None,
        }
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        assert_eq!(Lang::from_tag("ru"), Some(Lang::Ru));
        assert_eq!(Lang::from_tag("ru-RU"), Some(Lang::Ru));
        assert_eq!(Lang::from_tag("en_US"), Some(Lang::En));
        assert_eq!(Lang::from_tag("us(intl)"), Some(Lang::En));
        assert_eq!(Lang::from_tag("ua"), None);
        assert_eq!(Lang::from_tag(""), None);
    }

    #[test]
    fn windows_langids() {
        assert_eq!(Lang::from_windows_langid(0x0419), Some(Lang::Ru));
        assert_eq!(Lang::from_windows_langid(0x0409), Some(Lang::En));
        assert_eq!(Lang::from_windows_langid(0x0809), Some(Lang::En));
        assert_eq!(Lang::from_windows_langid(0x0422), None);
    }

    #[test]
    fn other() {
        assert_eq!(Lang::Ru.other(), Lang::En);
        assert_eq!(Lang::En.other().other(), Lang::En);
    }
}
