#![cfg(feature = "builtin-data")]

use okbs_core::Lang;
use okbs_core::config::{Config, MatchKind, Rule, RuleAction};
use okbs_core::detect::{Decision, Detector, StayReason, SwitchReason};
use okbs_core::layouts::{builtin_keymap, keys_for_text};

fn decide(config: &Config, text: &str, intended: Lang, current: Lang) -> Decision {
    let keys = keys_for_text(text, builtin_keymap(intended)).expect("test text");
    Detector::from_config(config).decide(&keys, current)
}

#[test]
fn rules_add_evidence_for_a_short_word_without_overriding_ambiguity() {
    let mut config = Config::default();
    assert_eq!(
        decide(&config, "кен", Lang::Ru, Lang::En),
        Decision::Switch {
            to: Lang::Ru,
            text: "кен".into(),
            reason: SwitchReason::ExtraRule,
        }
    );
    config.rules_options.extra_rules = false;
    assert_eq!(decide(&config, "кен", Lang::Ru, Lang::En).switch_to(), None);
    config.rules_options.extra_rules = true;
    for (lang, words) in [
        (Lang::Ru, ["рук", "руки", "руку", "ем"]),
        (Lang::En, ["her", "here", "bob", "eve"]),
    ] {
        for word in words {
            assert_eq!(
                decide(&config, word, lang, lang).switch_to(),
                None,
                "{word}"
            );
        }
    }
}

#[test]
fn user_rules_filters_and_dictionary_fallback_keep_their_priority() {
    let mut config = Config::default();
    config.rules.push(Rule {
        pattern: "rty".into(),
        match_kind: MatchKind::Equals,
        case_sensitive: false,
        action: RuleAction::Stay,
        comment: String::new(),
    });
    assert_eq!(
        decide(&config, "кен", Lang::Ru, Lang::En),
        Decision::Stay(StayReason::Rule)
    );
    config.rules.clear();
    config.troubleshooting.detector.min_word_len = 4;
    assert_eq!(
        decide(&config, "кен", Lang::Ru, Lang::En),
        Decision::Stay(StayReason::TooShort)
    );
    config.troubleshooting.detector.min_word_len = 1;
    assert_eq!(
        decide(&config, "gHbDtN", Lang::En, Lang::En),
        Decision::Stay(StayReason::MixedCase)
    );
    assert_eq!(
        decide(&config, "abc123", Lang::En, Lang::Ru),
        Decision::Stay(StayReason::Digits)
    );
    config.advanced.fix_abbreviations = false;
    assert_eq!(
        decide(&config, "ISO", Lang::En, Lang::Ru),
        Decision::Stay(StayReason::Abbreviation)
    );
    for word in ["in", "need", "needs", "hungry", "they're"] {
        assert_eq!(
            decide(&config, word, Lang::En, Lang::Ru).switch_to(),
            Some(Lang::En),
            "{word}"
        );
    }
}

#[test]
fn casing_and_full_reading_are_preserved() {
    let config = Config::default();
    for (lang, word) in [
        (Lang::Ru, "ёлка"),
        (Lang::Ru, "Ёлка"),
        (Lang::En, "don't"),
        (Lang::En, "hello,"),
    ] {
        let result = decide(&config, word, lang, lang.other());
        assert!(matches!(result, Decision::Switch { text, to, .. } if text == word && to == lang));
        assert_eq!(
            decide(&config, word, lang, lang).switch_to(),
            None,
            "{word}"
        );
    }
}
