//! Evaluation of the layout detector on held-out Tatoeba words
//! (`data/generated/eval-*.tsv`, not used to build the models).
//!
//! For every word we simulate typing it in the right layout (must stay) and in
//! the wrong layout (must switch). Metrics are weighted by word frequency.
//! Run with `cargo test -p okbs-core --release --test detector_eval -- --nocapture`
//! to see the report.

#![cfg(feature = "builtin-data")]

use okbs_core::Lang;
use okbs_core::config::Config;
use okbs_core::detect::{Decision, Detector, DetectorOptions};
use okbs_core::layouts::{builtin_keymap, keys_for_text};
use okbs_core::rules::RuleSet;
use std::sync::Arc;

#[derive(Default, Debug)]
struct Bucket {
    words: u64,
    tokens: u64,
    hit_words: u64,
    hit_tokens: u64,
}

impl Bucket {
    fn add(&mut self, count: u64, hit: bool) {
        self.words += 1;
        self.tokens += count;
        if hit {
            self.hit_words += 1;
            self.hit_tokens += count;
        }
    }

    fn token_rate(&self) -> f64 {
        if self.tokens == 0 {
            0.0
        } else {
            self.hit_tokens as f64 / self.tokens as f64
        }
    }

    fn word_rate(&self) -> f64 {
        if self.words == 0 {
            0.0
        } else {
            self.hit_words as f64 / self.words as f64
        }
    }
}

struct Report {
    lang: Lang,
    false_switch: Bucket,
    recall_single: Bucket,
    recall_short: Bucket,
    recall_long: Bucket,
    misses: Vec<(u64, String, String)>,
    false_switches: Vec<(u64, String, String)>,
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|f| f.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn evaluate(lang: Lang, detector: &Detector, capitalized: bool) -> Report {
    let path = format!(
        "{}/../../data/generated/eval-{}.tsv",
        env!("CARGO_MANIFEST_DIR"),
        lang.code()
    );
    let text = std::fs::read_to_string(&path).expect("evaluation data (run tools/build-lang-data)");
    let mut report = Report {
        lang,
        false_switch: Bucket::default(),
        recall_single: Bucket::default(),
        recall_short: Bucket::default(),
        recall_long: Bucket::default(),
        misses: Vec::new(),
        false_switches: Vec::new(),
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let Some((word, count)) = line.split_once('\t') else {
            continue;
        };
        let count: u64 = count.parse().expect("word count");
        let letters = word.chars().filter(|c| c.is_alphabetic()).count();
        if letters == 0 {
            continue;
        }
        let word = if capitalized {
            capitalize(word)
        } else {
            word.to_string()
        };
        let word = word.as_str();
        let Some(keys) = keys_for_text(word, builtin_keymap(lang)) else {
            continue;
        };

        let native = detector.decide(&keys, lang);
        let false_switch = native.switch_to().is_some();
        report.false_switch.add(count, false_switch);
        if false_switch {
            report
                .false_switches
                .push((count, word.to_string(), format!("{native:?}")));
        }

        let wrong = detector.decide(&keys, lang.other());
        let hit = wrong.switch_to() == Some(lang);
        if letters == 1 {
            if okbs_core::detect::single_letter_word(lang, word.chars().next().unwrap_or(' ')) {
                report.recall_single.add(count, hit);
            }
        } else if letters <= 3 {
            report.recall_short.add(count, hit);
        } else {
            report.recall_long.add(count, hit);
        }
        if !hit {
            let reason = match &wrong {
                Decision::Stay(r) => format!("{r:?}"),
                other => format!("{other:?}"),
            };
            report.misses.push((count, word.to_string(), reason));
        }
    }
    report.misses.sort_by_key(|m| std::cmp::Reverse(m.0));
    report
        .false_switches
        .sort_by_key(|m| std::cmp::Reverse(m.0));
    report
}

fn print(r: &Report, capitalized: bool) {
    println!(
        "== {}{} ==",
        r.lang,
        if capitalized { ", Capitalized" } else { "" }
    );
    println!(
        "false switches (typed correctly):  tokens {:.3}%  words {:.3}%  ({} words)",
        r.false_switch.token_rate() * 100.0,
        r.false_switch.word_rate() * 100.0,
        r.false_switch.words
    );
    println!(
        "recall, 1 letter (wrong layout):    tokens {:.2}%  words {:.2}%",
        r.recall_single.token_rate() * 100.0,
        r.recall_single.word_rate() * 100.0
    );
    println!(
        "recall, 2-3 letters (wrong layout): tokens {:.2}%  words {:.2}%",
        r.recall_short.token_rate() * 100.0,
        r.recall_short.word_rate() * 100.0
    );
    println!(
        "recall, 4+ letters (wrong layout):  tokens {:.2}%  words {:.2}%",
        r.recall_long.token_rate() * 100.0,
        r.recall_long.word_rate() * 100.0
    );
    println!(
        "top false switches: {:?}",
        &r.false_switches[..r.false_switches.len().min(25)]
    );
    println!("top misses: {:?}", &r.misses[..r.misses.len().min(40)]);
}

#[test]
fn detector_quality_on_held_out_words() {
    let detector = Detector::new(
        Arc::new(okbs_core::detect::BuiltinLexicon),
        DetectorOptions::default(),
        RuleSet::default(),
    );
    for lang in Lang::ALL {
        for capitalized in [false, true] {
            let report = evaluate(lang, &detector, capitalized);
            print(&report, capitalized);
            assert!(
                report.false_switch.token_rate() < 0.005,
                "{lang}: too many false switches"
            );
            assert!(
                report.false_switch.word_rate() < 0.005,
                "{lang}: too many distinct words switched by mistake"
            );
            assert!(
                report.recall_long.token_rate() >= 0.97,
                "{lang}: recall of long words too low"
            );
            // Frequent short words dominate the token rate and hid the fact
            // that most distinct two- and three-letter words never switched.
            assert!(
                report.recall_short.word_rate() >= 0.60,
                "{lang}: recall of distinct short words too low"
            );
        }
    }
}

#[test]
fn configured_extra_rules_quality() {
    let enabled = Detector::from_config(&Config::default());
    let mut config = Config::default();
    config.rules_options.extra_rules = false;
    let disabled = Detector::from_config(&config);
    for lang in Lang::ALL {
        for capitalized in [false, true] {
            let baseline = evaluate(lang, &disabled, capitalized);
            let report = evaluate(lang, &enabled, capitalized);
            print(&report, capitalized);
            println!(
                "baseline: false tokens={:.5}% short={:.3}% long={:.3}%",
                baseline.false_switch.token_rate() * 100.0,
                baseline.recall_short.token_rate() * 100.0,
                baseline.recall_long.token_rate() * 100.0
            );
            assert!(report.false_switch.token_rate() < 0.005);
            assert!(report.false_switch.word_rate() < 0.005);
            assert!(report.recall_long.token_rate() >= 0.97);
            assert!(report.recall_short.word_rate() >= 0.60);
            // Aggregate scores must not hide a newly damaged word. Expectations
            // here belong to our held-out corpus.
            for (_, word, _) in &report.false_switches {
                assert!(
                    baseline
                        .false_switches
                        .iter()
                        .any(|(_, old, _)| old == word),
                    "new false switch: {word}"
                );
            }
            for (_, word, _) in &report.misses {
                assert!(
                    baseline.misses.iter().any(|(_, old, _)| old == word),
                    "new missed correction: {word}"
                );
            }
            let fixed: Vec<_> = baseline
                .misses
                .iter()
                .filter(|(_, word, _)| !report.misses.iter().any(|(_, old, _)| old == word))
                .map(|(_, word, _)| word.as_str())
                .collect();
            println!(
                "newly corrected: {} words; examples {:?}",
                fixed.len(),
                &fixed[..fixed.len().min(30)]
            );
            assert!(!fixed.is_empty(), "extra rules should improve detection");
        }
    }
}
