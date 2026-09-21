//! Shows why a word is or is not converted, in both directions.
//!
//! `cargo run -p okbs-core --release --example diag_detector -- ISO лук You`
//!
//! Each word is typed once in the wrong layout, where it must switch, and once
//! in its own layout, where it must stay. Prefix a word with `ru:` or `en:` to
//! set its language explicitly.

use okbs_core::Lang;
use okbs_core::config::Config;
use okbs_core::detect::{Analysis, Decision, Detector};
use okbs_core::layouts::{builtin_keymap, keys_for_text, render};

fn verdict(a: &Analysis) -> String {
    match &a.decision {
        Decision::Switch { to, text, reason } => format!("SWITCH→{to:?} {text} ({reason:?})"),
        Decision::Stay(r) => format!("stay ({r:?})"),
        Decision::Suspicious => "suspicious".to_string(),
    }
}

fn facts(a: &Analysis) -> String {
    let f = |r: &okbs_core::detect::Reading| {
        format!(
            "[valid={} dict={} rank={:?} impossible={} cost={:.2} normalized={:?} rule={:?}]",
            r.valid as u8,
            r.in_dictionary as u8,
            r.rank,
            r.score.impossible,
            r.score.cost,
            okbs_core::extra_rules::normalize(&r.text),
            okbs_core::extra_rules::builtin().find(&r.text)
        )
    };
    format!("typed{} other{}", f(&a.current), f(&a.other))
}

fn guess_lang(word: &str) -> Lang {
    if word
        .chars()
        .any(|c| ('а'..='я').contains(&c) || ('А'..='Я').contains(&c))
    {
        Lang::Ru
    } else {
        Lang::En
    }
}

fn main() {
    let detector = Detector::from_config(&Config::default());
    for arg in std::env::args().skip(1) {
        let (word, lang) = match arg.split_once(':') {
            Some(("ru", w)) => (w.to_string(), Lang::Ru),
            Some(("en", w)) => (w.to_string(), Lang::En),
            _ => (arg.clone(), guess_lang(&arg)),
        };
        let Some(keys) = keys_for_text(&word, builtin_keymap(lang)) else {
            println!("{word}: cannot be typed in the {lang:?} layout");
            continue;
        };
        let wrong = detector.analyze(&keys, lang.other(), None);
        let right = detector.analyze(&keys, lang, None);
        println!(
            "{word} ({lang:?})\n  wrong layout, on screen {:<14} {}\n    {}\n  own layout,   on screen {:<14} {}\n    {}",
            render(&keys, builtin_keymap(lang.other())),
            verdict(&wrong),
            facts(&wrong),
            word,
            verdict(&right),
            facts(&right),
        );
    }
}
