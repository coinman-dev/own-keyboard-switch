//! Whole English software names and programming terms missing from Hunspell.
//! Case folding recognises brands such as GitHub without matching identifiers.

use std::sync::LazyLock;

static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    include_str!("../../../data/technical-en.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
});

pub(crate) fn contains(word: &str) -> bool {
    WORDS
        .binary_search(&word.to_ascii_lowercase().as_str())
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_words_are_sorted_unique_and_case_insensitive() {
        assert!(!WORDS.is_empty());
        assert!(WORDS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            WORDS
                .iter()
                .all(|word| word.len() >= 2 && word.bytes().all(|byte| byte.is_ascii_lowercase()))
        );
        for word in ["github", "Github", "GitHub", "GITHUB", "npm", "TypeScript"] {
            assert!(contains(word), "{word}");
        }
        for word in [
            "myGitHub",
            "GitHub2",
            "github_token",
            "github.com",
            "GitHub!",
            "и",
        ] {
            assert!(!contains(word), "{word}");
        }
    }
}
