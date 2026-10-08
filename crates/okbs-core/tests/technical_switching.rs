#![cfg(feature = "builtin-data")]

use okbs_core::Lang;
use okbs_core::config::{Config, MatchKind, Rule, RuleAction};
use okbs_core::detect::{Decision, Detector, StayReason};
use okbs_core::layouts::{builtin_keymap, keys_for_text};

fn keys(word: &str) -> Vec<okbs_core::layouts::KeyPress> {
    keys_for_text(word, builtin_keymap(Lang::En)).expect("English fixture")
}

#[test]
fn technical_corpus_preserves_case_and_lexical_collisions() {
    // These physical keys also spell genuine Russian words or an acronym.
    // They remain ambiguous, independently of the added technical vocabulary.
    let collisions = ["gtk", "ide", "tls", "uv"];
    for extra_rules in [false, true] {
        let mut config = Config::default();
        config.rules_options.extra_rules = extra_rules;
        let detector = Detector::from_config(&config);
        let mut cases = 0;
        let mut ambiguous = 0;
        for word in include_str!("../../../data/technical-en.txt")
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let capitalized = format!("{}{}", word[..1].to_ascii_uppercase(), &word[1..]);
            for variant in [word.to_string(), capitalized, word.to_ascii_uppercase()] {
                cases += 1;
                let keys = keys(&variant);
                let wrong = detector.decide(&keys, Lang::Ru);
                let native = detector.decide(&keys, Lang::En);
                let collision = collisions.contains(&word) || variant == "VUE";
                if collision {
                    ambiguous += 1;
                    assert_eq!(wrong.switch_to(), None, "{variant}: {wrong:?}");
                } else {
                    assert!(
                        matches!(wrong, Decision::Switch { to: Lang::En, ref text, .. }
                        if text == &variant),
                        "{variant}: {wrong:?}"
                    );
                }
                // Preserve the established correction of VUE → МГУ.
                assert_eq!(
                    native.switch_to(),
                    (variant == "VUE").then_some(Lang::Ru),
                    "native {variant}: {native:?}"
                );
            }
        }
        assert_eq!(ambiguous, 13);
        assert_eq!(cases, 1308);
        println!("extra_rules={extra_rules}: {cases} cases, {ambiguous} lexical collisions");
    }
}

#[test]
fn reported_names_and_mixed_case_brands_switch_as_whole_words() {
    let detector = Detector::from_config(&Config::default());
    for word in [
        "Github",
        "GitHub",
        "windows",
        "Windows",
        "linux",
        "Linux",
        "GitLab",
        "JavaScript",
        "TypeScript",
        "NodeJS",
        "NextJS",
        "NuxtJS",
        "NestJS",
        "PostgreSQL",
        "MySQL",
        "MongoDB",
        "MariaDB",
        "ClickHouse",
        "CockroachDB",
        "DynamoDB",
        "CouchDB",
        "InfluxDB",
        "TimescaleDB",
        "GraphQL",
        "gRPC",
        "FastAPI",
        "OpenCV",
        "PyTorch",
        "TensorFlow",
        "NumPy",
        "SciPy",
        "PyQt",
        "PowerShell",
        "macOS",
        "iOS",
        "FreeBSD",
        "OpenBSD",
        "NetBSD",
        "CentOS",
        "IntelliJ",
        "PyCharm",
        "WebStorm",
        "GoLand",
        "CLion",
        "RubyMine",
        "PhpStorm",
        "VSCode",
        "VSCodium",
        "GitHubActions",
        "GitLabCI",
        "ArgoCD",
        "DevOps",
        "DevSecOps",
        "OpenTelemetry",
        "CircleCI",
        "TravisCI",
        "DigitalOcean",
        "OAuth",
        "JSONRPC",
        "Kubernetes",
        "npm",
        "pnpm",
        "rustc",
        "API",
        "GITHUB",
        "BITBUCKET",
        "ELASTICSEARCH",
        "FEDORA",
        "JENKINS",
    ] {
        for ending in ["", ",", ".", "!"] {
            let word = format!("{word}{ending}");
            let keys = keys(&word);
            let wrong = detector.decide(&keys, Lang::Ru);
            assert!(
                matches!(wrong, Decision::Switch { to: Lang::En, ref text, .. }
                if text == &word),
                "{word}: {wrong:?}"
            );
            assert_eq!(detector.decide(&keys, Lang::En).switch_to(), None, "{word}");
        }
    }
}

#[test]
fn technical_targets_respect_filters_user_rules_and_token_boundaries() {
    let mut config = Config::default();
    config.rules.push(Rule {
        pattern: "GitHub".into(),
        match_kind: MatchKind::Equals,
        case_sensitive: false,
        action: RuleAction::Stay,
        comment: String::new(),
    });
    assert_eq!(
        Detector::from_config(&config).decide(&keys("GitHub"), Lang::Ru),
        Decision::Stay(StayReason::Rule)
    );
    config.rules.clear();
    config.troubleshooting.detector.min_word_len = 7;
    assert_eq!(
        Detector::from_config(&config).decide(&keys("GitHub"), Lang::Ru),
        Decision::Stay(StayReason::TooShort)
    );
    config.troubleshooting.detector.min_word_len = 1;
    config.advanced.fix_abbreviations = false;
    assert_eq!(
        Detector::from_config(&config).decide(&keys("GITHUB"), Lang::Ru),
        Decision::Stay(StayReason::Abbreviation)
    );
    let detector = Detector::from_config(&Config::default());
    for word in ["GitHub2", "Neo4j", "GitHub2026"] {
        assert_eq!(
            detector.decide(&keys(word), Lang::Ru),
            Decision::Stay(StayReason::Digits)
        );
    }
    for word in [
        "myGitHub",
        "GitHubToken",
        "xGitHubx",
        "pAssWord",
        "iPhone",
        "gHbDtN",
    ] {
        assert_eq!(
            detector.decide(&keys(word), Lang::Ru).switch_to(),
            None,
            "{word}"
        );
    }
}

#[test]
fn b_switches_in_english_context_with_only_the_plan_exception() {
    let detector = Detector::from_config(&Config::default());
    for letter in ["b", "B"] {
        for context in [None, Some(Lang::En), Some(Lang::Ru)] {
            for after_plan in [false, true] {
                let decision = detector
                    .analyze_with_word_context(
                        &keys(letter),
                        Lang::En,
                        context,
                        Some(Lang::En),
                        after_plan,
                    )
                    .decision;
                if after_plan {
                    assert_eq!(decision, Decision::Stay(StayReason::NotConfident));
                } else {
                    let expected = if letter == "b" { "и" } else { "И" };
                    assert!(
                        matches!(decision, Decision::Switch { to: Lang::Ru, ref text, .. }
                        if text == expected),
                        "{letter}, context={context:?}: {decision:?}"
                    );
                }
            }
        }
    }
    // The requested b rule leaves other English letter labels intact.
    for letter in ["c", "d", "f", "r", "j", "e", "z"] {
        assert_eq!(
            detector
                .analyze(&keys(letter), Lang::En, Some(Lang::En))
                .decision,
            Decision::Stay(StayReason::NotConfident),
            "{letter}"
        );
    }
}

#[test]
fn b_context_rule_keeps_user_rules_and_minimum_length_priority() {
    let mut config = Config::default();
    config.rules.push(Rule {
        pattern: "b".into(),
        match_kind: MatchKind::Equals,
        case_sensitive: false,
        action: RuleAction::Stay,
        comment: String::new(),
    });
    let judge = |config: &Config, after_plan| {
        Detector::from_config(config)
            .analyze_with_word_context(
                &keys("b"),
                Lang::En,
                Some(Lang::En),
                Some(Lang::En),
                after_plan,
            )
            .decision
    };
    assert_eq!(judge(&config, false), Decision::Stay(StayReason::Rule));
    config.rules[0].action = RuleAction::Switch;
    assert_eq!(judge(&config, true).switch_to(), Some(Lang::Ru));
    config.rules.clear();
    config.troubleshooting.detector.min_word_len = 2;
    for after_plan in [false, true] {
        assert_eq!(
            judge(&config, after_plan),
            Decision::Stay(StayReason::TooShort)
        );
    }
}
