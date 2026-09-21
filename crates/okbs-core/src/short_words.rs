//! Additional English targets for the optional contextual correction layer.

use std::sync::OnceLock;

fn targets() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/generated/extra-short-en.txt"
        ))
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
    })
}

pub(crate) fn contains(word: &str) -> bool {
    targets().binary_search(&word).is_ok()
}

/// Reviewed labels, not ordinary Russian words or personal/place names.
/// Other uppercase-only dictionary entries do not protect a lowercase reading.
pub(crate) fn is_russian_label(word: &str) -> bool {
    matches!(word, "вшк" | "гр" | "фр" | "уч")
}

/// A surname observed in the project's Russian corpus but absent from the
/// spelling dictionary. Its lowercase spelling must not become English `del`.
pub(crate) fn is_russian_name(word: &str) -> bool {
    word == "вуд"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_list_is_sorted_unique_and_exact() {
        let words = targets();
        assert!(!words.is_empty());
        assert!(words.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(words.iter().all(|word| matches!(word.len(), 2 | 3)
            && word.bytes().all(|byte| byte.is_ascii_lowercase())));
        for word in ["dir", "iso", "ing"] {
            assert!(contains(word));
        }
        for word in ["ISO", "dir2", "directory", "рук", "xdir"] {
            assert!(!contains(word));
        }
    }
}
