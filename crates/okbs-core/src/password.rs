//! «Распознавать пароли»: does a chunk of typed text look like a password?
//!
//! A password is judged by its English reading, the layout passwords are
//! typed in: at least [`MIN_CHARS`] characters, [`MIN_LETTERS`] letters, and
//! three of the four kinds — lowercase, capitals, digits, other symbols. The
//! same rule is used by other layout switchers (Caramba Switcher: «longer
//! than six characters with lowercase letters, capitals, digits or
//! symbols»). Russian text with digits and symbols (`Москва-2026`,
//! `Ваня123!`, `Шпаковский,`) stays Russian: see
//! [`crate::detect::Detector::password_reading`].

/// Shortest password, in characters.
pub const MIN_CHARS: usize = 6;
/// Fewest letters: dates, times and numbers with a letter or two are not
/// passwords (`2026г.` reads `2026u/`).
pub const MIN_LETTERS: usize = 3;
/// Kinds of characters (lowercase, capitals, digits, symbols) a password mixes.
pub const MIN_KINDS: usize = 3;

/// Whether `english`, the English reading of a chunk without spaces, looks
/// like a password.
pub fn looks_like_password(english: &str) -> bool {
    let (mut chars, mut letters) = (0, 0);
    let (mut lower, mut upper, mut digit, mut symbol) = (false, false, false, false);
    for c in english.chars() {
        if c.is_whitespace() {
            return false;
        }
        chars += 1;
        if c.is_alphabetic() {
            letters += 1;
            if c.is_uppercase() {
                upper = true;
            } else {
                lower = true;
            }
        } else if c.is_ascii_digit() {
            digit = true;
        } else {
            symbol = true;
        }
    }
    let kinds = [lower, upper, digit, symbol].iter().filter(|&&k| k).count();
    chars >= MIN_CHARS && letters >= MIN_LETTERS && kinds >= MIN_KINDS
}

/// Runs of Cyrillic letters in `text` with at least two letters: the parts
/// that would have to be Russian words for the text to be Russian.
pub fn russian_runs(text: &str) -> Vec<String> {
    text.split(|c: char| !crate::lm::is_letter(crate::Lang::Ru, c))
        .filter(|run| run.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_of_three_kinds_are_passwords() {
        for password in [
            "BigVova#$",
            "Tnone21$",
            "StartToGo@11Go",
            "P@ssw0rd",
            "Qwerty123!",
            "MyDog2015!",
            "zXc!9Vb7",
            "Summer2026!",
            "iPhone15Pro",
            "S3cr3t!",
            "Pa$$w0rd",
            "1q2w3e4R",
            "!QAZ2wsx",
            "Kot_Murzik_7",
            "Xo4u_Pit$a",
            "Admin_2024",
            "HelloWorld42",
            "vova_1987",
        ] {
            assert!(looks_like_password(password), "{password}");
        }
    }

    /// `Hello,` mixes three kinds too: whether it is Russian text with a
    /// comma is decided by the Russian reading, see `password_reading`.
    #[test]
    fn words_numbers_and_short_tokens_are_not() {
        for text in [
            "hello",
            "iPhone",
            "qwerty123",
            "2026u/",
            "15:30",
            "100%",
            "+7(999)123-45-67",
            "Ab1!",
            "e-mail:",
            "big vova#$",
        ] {
            assert!(!looks_like_password(text), "{text}");
        }
    }

    #[test]
    fn russian_runs_skip_single_letters_and_symbols() {
        assert_eq!(russian_runs("Москва-2026"), vec!["Москва"]);
        assert_eq!(russian_runs("1й2ц3у4К"), Vec::<String>::new());
        assert_eq!(russian_runs("ИшпМщмф№;"), vec!["ИшпМщмф"]);
        assert_eq!(russian_runs("Ваня_и_Маша"), vec!["Ваня", "Маша"]);
    }
}
