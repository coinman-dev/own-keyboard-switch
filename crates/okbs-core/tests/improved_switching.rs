#![cfg(feature = "builtin-data")]

use okbs_core::Lang;
use okbs_core::config::{Config, MatchKind, Rule, RuleAction};
use okbs_core::detect::{Decision, Detector, StayReason, SwitchReason};
use okbs_core::layouts::{builtin_keymap, keys_for_text};

fn enabled() -> Config {
    let mut config = Config::default();
    config.rules_options.improve_switching = true;
    config
}

fn judge(
    config: &Config,
    text: &str,
    typed: Lang,
    current: Lang,
    previous: Option<Lang>,
) -> Decision {
    Detector::from_config(config)
        .analyze(
            &keys_for_text(text, builtin_keymap(typed)).expect("fixture"),
            current,
            previous,
        )
        .decision
}

#[test]
fn additional_targets_require_the_option_and_russian_context() {
    for word in ["iso", "dir", "ing", "aa", "aal"] {
        assert_eq!(
            judge(&enabled(), word, Lang::En, Lang::Ru, Some(Lang::Ru)),
            Decision::Switch {
                to: Lang::En,
                text: word.to_string(),
                reason: SwitchReason::ImprovedSwitching,
            },
            "{word}"
        );
        assert_eq!(
            judge(&Config::default(), word, Lang::En, Lang::Ru, Some(Lang::Ru)).switch_to(),
            None,
            "{word}"
        );
        for previous in [None, Some(Lang::En)] {
            assert_eq!(
                judge(&enabled(), word, Lang::En, Lang::Ru, previous),
                judge(&Config::default(), word, Lang::En, Lang::Ru, previous)
            );
        }
        assert_eq!(
            judge(&enabled(), word, Lang::En, Lang::En, Some(Lang::Ru)),
            judge(&Config::default(), word, Lang::En, Lang::En, Some(Lang::Ru))
        );
    }
}

#[test]
fn russian_words_names_and_inflections_are_protected() {
    for word in [
        "суд", "рук", "ну", "ищи", "душ", "увы", "ту", "щи", "вуз", "пуд", "луи", "уфы", "рур",
        "вуд",
    ] {
        assert_eq!(
            judge(&enabled(), word, Lang::Ru, Lang::Ru, Some(Lang::Ru)).switch_to(),
            None,
            "{word}"
        );
    }
    // A verified technical label is not treated as a place or personal name.
    assert_eq!(
        judge(&enabled(), "вшк", Lang::Ru, Lang::Ru, Some(Lang::Ru)).switch_to(),
        Some(Lang::En)
    );
}

#[test]
fn explicit_rules_filters_and_full_token_matching_keep_priority() {
    let mut config = enabled();
    config.rules.push(Rule {
        pattern: "шыщ".into(),
        match_kind: MatchKind::Equals,
        case_sensitive: false,
        action: RuleAction::Stay,
        comment: String::new(),
    });
    assert_eq!(
        judge(&config, "iso", Lang::En, Lang::Ru, Some(Lang::Ru)),
        Decision::Stay(StayReason::Rule)
    );
    config.rules[0].pattern = "рук".into();
    config.rules[0].action = RuleAction::Switch;
    assert_eq!(
        judge(&config, "рук", Lang::Ru, Lang::Ru, Some(Lang::Ru)).switch_to(),
        Some(Lang::En)
    );
    config.rules.clear();
    config.troubleshooting.detector.min_word_len = 4;
    assert_eq!(
        judge(&config, "iso", Lang::En, Lang::Ru, Some(Lang::Ru)),
        Decision::Stay(StayReason::TooShort)
    );
    for word in ["i2s", "iSo", "ISO", "xdirx", "dir.", "x-ing"] {
        assert_eq!(
            judge(&enabled(), word, Lang::En, Lang::Ru, Some(Lang::Ru)),
            judge(&Config::default(), word, Lang::En, Lang::Ru, Some(Lang::Ru)),
            "{word}"
        );
    }
}

#[test]
fn held_out_russian_words_gain_no_false_corrections() {
    let baseline = Detector::from_config(&Config::default());
    let improved = Detector::from_config(&enabled());
    let input = include_str!("../../../data/generated/eval-ru.tsv");
    for line in input.lines().filter(|line| !line.starts_with('#')) {
        let Some((word, _)) = line.split_once('\t') else {
            continue;
        };
        let Some(keys) = keys_for_text(word, builtin_keymap(Lang::Ru)) else {
            continue;
        };
        let old = baseline.analyze(&keys, Lang::Ru, Some(Lang::Ru)).decision;
        let new = improved.analyze(&keys, Lang::Ru, Some(Lang::Ru)).decision;
        assert!(
            old.switch_to().is_some() || new.switch_to().is_none(),
            "new false correction: {word}: {new:?}"
        );
    }
}
