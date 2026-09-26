//! Research prototype: does knowing the previous words make automatic
//! spelling correction better than the dictionary-only choice of
//! `okbs_core::typo`?
//!
//! Corpus: Tatoeba sentences in `data/sources` (CC-BY 2.0 FR). The split is
//! the one of `build-lang-data`: every 20th sentence is held out. Held-out
//! sentences that also occur in the training part are dropped, so no test
//! sentence was seen in training.
//!
//! Model: word unigrams, bigrams and trigrams of the training part with
//! "stupid backoff" (0.4 per shorter context). Cases: one typing slip in a
//! held-out word that has two previous words in its sentence. Only the words
//! before the typo are used, as while typing.
//!
//! Both methods rank the same Hunspell suggestions and must leave the word
//! when unsure. Thresholds are chosen on the `dev` half, numbers are
//! reported for the `test` half.
//!
//!   cargo run --release -p context-eval

use anyhow::{Context, Result};
use okbs_core::Lang;
use okbs_core::data::{dictionary, language_model};
use okbs_core::lm;
use okbs_core::typo::{self, POLICY, Policy};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const EVAL_EVERY: usize = 20;
const BACKOFF: f64 = 0.4;
const CASES: usize = 2000;

/// Lowercase words of a sentence: letters of `lang` with inner joiners.
fn tokens(lang: Lang, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if lm::is_letter(lang, c) || (lm::is_joiner(c) && !word.is_empty()) {
            word.push(lm::to_lower(c));
        } else if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    for w in &mut out {
        while w.ends_with(lm::is_joiner) {
            w.pop();
        }
    }
    out.retain(|w| !w.is_empty());
    out
}

/// Word n-gram counts of the training sentences.
#[derive(Default)]
struct Ngrams {
    ids: HashMap<String, u32>,
    uni: Vec<u64>,
    total: u64,
    bi: HashMap<(u32, u32), u32>,
    tri: HashMap<(u32, u32, u32), u32>,
}

impl Ngrams {
    fn id(&mut self, word: &str) -> u32 {
        if let Some(&id) = self.ids.get(word) {
            return id;
        }
        let id = self.uni.len() as u32;
        self.ids.insert(word.to_string(), id);
        self.uni.push(0);
        id
    }

    fn add(&mut self, sentence: &[String]) {
        let ids: Vec<u32> = sentence.iter().map(|w| self.id(w)).collect();
        for (i, &w) in ids.iter().enumerate() {
            self.uni[w as usize] += 1;
            self.total += 1;
            if i >= 1 {
                *self.bi.entry((ids[i - 1], w)).or_default() += 1;
            }
            if i >= 2 {
                *self.tri.entry((ids[i - 2], ids[i - 1], w)).or_default() += 1;
            }
        }
    }

    /// Drops n-grams seen fewer than `min` times, as a shipped model would.
    fn prune(&mut self, min: u32) {
        self.bi.retain(|_, c| *c >= min);
        self.tri.retain(|_, c| *c >= min);
    }

    /// Stupid-backoff score of `word` after `u v` (natural log).
    fn log_score(&self, u: &str, v: &str, word: &str) -> f64 {
        let Some(&w) = self.ids.get(word) else {
            // Unseen word: below any seen one.
            return (0.5 / self.total as f64).ln() + 2.0 * BACKOFF.ln();
        };
        let (u, v) = (self.ids.get(u).copied(), self.ids.get(v).copied());
        if let (Some(u), Some(v)) = (u, v)
            && let (Some(&c), Some(&ctx)) = (self.tri.get(&(u, v, w)), self.bi.get(&(u, v)))
        {
            return (f64::from(c) / f64::from(ctx)).ln();
        }
        if let Some(v) = v
            && let Some(&c) = self.bi.get(&(v, w))
        {
            return BACKOFF.ln() + (f64::from(c) / self.uni[v as usize] as f64).ln();
        }
        2.0 * BACKOFF.ln() + (self.uni[w as usize] as f64 / self.total as f64).ln()
    }
}

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
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One typing slip, as in `crates/okbs-core/tests/typo_eval.rs`.
fn slip(word: &str, lang: Lang, rng: &mut Rng) -> Option<String> {
    let mut chars: Vec<char> = word.chars().collect();
    let at = rng.below(chars.len());
    match rng.below(5) {
        0 => {
            let near = typo::neighbours(lang, chars[at]);
            chars[at] = *near.get(rng.below(near.len()))?;
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
            let extra = *near.get(rng.below(near.len()))?;
            chars.insert(at + rng.below(2), extra);
        }
        _ => chars.insert(at, chars[at]),
    }
    Some(chars.into_iter().collect())
}

struct Case {
    before: [String; 2],
    typed: String,
    /// `None`: a correct word the dictionary does not know; must stay.
    meant: Option<String>,
    suggestions: Vec<String>,
}

/// Decision with the previous words: the dictionary score minus `weight`
/// times the context log score, relative to the best candidate.
fn with_context(
    case: &Case,
    lang: Lang,
    model: &Ngrams,
    policy: &Policy,
    weight: f64,
    margin: f32,
) -> Option<String> {
    if case.typed.chars().count() < policy.min_letters {
        return None;
    }
    let rank = |w: &str| language_model(lang).rank(w);
    let scored = typo::score_suggestions(&case.typed, &case.suggestions, lang, policy, rank);
    let mut combined: Vec<(f64, &typo::Scored)> = scored
        .iter()
        .map(|s| {
            let context = model.log_score(&case.before[0], &case.before[1], &s.word.to_lowercase());
            (f64::from(s.score) - weight * context, s)
        })
        .collect();
    combined.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (best_score, best) = combined.first()?;
    if best.cost > policy.max_cost {
        return None;
    }
    if let Some((second, _)) = combined.get(1)
        && second - best_score < f64::from(margin)
    {
        return None;
    }
    Some(best.word.clone())
}

#[derive(Default, Debug, Clone, Copy)]
struct Score {
    typos: usize,
    fixed: usize,
    wrong: usize,
    unknown: usize,
    unknown_changed: usize,
    top1: usize,
    meant_suggested: usize,
}

impl std::fmt::Display for Score {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
        write!(
            f,
            "fixed right {:5.1}%, fixed wrong {:4.1}% (precision {:5.1}%), \
             top-1 when suggested {:5.1}%, unknown changed {:4.1}%",
            pct(self.fixed, self.typos),
            pct(self.wrong, self.typos),
            pct(self.fixed, self.fixed + self.wrong),
            pct(self.top1, self.meant_suggested),
            pct(self.unknown_changed, self.unknown),
        )
    }
}

fn evaluate(
    cases: &[Case],
    decide: impl Fn(&Case) -> Option<String>,
    top1: impl Fn(&Case) -> Option<String>,
) -> Score {
    let mut score = Score::default();
    for case in cases {
        let fix = decide(case).map(|f| f.to_lowercase());
        match &case.meant {
            Some(meant) => {
                score.typos += 1;
                match fix {
                    Some(fix) if fix == *meant => score.fixed += 1,
                    Some(_) => score.wrong += 1,
                    None => {}
                }
                if case.suggestions.iter().any(|s| s.to_lowercase() == *meant) {
                    score.meant_suggested += 1;
                    if top1(case).is_some_and(|t| t.to_lowercase() == *meant) {
                        score.top1 += 1;
                    }
                }
            }
            None => {
                score.unknown += 1;
                if fix.is_some() {
                    score.unknown_changed += 1;
                }
            }
        }
    }
    score
}

fn run(lang: Lang, sources: &Path) -> Result<()> {
    let file = match lang {
        Lang::Ru => "rus_sentences.tsv",
        Lang::En => "eng_sentences.tsv",
    };
    let corpus = std::fs::read_to_string(sources.join(file)).context(file)?;
    let mut train: Vec<Vec<String>> = Vec::new();
    let mut held: Vec<Vec<String>> = Vec::new();
    for (i, line) in corpus.lines().enumerate() {
        let Some(text) = line.splitn(3, '\t').nth(2) else {
            continue;
        };
        let words = tokens(lang, text);
        if words.is_empty() {
            continue;
        }
        if i % EVAL_EVERY == EVAL_EVERY - 1 {
            held.push(words);
        } else {
            train.push(words);
        }
    }
    let seen: HashSet<&Vec<String>> = train.iter().collect();
    let held_total = held.len();
    held.retain(|s| !seen.contains(s));
    let mut model = Ngrams::default();
    for sentence in &train {
        model.add(sentence);
    }
    println!(
        "[{lang}] train {} sentences, {} tokens, {} words; held out {} of {held_total} unseen",
        train.len(),
        model.total,
        model.uni.len(),
        held.len()
    );
    let (bi_all, tri_all) = (model.bi.len(), model.tri.len());
    model.prune(2);
    println!(
        "  n-grams: bigrams {bi_all} -> {} kept (count >= 2), trigrams {tri_all} -> {} kept; \
         about {:.1} MB as (ids + count) records",
        model.bi.len(),
        model.tri.len(),
        (model.bi.len() * 12 + model.tri.len() * 16) as f64 / 1e6
    );

    // Cases: typos with two previous words, and unknown correct words.
    let dict = dictionary(lang);
    let mut rng = Rng(0x0C0F_FEE5 ^ lang as u64);
    let mut cases = Vec::new();
    let mut unknown = 0usize;
    let mut tries = 0usize;
    while cases.len() < CASES && tries < CASES * 200 {
        tries += 1;
        let sentence = &held[rng.below(held.len())];
        if sentence.len() < 3 {
            continue;
        }
        let at = 2 + rng.below(sentence.len() - 2);
        let word = &sentence[at];
        if word.chars().count() < 4 {
            continue;
        }
        let before = [sentence[at - 2].clone(), sentence[at - 1].clone()];
        let known = dict.check(word);
        if !known {
            let mut capital = word.chars();
            let capitalized: String = capital
                .next()
                .map(|c| c.to_uppercase().chain(capital).collect())
                .unwrap_or_default();
            if dict.check(&capitalized) || unknown * 5 >= CASES {
                continue;
            }
            unknown += 1;
            let mut suggestions = Vec::new();
            dict.suggest(word, &mut suggestions);
            cases.push(Case {
                before,
                typed: word.clone(),
                meant: None,
                suggestions,
            });
            continue;
        }
        let Some(typed) = slip(word, lang, &mut rng) else {
            continue;
        };
        if typed == *word || dict.check(&typed) {
            continue;
        }
        let mut suggestions = Vec::new();
        dict.suggest(&typed, &mut suggestions);
        cases.push(Case {
            before,
            typed,
            meant: Some(word.clone()),
            suggestions,
        });
    }
    let (dev, test): (Vec<_>, Vec<_>) =
        cases.into_iter().enumerate().partition(|(i, _)| i % 2 == 0);
    let dev: Vec<Case> = dev.into_iter().map(|(_, c)| c).collect();
    let test: Vec<Case> = test.into_iter().map(|(_, c)| c).collect();
    let rank = |w: &str| language_model(lang).rank(w);
    let base = |c: &Case| typo::automatic_correction(&c.typed, &c.suggestions, lang, &POLICY, rank);
    let base_top1 = |c: &Case| {
        typo::score_suggestions(&c.typed, &c.suggestions, lang, &POLICY, rank)
            .first()
            .map(|s| s.word.clone())
    };
    println!("  dev base:    {}", evaluate(&dev, base, base_top1));

    // Choose weight and margin on dev: most right fixes while the wrong
    // fixes and changed unknown words stay at or below the base method.
    let base_dev = evaluate(&dev, base, base_top1);
    let mut best: Option<(f64, f32, Score)> = None;
    for weight in [0.05, 0.1, 0.2, 0.3, 0.5, 0.8] {
        for margin in [0.3f32, 0.5, 0.7, 1.0, 1.4, 2.0, 3.0] {
            let decide = |c: &Case| with_context(c, lang, &model, &POLICY, weight, margin);
            let top1 = |c: &Case| with_context(c, lang, &model, &POLICY, weight, -1e9);
            let score = evaluate(&dev, decide, top1);
            let safe =
                score.wrong <= base_dev.wrong && score.unknown_changed <= base_dev.unknown_changed;
            if safe && best.as_ref().is_none_or(|(_, _, b)| score.fixed > b.fixed) {
                best = Some((weight, margin, score));
            }
        }
    }
    println!("  test base:   {}", evaluate(&test, base, base_top1));
    match best {
        Some((weight, margin, dev_score)) => {
            println!("  dev context: {dev_score}  (weight {weight}, margin {margin})");
            let decide = |c: &Case| with_context(c, lang, &model, &POLICY, weight, margin);
            let top1 = |c: &Case| with_context(c, lang, &model, &POLICY, weight, -1e9);
            println!("  test context:{}", evaluate(&test, decide, top1));
        }
        None => println!("  no context setting is as safe as the base method on dev"),
    }
    Ok(())
}

fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sources = root.join("data/sources");
    for lang in Lang::ALL {
        run(lang, &sources)?;
    }
    Ok(())
}
