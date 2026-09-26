//! Quality of automatic spelling correction on generated typos of held-out
//! Tatoeba words (`data/generated/eval-*.tsv`, not used for the frequency
//! ranks), and on correct words the dictionary does not know.
//!
//! Targets are drawn in proportion to their frequency, as typos hit words in
//! running text. Each gets one typing slip: a neighbouring key, two swapped
//! letters, a missing, extra or doubled letter. Typos that happen to be
//! words themselves are skipped: a dictionary cannot notice them.
//!
//! Two independent samples: `dev` for choosing thresholds, `test` for the
//! reported numbers. The guard test runs by default; for the full report and
//! the threshold sweep run
//! `cargo test -p okbs-core --release --test typo_eval -- --ignored --nocapture`.

#![cfg(feature = "builtin-data")]

use okbs_core::Lang;
use okbs_core::data::{dictionary, language_model};
use okbs_core::typo::{self, POLICY, Policy};

/// Deterministic xorshift generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn held_out(lang: Lang) -> Vec<(String, u64)> {
    let name = match lang {
        Lang::Ru => "eval-ru.tsv",
        Lang::En => "eval-en.tsv",
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/generated")
        .join(name);
    std::fs::read_to_string(path)
        .expect("held-out words")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (word, count) = line.split_once('\t')?;
            Some((word.to_string(), count.parse().ok()?))
        })
        .collect()
}

/// One typing slip in `word`, or `None` when it cannot be made.
fn slip(word: &str, lang: Lang, rng: &mut Rng) -> Option<String> {
    let mut chars: Vec<char> = word.chars().collect();
    let at = rng.below(chars.len());
    match rng.below(5) {
        0 => {
            let near = typo::neighbours(lang, chars[at]);
            chars[at] = *near.get(rng.below(near.len().max(1)))?;
        }
        1 => {
            let at = at.min(chars.len() - 2);
            if chars[at] == chars[at + 1] {
                return None;
            }
            chars.swap(at, at + 1);
        }
        2 => {
            chars.remove(at);
        }
        3 => {
            let near = typo::neighbours(lang, chars[at]);
            let extra = *near.get(rng.below(near.len().max(1)))?;
            chars.insert(at + rng.below(2), extra);
        }
        _ => chars.insert(at, chars[at]),
    }
    Some(chars.into_iter().collect())
}

/// A typo with the word the user meant and all dictionary suggestions.
struct Case {
    typed: String,
    meant: String,
    suggestions: Vec<String>,
}

fn cases(lang: Lang, seed: u64, count: usize) -> Vec<Case> {
    let words: Vec<(String, u64)> = held_out(lang)
        .into_iter()
        .filter(|(w, _)| w.chars().count() >= 4 && dictionary(lang).check(w))
        .collect();
    let total: u64 = words.iter().map(|(_, c)| c).sum();
    let mut rng = Rng(seed);
    let mut out = Vec::new();
    while out.len() < count {
        let mut pick = rng.next() % total;
        let (word, _) = words
            .iter()
            .find(|(_, c)| {
                if pick < *c {
                    true
                } else {
                    pick -= c;
                    false
                }
            })
            .expect("weighted pick");
        let Some(typed) = slip(word, lang, &mut rng) else {
            continue;
        };
        if typed == *word || dictionary(lang).check(&typed) {
            continue;
        }
        let mut suggestions = Vec::new();
        dictionary(lang).suggest(&typed, &mut suggestions);
        out.push(Case {
            typed,
            meant: word.clone(),
            suggestions,
        });
    }
    out
}

/// Correct but unknown words: held-out words the dictionary rejects in any
/// case (surnames, slang, new terms). None of them may be changed.
fn unknown_words(lang: Lang, count: usize) -> Vec<(String, Vec<String>)> {
    held_out(lang)
        .into_iter()
        .filter(|(w, _)| {
            let mut capital = w.chars();
            let capitalized: String = capital
                .next()
                .map(|c| c.to_uppercase().chain(capital).collect())
                .unwrap_or_default();
            w.chars().count() >= 4
                && w.chars().all(|c| okbs_core::lm::is_letter(lang, c))
                && !dictionary(lang).check(w)
                && !dictionary(lang).check(&capitalized)
        })
        .take(count)
        .map(|(w, _)| {
            let mut suggestions = Vec::new();
            dictionary(lang).suggest(&w, &mut suggestions);
            (w, suggestions)
        })
        .collect()
}

#[derive(Debug, Default, Clone, Copy)]
struct Score {
    typos: usize,
    meant_suggested: usize,
    fixed: usize,
    wrong: usize,
    unknown: usize,
    unknown_changed: usize,
}

impl Score {
    fn precision(&self) -> f64 {
        self.fixed as f64 / (self.fixed + self.wrong).max(1) as f64
    }
    fn coverage(&self) -> f64 {
        self.fixed as f64 / self.typos.max(1) as f64
    }
}

impl std::fmt::Display for Score {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "typos {:4}, meant among suggestions {:5.1}%, fixed right {:5.1}%, fixed wrong {:4.1}% \
             (precision {:5.1}%), unknown words changed {}/{}",
            self.typos,
            100.0 * self.meant_suggested as f64 / self.typos.max(1) as f64,
            100.0 * self.coverage(),
            100.0 * self.wrong as f64 / self.typos.max(1) as f64,
            100.0 * self.precision(),
            self.unknown_changed,
            self.unknown,
        )
    }
}

fn evaluate(
    lang: Lang,
    cases: &[Case],
    unknown: &[(String, Vec<String>)],
    policy: &Policy,
) -> Score {
    let model = language_model(lang);
    let rank = |w: &str| model.rank(w);
    let mut score = Score {
        typos: cases.len(),
        unknown: unknown.len(),
        ..Score::default()
    };
    for case in cases {
        if case.suggestions.contains(&case.meant) {
            score.meant_suggested += 1;
        }
        match typo::automatic_correction(&case.typed, &case.suggestions, lang, policy, rank) {
            Some(fix) if fix.to_lowercase() == case.meant => score.fixed += 1,
            Some(_) => score.wrong += 1,
            None => {}
        }
    }
    for (word, suggestions) in unknown {
        if typo::automatic_correction(word, suggestions, lang, policy, rank).is_some() {
            score.unknown_changed += 1;
        }
    }
    score
}

/// Safety guard on the default thresholds: silent changes are almost always right.
#[test]
fn automatic_correction_is_precise() {
    for lang in Lang::ALL {
        let cases = cases(lang, 0x5EED_0002, 150);
        let unknown = unknown_words(lang, 60);
        let score = evaluate(lang, &cases, &unknown, &POLICY);
        println!("{lang}: {score}");
        // Measured on the full test sample: precision 99.8% (ru) and 100% (en),
        // 67% and 61% of typos fixed, 4% and 2% of unknown lowercase words
        // changed (mostly names, which the program skips when capitalized).
        assert!(score.precision() >= 0.98, "{lang}: {score}");
        assert!(score.coverage() >= 0.5, "{lang}: {score}");
        assert!(
            score.unknown_changed * 100 <= score.unknown * 8,
            "{lang}: {score}"
        );
    }
}

/// Which unknown words the program would change, to judge whether they are
/// really correct words or typos of the corpus itself.
#[test]
#[ignore = "report: run with --ignored --nocapture"]
fn unknown_word_examples() {
    for lang in Lang::ALL {
        let model = language_model(lang);
        for (word, suggestions) in unknown_words(lang, 300) {
            if let Some(fix) =
                typo::automatic_correction(&word, &suggestions, lang, &POLICY, |w| model.rank(w))
            {
                println!("{lang}: {word} -> {fix}");
            }
        }
    }
}

/// Full report and the threshold sweep used to choose [`POLICY`].
#[test]
#[ignore = "slow report: run with --ignored --nocapture"]
fn report_and_sweep() {
    for lang in Lang::ALL {
        let dev = cases(lang, 0x5EED_0001, 600);
        let test = cases(lang, 0x5EED_0002, 600);
        let unknown = unknown_words(lang, 300);
        println!("== {lang} dev sweep (min_margin, max_cost, rarity)");
        for min_margin in [0.2, 0.3, 0.35, 0.4, 0.5, 0.6] {
            for max_cost in [0.6, 1.0, 1.2, 1.6] {
                for rarity in [0.0, 0.2, 0.4, 0.8] {
                    let policy = Policy {
                        min_margin,
                        max_cost,
                        rarity,
                        ..POLICY
                    };
                    let score = evaluate(lang, &dev, &unknown, &policy);
                    println!("{min_margin:4.2} {max_cost:3.1} {rarity:3.1}  {score}");
                }
            }
        }
        println!("== {lang} test with POLICY {POLICY:?}");
        println!("{}", evaluate(lang, &test, &unknown, &POLICY));
        for case in test.iter().take(40) {
            let fix =
                typo::automatic_correction(&case.typed, &case.suggestions, lang, &POLICY, |w| {
                    language_model(lang).rank(w)
                });
            println!(
                "  {:>16} meant {:>16} -> {:?}  {:?}",
                case.typed,
                case.meant,
                fix,
                case.suggestions.iter().take(5).collect::<Vec<_>>()
            );
        }
    }
    for word in [
        "арфографии",
        "паботает",
        "хочеш",
        "теасты",
        "ничегно",
        "поши",
        "разробраться",
    ] {
        let mut suggestions = Vec::new();
        dictionary(Lang::Ru).suggest(word, &mut suggestions);
        let fix = typo::automatic_correction(word, &suggestions, Lang::Ru, &POLICY, |w| {
            language_model(Lang::Ru).rank(w)
        });
        println!("{word}: {fix:?} from {suggestions:?}");
    }
}
