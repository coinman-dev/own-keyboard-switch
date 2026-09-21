//! Builds the language data of Own Keyboard Switch.
//!
//! Inputs (download with `tools/fetch-lang-sources.sh`) in `data/sources/`:
//! Hunspell dictionaries and Tatoeba sentences. Outputs in `data/generated/`:
//! * `ru.lm.z`, `en.lm.z` — trigram language models (zlib);
//! * `ru_RU.aff.z`, `ru_RU.dic.z`, `en_US.aff.z`, `en_US.dic.z` — dictionaries (zlib);
//! * `eval-ru.tsv`, `eval-en.tsv` — held-out word frequencies for detector tests.
//!
//! Usage: `cargo run --release -p build-lang-data [-- <sources> <generated>]`

mod hunspell;

use anyhow::{Context, Result};
use okbs_core::Lang;
use okbs_core::lm::{self, BOUNDARY, COST_SCALE, LangModel, symbol_count, trigram_index};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Number of frequent words stored in a model.
const MODEL_WORDS: usize = 20_000;
/// Number of held-out words written for evaluation.
const EVAL_WORDS: usize = 30_000;
/// Every n-th sentence goes to the evaluation split.
const EVAL_EVERY: usize = 20;
/// A corpus trigram becomes "possible" when seen at least this often.
const CORPUS_POSSIBLE_MIN: u64 = 3;
/// Interpolation weights of trigram, bigram and unigram estimates.
const LAMBDA: [f64; 3] = [0.7, 0.2, 0.1];

struct Spec {
    lang: Lang,
    code: &'static str,
    dict: &'static str,
    corpus: &'static str,
}

const SPECS: [Spec; 2] = [
    Spec {
        lang: Lang::Ru,
        code: "ru",
        dict: "ru_RU",
        corpus: "rus_sentences.tsv",
    },
    Spec {
        lang: Lang::En,
        code: "en",
        dict: "en_US",
        corpus: "eng_sentences.tsv",
    },
];

fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut args = std::env::args_os().skip(1);
    let sources = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("data/sources"));
    let out = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("data/generated"));
    fs::create_dir_all(&out)?;

    for spec in &SPECS {
        build(spec, &sources, &out).with_context(|| format!("building {}", spec.code))?;
    }
    Ok(())
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| {
        format!(
            "cannot read {} (run tools/fetch-lang-sources.sh)",
            path.display()
        )
    })
}

fn write_compressed(path: &Path, data: &[u8]) -> Result<()> {
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(data, 10);
    fs::write(path, &packed).with_context(|| format!("cannot write {}", path.display()))?;
    println!(
        "  wrote {} ({} -> {} bytes)",
        path.display(),
        data.len(),
        packed.len()
    );
    Ok(())
}

/// Splits text into lowercase words of letters with inner joiners.
/// Tokens glued to digits or letters of another script are dropped.
fn words(lang: Lang, text: &str, mut f: impl FnMut(&str)) {
    let mut word = String::new();
    let mut poisoned = false;
    let mut pending_joiner: Option<char> = None;
    for ch in text.chars() {
        let lower = lm::to_lower(ch);
        if lm::letter_symbol(lang, lower).is_some() {
            if !poisoned {
                if let Some(j) = pending_joiner.take() {
                    word.push(if j == '’' { '\'' } else { j });
                }
                word.push(lower);
            }
        } else if lm::is_joiner(ch) && !word.is_empty() && pending_joiner.is_none() && !poisoned {
            pending_joiner = Some(ch);
        } else if ch.is_alphanumeric() {
            poisoned = true;
            word.clear();
            pending_joiner = None;
        } else {
            if !poisoned && !word.is_empty() {
                f(&word);
            }
            word.clear();
            poisoned = false;
            pending_joiner = None;
        }
    }
    if !poisoned && !word.is_empty() {
        f(&word);
    }
}

/// Symbol sequences of the letter runs of a word, with boundary padding.
fn runs(lang: Lang, word: &str, mut f: impl FnMut(&[usize])) {
    let mut seq = vec![BOUNDARY, BOUNDARY];
    for ch in word.chars() {
        match lm::letter_symbol(lang, ch) {
            Some(s) => seq.push(s),
            None => {
                if seq.len() > 2 {
                    seq.push(BOUNDARY);
                    f(&seq);
                }
                seq.truncate(2);
            }
        }
    }
    if seq.len() > 2 {
        seq.push(BOUNDARY);
        f(&seq);
    }
}

fn build(spec: &Spec, sources: &Path, out: &Path) -> Result<()> {
    let lang = spec.lang;
    let s = symbol_count(lang);
    let len = s * s * s;
    println!("[{}]", spec.code);

    // 1. Possible trigrams from all dictionary word forms.
    let aff_text = read(&sources.join(format!("{}.aff", spec.dict)))?;
    let dic_text = read(&sources.join(format!("{}.dic", spec.dict)))?;
    let aff = hunspell::AffixFile::parse(&aff_text)?;
    let mut possible = vec![false; len];
    let mut forms: u64 = 0;
    let mut skipped: u64 = 0;
    for (word, flags) in hunspell::dic_entries(&dic_text) {
        aff.expand(word, flags, &mut |form: &str| {
            let letters: Vec<char> = form.chars().filter(|c| c.is_alphabetic()).collect();
            let all_caps = letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase());
            let foreign = form
                .chars()
                .any(|c| !lm::is_joiner(c) && !lm::is_letter(lang, c));
            if all_caps || foreign {
                skipped += 1;
                return;
            }
            forms += 1;
            let lower: String = form.chars().map(lm::to_lower).collect();
            runs(lang, &lower, |seq| {
                for w in seq.windows(3) {
                    possible[trigram_index(lang, w[0], w[1], w[2])] = true;
                }
            });
        });
    }
    let from_dict = possible.iter().filter(|&&p| p).count();
    println!("  dictionary: {forms} forms ({skipped} skipped), {from_dict} possible trigrams");

    // 2. Corpus statistics.
    let corpus = read(&sources.join(spec.corpus))?;
    let mut train_counts: HashMap<String, u64> = HashMap::new();
    let mut eval_counts: HashMap<String, u64> = HashMap::new();
    let mut sentences = 0usize;
    for (i, line) in corpus.lines().enumerate() {
        let Some(text) = line.splitn(3, '\t').nth(2) else {
            continue;
        };
        sentences += 1;
        let target = if i % EVAL_EVERY == EVAL_EVERY - 1 {
            &mut eval_counts
        } else {
            &mut train_counts
        };
        words(lang, text, |w| {
            *target.entry(w.to_string()).or_default() += 1
        });
    }
    println!(
        "  corpus: {sentences} sentences, {} train words, {} eval words",
        train_counts.len(),
        eval_counts.len()
    );

    let mut tri = vec![0u64; len];
    for (word, &count) in &train_counts {
        runs(lang, word, |seq| {
            for w in seq.windows(3) {
                tri[trigram_index(lang, w[0], w[1], w[2])] += count;
            }
        });
    }
    let mut added = 0usize;
    for (i, &c) in tri.iter().enumerate() {
        if c >= CORPUS_POSSIBLE_MIN && !possible[i] {
            possible[i] = true;
            added += 1;
        }
    }
    println!(
        "  possible trigrams: {} of {len} ({added} added from corpus)",
        possible.iter().filter(|&&p| p).count()
    );

    // 3. Interpolated trigram costs.
    let mut bi = vec![0u64; s * s];
    let mut uni = vec![0u64; s];
    let mut ctx2 = vec![0u64; s * s];
    let mut ctx1 = vec![0u64; s];
    for a in 0..s {
        for b in 0..s {
            for c in 0..s {
                let n = tri[trigram_index(lang, a, b, c)];
                bi[b * s + c] += n;
                uni[c] += n;
                ctx2[a * s + b] += n;
            }
        }
    }
    for b in 0..s {
        for c in 0..s {
            ctx1[b] += bi[b * s + c];
        }
    }
    let total: u64 = uni.iter().sum();
    let mut costs = vec![0u8; len];
    for a in 0..s {
        for b in 0..s {
            for c in 0..s {
                let p1 = (uni[c] as f64 + 1.0) / (total as f64 + s as f64);
                let (w3, p3) = if ctx2[a * s + b] > 0 {
                    (
                        LAMBDA[0],
                        tri[trigram_index(lang, a, b, c)] as f64 / ctx2[a * s + b] as f64,
                    )
                } else {
                    (0.0, 0.0)
                };
                let (w2, p2) = if ctx1[b] > 0 {
                    (LAMBDA[1], bi[b * s + c] as f64 / ctx1[b] as f64)
                } else {
                    (0.0, 0.0)
                };
                let w1 = 1.0 - w3 - w2;
                let p = w3 * p3 + w2 * p2 + w1 * p1;
                let cost = (-p.log2() * f64::from(COST_SCALE))
                    .round()
                    .clamp(0.0, 255.0);
                costs[trigram_index(lang, a, b, c)] = cost as u8;
            }
        }
    }

    // 4. Frequent words.
    let mut ranked: Vec<(&String, &u64)> = train_counts.iter().filter(|(_, c)| **c >= 2).collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let model_words: Vec<String> = ranked
        .iter()
        .take(MODEL_WORDS)
        .map(|(w, _)| (*w).clone())
        .collect();

    let model = LangModel::from_parts(lang, costs, &possible, model_words)?;
    let bytes = model.to_bytes();
    let check = LangModel::from_bytes(&bytes)?;
    anyhow::ensure!(check == model, "model roundtrip mismatch");
    write_compressed(&out.join(format!("{}.lm.z", spec.code)), &bytes)?;
    write_compressed(
        &out.join(format!("{}.aff.z", spec.dict)),
        aff_text.as_bytes(),
    )?;
    write_compressed(
        &out.join(format!("{}.dic.z", spec.dict)),
        dic_text.as_bytes(),
    )?;

    // 5. Evaluation words.
    let mut eval: Vec<(&String, &u64)> = eval_counts.iter().collect();
    eval.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let mut text = format!(
        "# Held-out {} words from Tatoeba (CC-BY 2.0 FR), every {EVAL_EVERY}th sentence: word<TAB>count\n",
        spec.code
    );
    for (word, count) in eval.iter().take(EVAL_WORDS) {
        text.push_str(&format!("{word}\t{count}\n"));
    }
    let path = out.join(format!("eval-{}.tsv", spec.code));
    fs::write(&path, text)?;
    println!("  wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(lang: Lang, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        words(lang, text, |w| out.push(w.to_string()));
        out
    }

    #[test]
    fn tokenizes_words() {
        assert_eq!(
            collect(Lang::Ru, "Давайте что-нибудь попробуем! Tom, ёлка-"),
            vec!["давайте", "что-нибудь", "попробуем", "ёлка"]
        );
        assert_eq!(
            collect(Lang::En, "Let's try it’s 18th Muiriel's birthday -- ok"),
            vec!["let's", "try", "it's", "muiriel's", "birthday", "ok"]
        );
    }

    #[test]
    fn letter_runs() {
        let mut seqs = Vec::new();
        runs(Lang::En, "it's", |s| seqs.push(s.to_vec()));
        assert_eq!(seqs, vec![vec![0, 0, 9, 20, 0], vec![0, 0, 19, 0]]);
    }
}
