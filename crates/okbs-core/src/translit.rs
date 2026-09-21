//! Transliteration between Cyrillic and Latin («Транслитерировать», Alt+Scroll Lock).
//!
//! The built-in tables follow the familiar passport-like scheme
//! (`щ` → `shch`, `ж` → `zh`, `я` → `ya`), so that `Привет` ↔ `Privet`.
//! Tables are longest-match, so users can supply their own in the same format.

use crate::lang::Lang;
use crate::layouts::guess_text_lang;

/// A longest-match substitution table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transliterator {
    /// `(from, to)` pairs, lowercase, sorted by descending `from` length.
    pairs: Vec<(Vec<char>, String)>,
    max_len: usize,
    /// Latin `y` after a vowel becomes `й` instead of `ы`.
    latin_y_rule: bool,
}

const RU_TO_LATIN: &[(&str, &str)] = &[
    ("а", "a"),
    ("б", "b"),
    ("в", "v"),
    ("г", "g"),
    ("д", "d"),
    ("е", "e"),
    ("ё", "yo"),
    ("ж", "zh"),
    ("з", "z"),
    ("и", "i"),
    ("й", "y"),
    ("к", "k"),
    ("л", "l"),
    ("м", "m"),
    ("н", "n"),
    ("о", "o"),
    ("п", "p"),
    ("р", "r"),
    ("с", "s"),
    ("т", "t"),
    ("у", "u"),
    ("ф", "f"),
    ("х", "kh"),
    ("ц", "ts"),
    ("ч", "ch"),
    ("ш", "sh"),
    ("щ", "shch"),
    ("ъ", ""),
    ("ы", "y"),
    ("ь", ""),
    ("э", "e"),
    ("ю", "yu"),
    ("я", "ya"),
];

const LATIN_TO_RU: &[(&str, &str)] = &[
    ("shch", "щ"),
    ("sch", "щ"),
    ("zh", "ж"),
    ("kh", "х"),
    ("ts", "ц"),
    ("ch", "ч"),
    ("sh", "ш"),
    ("yu", "ю"),
    ("ya", "я"),
    ("yo", "ё"),
    ("ju", "ю"),
    ("ja", "я"),
    ("jo", "ё"),
    ("a", "а"),
    ("b", "б"),
    ("c", "ц"),
    ("d", "д"),
    ("e", "е"),
    ("f", "ф"),
    ("g", "г"),
    ("h", "х"),
    ("i", "и"),
    ("j", "й"),
    ("k", "к"),
    ("l", "л"),
    ("m", "м"),
    ("n", "н"),
    ("o", "о"),
    ("p", "п"),
    ("q", "к"),
    ("r", "р"),
    ("s", "с"),
    ("t", "т"),
    ("u", "у"),
    ("v", "в"),
    ("w", "в"),
    ("x", "кс"),
    ("y", "ы"),
    ("z", "з"),
];

impl Transliterator {
    /// Builds a table from `(from, to)` pairs. Matching is case-insensitive;
    /// the case of the source is carried over to the result.
    pub fn new<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut pairs: Vec<(Vec<char>, String)> = pairs
            .into_iter()
            .filter(|(from, _)| !from.is_empty())
            .map(|(from, to)| (from.to_lowercase().chars().collect(), to.to_lowercase()))
            .collect();
        pairs.sort_by_key(|p| std::cmp::Reverse(p.0.len()));
        let max_len = pairs.first().map_or(0, |p| p.0.len());
        Self {
            pairs,
            max_len,
            latin_y_rule: false,
        }
    }

    /// Cyrillic → Latin.
    pub fn cyrillic_to_latin() -> Self {
        Self::new(RU_TO_LATIN.iter().copied())
    }

    /// Latin → Cyrillic.
    pub fn latin_to_cyrillic() -> Self {
        Self {
            latin_y_rule: true,
            ..Self::new(LATIN_TO_RU.iter().copied())
        }
    }

    /// Parses a user table: one `from=to` pair per line, `#` starts a comment.
    pub fn parse_table(text: &str) -> Self {
        let pairs: Vec<(&str, &str)> = text
            .lines()
            .map(|l| l.split('#').next().unwrap_or_default().trim())
            .filter_map(|l| l.split_once('='))
            .map(|(a, b)| (a.trim(), b.trim()))
            .collect();
        Self::new(pairs)
    }

    /// Applies the table.
    pub fn apply(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let lower: Vec<char> = chars.iter().map(|&c| crate::lm::to_lower(c)).collect();
        let mut out = String::with_capacity(text.len() * 2);
        let mut i = 0;
        while i < chars.len() {
            let found = self.pairs.iter().find(|(from, _)| {
                from.len() <= self.max_len
                    && lower.len() - i >= from.len()
                    && lower[i..i + from.len()] == from[..]
            });
            let Some((from, to)) = found else {
                out.push(chars[i]);
                i += 1;
                continue;
            };
            let mut to = to.as_str();
            if self.latin_y_rule
                && from[..] == ['y']
                && i > 0
                && "aeiouаеиоуыэюя".contains(lower[i - 1])
            {
                to = "й";
            }
            let source = &chars[i..i + from.len()];
            let next_upper = chars.get(i + from.len()).is_some_and(|c| c.is_uppercase());
            let prev_upper = i > 0 && chars[i - 1].is_uppercase();
            if source[0].is_uppercase() {
                let all_caps = source.iter().skip(1).any(|c| c.is_uppercase())
                    || next_upper
                    || (prev_upper && !chars.get(i + from.len()).is_some_and(|c| c.is_lowercase()));
                if all_caps {
                    out.push_str(&to.to_uppercase());
                } else {
                    let mut t = to.chars();
                    if let Some(first) = t.next() {
                        out.extend(first.to_uppercase());
                        out.extend(t);
                    }
                }
            } else {
                out.push_str(to);
            }
            i += from.len();
        }
        out
    }
}

/// Transliterates in the direction given by the script of `text`:
/// Cyrillic becomes Latin, Latin becomes Cyrillic.
pub fn transliterate(text: &str) -> String {
    match guess_text_lang(text) {
        Some(Lang::Ru) => Transliterator::cyrillic_to_latin().apply(text),
        _ => Transliterator::latin_to_cyrillic().apply(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transliteration_example() {
        assert_eq!(transliterate("Привет"), "Privet");
        assert_eq!(transliterate("Privet"), "Привет");
    }

    #[test]
    fn multi_letter_and_case() {
        let t = Transliterator::cyrillic_to_latin();
        assert_eq!(
            t.apply("Щука жила в Ярославле"),
            "Shchuka zhila v Yaroslavle"
        );
        assert_eq!(t.apply("ЩУКА"), "SHCHUKA");
        assert_eq!(t.apply("объявление, 2026!"), "obyavlenie, 2026!");
        let r = Transliterator::latin_to_cyrillic();
        assert_eq!(
            r.apply("Shchuka zhila v Yaroslavle"),
            "Щука жила в Ярославле"
        );
        assert_eq!(r.apply("SHCHUKA"), "ЩУКА");
        assert_eq!(r.apply("Andrey, my"), "Андрей, мы");
    }

    #[test]
    fn user_table() {
        let t = Transliterator::parse_table("# comment\nх = h\nщ=sch # German-like\n\nbad line\n");
        assert_eq!(t.apply("Хорош, щи"), "Hорош, schи");
    }
}
