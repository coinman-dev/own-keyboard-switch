//! «Распознавать пароли»: which chunks typed without spaces are passwords.

use okbs_core::Lang;
use okbs_core::config::Config;
use okbs_core::detect::Detector;
use okbs_core::layouts::{KeyPress, builtin_keymap, keys_for_text};

fn keys(text: &str, layout: Lang) -> Vec<KeyPress> {
    keys_for_text(text, builtin_keymap(layout)).unwrap_or_else(|| panic!("cannot type {text}"))
}

/// Passwords the user typed with the Russian layout on: the keys are those of
/// the English password.
#[test]
fn passwords_typed_in_the_russian_layout_read_as_english() {
    let detector = Detector::from_config(&Config::default());
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
        assert_eq!(
            detector
                .password_reading(&keys(password, Lang::En))
                .as_deref(),
            Some(password),
            "{password}"
        );
    }
}

/// Russian words with digits and symbols stay Russian.
#[test]
fn russian_text_with_digits_and_symbols_is_not_a_password() {
    let detector = Detector::from_config(&Config::default());
    for text in [
        "Москва-2026",
        "Москве2026!",
        "Ваня123!",
        "Привет!!!2",
        "Кот_Мурзик_7",
        "Россия2026!",
        "Иван_Петров1",
        "2-комнатная!",
        "Шпаковский,",
        "Мурзик!",
        "Вован_77",
    ] {
        assert_eq!(
            detector.password_reading(&keys(text, Lang::Ru)),
            None,
            "{text}"
        );
    }
}

/// Ordinary words, numbers and abbreviations are no passwords in any layout.
#[test]
fn ordinary_tokens_are_not_passwords() {
    let detector = Detector::from_config(&Config::default());
    for (text, layout) in [
        ("Привет", Lang::Ru),
        ("ВКонтакте", Lang::Ru),
        ("МГУ", Lang::Ru),
        ("2026г.", Lang::Ru),
        ("15:30", Lang::Ru),
        ("100%", Lang::En),
        ("iPhone", Lang::En),
        ("hello,", Lang::En),
        ("qwerty123", Lang::En),
    ] {
        assert_eq!(
            detector.password_reading(&keys(text, layout)),
            None,
            "{text}"
        );
    }
}

/// Letter statistics of the Cyrillic runs, for tuning:
/// `cargo test -p okbs-core --test passwords run_scores -- --ignored --nocapture`.
#[test]
#[ignore = "diagnostic output"]
fn run_scores() {
    use okbs_core::data::language_model;
    use okbs_core::layouts::render;
    let samples: Vec<(&str, Lang)> = vec![
        ("BigVova", Lang::En),
        ("Tnone", Lang::En),
        ("StartToGo", Lang::En),
        ("Admin", Lang::En),
        ("Kot", Lang::En),
        ("Murzik", Lang::En),
        ("zXc", Lang::En),
        ("Summer", Lang::En),
        ("iPhone", Lang::En),
        ("HelloWorld", Lang::En),
        ("vova", Lang::En),
        ("Pit", Lang::En),
        ("Xo", Lang::En),
        ("Мурзик", Lang::Ru),
        ("Барсик", Lang::Ru),
        ("Шпаковский", Lang::Ru),
        ("Ваня", Lang::Ru),
        ("Вован", Lang::Ru),
        ("Котофей", Lang::Ru),
        ("Пушистик", Lang::Ru),
        ("Жучка", Lang::Ru),
    ];
    for (text, layout) in samples {
        let keys = keys(text, layout);
        let ru = render(&keys, builtin_keymap(Lang::Ru)).to_lowercase();
        let en = render(&keys, builtin_keymap(Lang::En)).to_lowercase();
        let r = language_model(Lang::Ru).score(&ru);
        let e = language_model(Lang::En).score(&en);
        println!(
            "{text:12} ru={ru:12} imp={} cost={:.2} vowel={} | en={en:12} imp={} cost={:.2}",
            r.impossible, r.cost, r.has_vowel, e.impossible, e.cost
        );
    }
}
