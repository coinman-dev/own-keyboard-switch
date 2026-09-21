//! Spell checking of text with Hunspell dictionaries (clipboard «Проверить орфографию»).

use crate::lang::Lang;
use crate::lm;

/// A word not found in the dictionary of its language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misspelling {
    /// Byte range of the word in the checked text.
    pub range: std::ops::Range<usize>,
    /// The word as written.
    pub word: String,
    /// Language detected from the word's letters.
    pub lang: Lang,
    /// Corrections, best first.
    pub suggestions: Vec<String>,
}

/// Words of `text` with their byte ranges: letter runs with inner hyphens and
/// apostrophes. Tokens glued to digits or underscores are skipped.
pub fn words(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut letters_end = 0;
    let mut glued = false;
    let mut finish = |start: &mut Option<usize>, glued: &mut bool, letters_end: usize| {
        if let Some(s) = start.take()
            && !*glued
            && letters_end > s
        {
            out.push((s..letters_end, &text[s..letters_end]));
        }
        *glued = false;
    };
    for (i, c) in text.char_indices() {
        if c.is_alphabetic() {
            start.get_or_insert(i);
            letters_end = i + c.len_utf8();
        } else if c.is_ascii_digit() || c == '_' {
            glued = true;
            start.get_or_insert(i);
        } else if !(lm::is_joiner(c) && start.is_some()) {
            finish(&mut start, &mut glued, letters_end);
        }
    }
    finish(&mut start, &mut glued, letters_end);
    out
}

/// Language of a word by its script, if all letters belong to one supported language.
pub fn word_lang(word: &str) -> Option<Lang> {
    Lang::ALL.into_iter().find(|&lang| {
        word.chars()
            .all(|c| lm::is_joiner(c) || lm::is_letter(lang, c))
    })
}

/// Checks `text` with `dictionary(lang)` for every language in `langs`.
///
/// Words of other scripts, words glued to digits and single letters are
/// skipped. At most `max_suggestions` corrections are returned per word.
pub fn check_text<'d>(
    text: &str,
    langs: &[Lang],
    max_suggestions: usize,
    dictionary: impl Fn(Lang) -> &'d spellbook::Dictionary,
) -> Vec<Misspelling> {
    let mut out = Vec::new();
    for (range, word) in words(text) {
        if word.chars().filter(|c| c.is_alphabetic()).count() < 2 {
            continue;
        }
        let Some(lang) = word_lang(word).filter(|l| langs.contains(l)) else {
            continue;
        };
        let dict = dictionary(lang);
        if dict.check(word) {
            continue;
        }
        let mut suggestions = Vec::new();
        dict.suggest(word, &mut suggestions);
        suggestions.truncate(max_suggestions);
        out.push(Misspelling {
            range,
            word: word.to_string(),
            lang,
            suggestions,
        });
    }
    out
}

/// Replaces every misspelling that has a suggestion with its first suggestion.
pub fn apply_first_suggestions(text: &str, misspellings: &[Misspelling]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for m in misspellings {
        let Some(fix) = m.suggestions.first() else {
            continue;
        };
        if m.range.start < pos || text.get(m.range.clone()) != Some(m.word.as_str()) {
            continue;
        }
        out.push_str(&text[pos..m.range.start]);
        out.push_str(fix);
        pos = m.range.end;
    }
    out.push_str(&text[pos..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_words() {
        let text = "Привет, мир! It's из-за b2b 42 x_y -минус";
        let w: Vec<&str> = words(text).into_iter().map(|(_, w)| w).collect();
        assert_eq!(w, vec!["Привет", "мир", "It's", "из-за", "минус"]);
        let (range, word) = &words(text)[1];
        assert_eq!(&text[range.clone()], *word);
    }

    #[test]
    fn detects_script() {
        assert_eq!(word_lang("из-за"), Some(Lang::Ru));
        assert_eq!(word_lang("don't"), Some(Lang::En));
        assert_eq!(word_lang("mixТекст"), None);
        assert_eq!(word_lang("café"), None);
    }

    #[cfg(feature = "builtin-data")]
    #[test]
    fn checks_mixed_text() {
        let text = "Превет, как дила? Hello wrold!";
        let found = check_text(text, &Lang::ALL, 5, crate::data::dictionary);
        let words: Vec<&str> = found.iter().map(|m| m.word.as_str()).collect();
        assert_eq!(words, vec!["Превет", "дила", "wrold"]);
        assert!(
            found[0].suggestions.iter().any(|s| s == "Привет"),
            "{:?}",
            found[0].suggestions
        );
        assert!(
            found[2].suggestions.iter().any(|s| s == "world"),
            "{:?}",
            found[2].suggestions
        );
        let ru_only = check_text(text, &[Lang::Ru], 5, crate::data::dictionary);
        assert_eq!(ru_only.len(), 2);
    }

    #[test]
    fn applies_suggestions() {
        let text = "аа бб вв";
        let m = |r: std::ops::Range<usize>, s: &[&str]| Misspelling {
            word: text[r.clone()].to_string(),
            range: r,
            lang: Lang::Ru,
            suggestions: s.iter().map(|s| s.to_string()).collect(),
        };
        let fixes = vec![m(0..4, &["АА"]), m(5..9, &[]), m(10..14, &["ВВВ"])];
        assert_eq!(apply_first_suggestions(text, &fixes), "АА бб ВВВ");
    }

    #[test]
    fn ignores_stale_and_invalid_suggestion_ranges() {
        let make = |range, word: &str| Misspelling {
            range,
            word: word.into(),
            lang: Lang::Ru,
            suggestions: vec!["replacement".into()],
        };
        let text = "текст";
        let invalid = vec![
            make(0..100, text),
            make(1..4, "те"),
            make(0..4, "ст"),
            make(std::ops::Range { start: 4, end: 2 }, ""),
        ];
        assert_eq!(apply_first_suggestions(text, &invalid), text);
    }
}
