//! Case corrections and case inversion.

use crate::detect::{CasePattern, case_pattern};

/// Words that legitimately start with two capitals (units, city abbreviations).
const TWO_CAPS_EXCEPTIONS: &[&str] = &[
    "СПб", "МГц", "ГГц", "ТГц", "кГц", "МВт", "ГВт", "ТВт", "МПа", "ГПа", "КПа", "МБит", "ГБит",
    "МГУ", "MHz", "GHz", "THz", "MPa", "GPa", "KPa", "MBit", "GBit",
];

fn capitalize_lower(word: &str) -> String {
    let lower = word.to_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map(|f| f.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// «Исправлять две заглавные буквы в начале слова»: `ПРивет` → `Привет`.
///
/// Returns `None` when the word does not need a fix, including exceptions
/// such as `СПб`, `МГц` and English plurals of abbreviations (`IDs`, `PCs`).
pub fn fix_two_initial_caps(word: &str) -> Option<String> {
    if case_pattern(word) != CasePattern::TwoInitialCaps {
        return None;
    }
    if TWO_CAPS_EXCEPTIONS.contains(&word) {
        return None;
    }
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() == 3 && letters[0].is_ascii() && letters[2] == 's' {
        return None;
    }
    Some(capitalize_lower(word))
}

/// «Исправлять случайное нажатие Caps Lock»: `пРИВЕТ` → `Привет`.
///
/// The caller must check that Caps Lock was on while the word was typed.
pub fn fix_inverted_caps(word: &str) -> Option<String> {
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    (letters >= 2 && case_pattern(word) == CasePattern::InvertedCapitalized)
        .then(|| capitalize_lower(word))
}

/// «Сменить регистр выделенного текста»: swaps the case of every letter,
/// `пРИВЕТ` ↔ `Привет`.
pub fn invert_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_uppercase() {
            out.extend(c.to_lowercase());
        } else if c.is_lowercase() {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_initial_caps() {
        assert_eq!(fix_two_initial_caps("ПРивет").as_deref(), Some("Привет"));
        assert_eq!(fix_two_initial_caps("HEllo").as_deref(), Some("Hello"));
        assert_eq!(fix_two_initial_caps("СПб"), None);
        assert_eq!(fix_two_initial_caps("МГц"), None);
        assert_eq!(fix_two_initial_caps("IDs"), None);
        assert_eq!(fix_two_initial_caps("Привет"), None);
        assert_eq!(fix_two_initial_caps("ПРИВЕТ"), None);
        assert_eq!(fix_two_initial_caps("ПР"), None);
    }

    #[test]
    fn inverted_caps() {
        assert_eq!(fix_inverted_caps("пРИВЕТ").as_deref(), Some("Привет"));
        assert_eq!(fix_inverted_caps("hELLO").as_deref(), Some("Hello"));
        assert_eq!(fix_inverted_caps("пР").as_deref(), Some("Пр"));
        assert_eq!(fix_inverted_caps("п"), None);
        assert_eq!(fix_inverted_caps("Привет"), None);
    }

    #[test]
    fn inversion() {
        assert_eq!(invert_case("пРИВЕТ"), "Привет");
        assert_eq!(invert_case("Привет, World 42!"), "пРИВЕТ, wORLD 42!");
        assert_eq!(invert_case(&invert_case("ÄbC ёЁ")), "ÄbC ёЁ");
    }
}
