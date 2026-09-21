//! Character trigram language models used by the layout detector.
//!
//! A model knows, for one language:
//! * the cost (negative log-probability) of every letter trigram, including
//!   word boundaries, estimated from a text corpus;
//! * which trigrams are *possible* at all: they occur in some word form of the
//!   Hunspell dictionary or often enough in the corpus. A word containing an
//!   impossible trigram is not a word of the language ("ghbdtn" is not English);
//! * the frequency ranks of the most common words.
//!
//! Models are produced by `tools/build-lang-data` and stored in a compact
//! binary format ([`LangModel::to_bytes`]).

use crate::lang::Lang;
use std::collections::HashMap;

/// Symbol index of the word boundary.
pub const BOUNDARY: usize = 0;

/// Number of symbols (letters + boundary) of a language.
pub const fn symbol_count(lang: Lang) -> usize {
    match lang {
        Lang::Ru => 34,
        Lang::En => 27,
    }
}

/// Symbol index (`1..symbol_count`) of a lowercase letter of `lang`.
pub fn letter_symbol(lang: Lang, c: char) -> Option<usize> {
    match lang {
        Lang::Ru => match c {
            'а'..='я' => Some(c as usize - 'а' as usize + 1),
            'ё' => Some(33),
            _ => None,
        },
        Lang::En => match c {
            'a'..='z' => Some(c as usize - 'a' as usize + 1),
            _ => None,
        },
    }
}

/// Whether `c` (in any case) is a letter of `lang`.
pub fn is_letter(lang: Lang, c: char) -> bool {
    letter_symbol(lang, to_lower(c)).is_some()
}

/// Simple lowercase mapping for single characters.
pub fn to_lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Characters that join parts of one word: hyphen and apostrophes.
pub fn is_joiner(c: char) -> bool {
    matches!(c, '-' | '\'' | '’')
}

/// Scale of quantized costs: `cost = round(-log2(p) * COST_SCALE)`.
pub const COST_SCALE: f32 = 8.0;

const MAGIC: &[u8; 8] = b"OKBSLM\x00\x01";

/// Error while decoding a model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LmError {
    /// Wrong magic bytes or version.
    #[error("not a language model file")]
    BadMagic,
    /// Unknown language code.
    #[error("unknown language code {0}")]
    BadLang(u8),
    /// The data ends too early or sizes do not match.
    #[error("language model file is truncated or inconsistent")]
    Truncated,
    /// A word is not valid UTF-8.
    #[error("language model contains invalid UTF-8")]
    BadUtf8,
    /// Decompression failed.
    #[error("cannot decompress language data")]
    Decompress,
}

/// Evaluation of a word by a [`LangModel`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WordScore {
    /// Letters of the language in the word.
    pub letters: u32,
    /// Characters that are neither letters of the language nor joiners.
    pub foreign: u32,
    /// Trigrams that never occur in the language.
    pub impossible: u32,
    /// Mean cost in bits per transition; lower is more natural. `0` without letters.
    pub cost: f32,
    /// The word contains a vowel of the language.
    pub has_vowel: bool,
}

/// A trigram language model of one language.
#[derive(Clone, PartialEq, Eq)]
pub struct LangModel {
    lang: Lang,
    costs: Box<[u8]>,
    possible: Box<[u64]>,
    words: Vec<Box<str>>,
    ranks: HashMap<Box<str>, u32>,
}

impl std::fmt::Debug for LangModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LangModel")
            .field("lang", &self.lang)
            .field("trigrams", &self.costs.len())
            .field("possible", &self.possible_count())
            .field("words", &self.words.len())
            .finish()
    }
}

/// Index of trigram `(a, b, c)` in the tables.
pub const fn trigram_index(lang: Lang, a: usize, b: usize, c: usize) -> usize {
    let s = symbol_count(lang);
    (a * s + b) * s + c
}

fn table_len(lang: Lang) -> usize {
    let s = symbol_count(lang);
    s * s * s
}

fn vowel(lang: Lang, c: char) -> bool {
    match lang {
        Lang::Ru => "аеёиоуыэюя".contains(c),
        Lang::En => "aeiouy".contains(c),
    }
}

impl LangModel {
    /// Builds a model from its parts.
    ///
    /// `costs` has one quantized cost per trigram, `possible` one flag per
    /// trigram, `words` are ordered from the most frequent.
    pub fn from_parts(
        lang: Lang,
        costs: Vec<u8>,
        possible: &[bool],
        words: Vec<String>,
    ) -> Result<Self, LmError> {
        let len = table_len(lang);
        if costs.len() != len || possible.len() != len {
            return Err(LmError::Truncated);
        }
        let mut bits = vec![0u64; len.div_ceil(64)];
        for (i, &p) in possible.iter().enumerate() {
            if p {
                bits[i / 64] |= 1 << (i % 64);
            }
        }
        Ok(Self::assemble(
            lang,
            costs.into_boxed_slice(),
            bits.into_boxed_slice(),
            words,
        ))
    }

    fn assemble(lang: Lang, costs: Box<[u8]>, possible: Box<[u64]>, words: Vec<String>) -> Self {
        let words: Vec<Box<str>> = words.into_iter().map(String::into_boxed_str).collect();
        let mut ranks = HashMap::with_capacity(words.len());
        for (rank, word) in words.iter().enumerate() {
            ranks.entry(word.clone()).or_insert(rank as u32);
        }
        Self {
            lang,
            costs,
            possible,
            words,
            ranks,
        }
    }

    /// Language of the model.
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// Quantized cost of trigram `(a, b, c)`.
    pub fn cost(&self, a: usize, b: usize, c: usize) -> u8 {
        self.costs[trigram_index(self.lang, a, b, c)]
    }

    /// Whether trigram `(a, b, c)` occurs in the language.
    pub fn is_possible(&self, a: usize, b: usize, c: usize) -> bool {
        let i = trigram_index(self.lang, a, b, c);
        self.possible[i / 64] & (1 << (i % 64)) != 0
    }

    /// Number of possible trigrams.
    pub fn possible_count(&self) -> usize {
        self.possible.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Frequency rank of a lowercase word (`0` = most frequent), if known.
    pub fn rank(&self, word_lower: &str) -> Option<u32> {
        self.ranks.get(word_lower).copied()
    }

    /// Frequent words, most frequent first.
    pub fn words(&self) -> &[Box<str>] {
        &self.words
    }

    /// Scores `word` (any case). Letter runs separated by joiners or foreign
    /// characters are scored as separate words.
    pub fn score(&self, word: &str) -> WordScore {
        let mut score = WordScore {
            letters: 0,
            foreign: 0,
            impossible: 0,
            cost: 0.0,
            has_vowel: false,
        };
        let mut total: u32 = 0;
        let mut transitions: u32 = 0;
        let (mut a, mut b) = (BOUNDARY, BOUNDARY);
        let mut in_run = false;
        let mut step = |a: usize, b: usize, c: usize, score: &mut WordScore| {
            total += u32::from(self.cost(a, b, c));
            transitions += 1;
            if !self.is_possible(a, b, c) {
                score.impossible += 1;
            }
        };
        for ch in word.chars() {
            let lower = to_lower(ch);
            match letter_symbol(self.lang, lower) {
                Some(c) => {
                    score.letters += 1;
                    score.has_vowel |= vowel(self.lang, lower);
                    step(a, b, c, &mut score);
                    (a, b) = (b, c);
                    in_run = true;
                }
                None => {
                    if in_run {
                        step(a, b, BOUNDARY, &mut score);
                        in_run = false;
                    }
                    (a, b) = (BOUNDARY, BOUNDARY);
                    if !is_joiner(ch) {
                        score.foreign += 1;
                    }
                }
            }
        }
        if in_run {
            step(a, b, BOUNDARY, &mut score);
        }
        if transitions > 0 {
            score.cost = total as f32 / transitions as f32 / COST_SCALE;
        }
        score
    }

    /// Serializes the model.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.costs.len() * 2);
        out.extend_from_slice(MAGIC);
        out.push(match self.lang {
            Lang::Ru => 0,
            Lang::En => 1,
        });
        out.extend_from_slice(&self.costs);
        for word in &self.possible {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&(self.words.len() as u32).to_le_bytes());
        for word in &self.words {
            let bytes = word.as_bytes();
            let len = u8::try_from(bytes.len()).unwrap_or(u8::MAX);
            out.push(len);
            out.extend_from_slice(&bytes[..usize::from(len)]);
        }
        out
    }

    /// Deserializes a model produced by [`to_bytes`](Self::to_bytes).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LmError> {
        let mut rest = bytes
            .strip_prefix(MAGIC.as_slice())
            .ok_or(LmError::BadMagic)?;
        let mut take = |n: usize| -> Result<&[u8], LmError> {
            if rest.len() < n {
                return Err(LmError::Truncated);
            }
            let (head, tail) = rest.split_at(n);
            rest = tail;
            Ok(head)
        };
        let lang = match take(1)?[0] {
            0 => Lang::Ru,
            1 => Lang::En,
            other => return Err(LmError::BadLang(other)),
        };
        let len = table_len(lang);
        let costs: Box<[u8]> = take(len)?.into();
        let possible: Box<[u64]> = take(len.div_ceil(64) * 8)?
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| u64::from_le_bytes(*c))
            .collect();
        let count = u32::from_le_bytes(take(4)?.try_into().map_err(|_| LmError::Truncated)?);
        let mut words = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let n = usize::from(take(1)?[0]);
            let word = std::str::from_utf8(take(n)?).map_err(|_| LmError::BadUtf8)?;
            words.push(word.to_string());
        }
        if !rest.is_empty() {
            return Err(LmError::Truncated);
        }
        Ok(Self::assemble(lang, costs, possible, words))
    }

    /// Decompresses (zlib) and deserializes a model.
    pub fn from_compressed(bytes: &[u8]) -> Result<Self, LmError> {
        let raw =
            miniz_oxide::inflate::decompress_to_vec_zlib(bytes).map_err(|_| LmError::Decompress)?;
        Self::from_bytes(&raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy_model(lang: Lang) -> LangModel {
        let len = table_len(lang);
        let mut possible = vec![false; len];
        let mut costs = vec![100u8; len];
        let sample = match lang {
            Lang::Ru => "аб",
            Lang::En => "ab",
        };
        let word: Vec<usize> = sample
            .chars()
            .filter_map(|c| letter_symbol(lang, c))
            .collect();
        let seq = [BOUNDARY, BOUNDARY, word[0], word[1], BOUNDARY];
        for w in seq.windows(3) {
            let i = trigram_index(lang, w[0], w[1], w[2]);
            possible[i] = true;
            costs[i] = 8;
        }
        LangModel::from_parts(lang, costs, &possible, vec!["the".into(), sample.into()]).unwrap()
    }

    #[test]
    fn symbols() {
        assert_eq!(letter_symbol(Lang::Ru, 'а'), Some(1));
        assert_eq!(letter_symbol(Lang::Ru, 'я'), Some(32));
        assert_eq!(letter_symbol(Lang::Ru, 'ё'), Some(33));
        assert_eq!(letter_symbol(Lang::Ru, 'a'), None);
        assert_eq!(letter_symbol(Lang::En, 'z'), Some(26));
        assert!(is_letter(Lang::Ru, 'Ё'));
        assert!(!is_letter(Lang::En, 'é'));
        assert!(is_joiner('-') && is_joiner('\'') && !is_joiner(','));
    }

    #[test]
    fn scoring() {
        let m = toy_model(Lang::En);
        let s = m.score("AB");
        assert_eq!((s.letters, s.foreign, s.impossible), (2, 0, 0));
        assert!((s.cost - 1.0).abs() < 1e-6);
        assert!(s.has_vowel);

        let s = m.score("ba");
        assert_eq!(s.impossible, 3);
        assert!(s.cost > 10.0);

        let s = m.score("ab-ab,");
        assert_eq!((s.letters, s.foreign, s.impossible), (4, 1, 0));

        let s = m.score("123");
        assert_eq!((s.letters, s.foreign, s.cost), (0, 3, 0.0));
    }

    #[test]
    fn roundtrip_and_ranks() {
        let m = toy_model(Lang::Ru);
        let bytes = m.to_bytes();
        let back = LangModel::from_bytes(&bytes).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.rank("the"), Some(0));
        assert_eq!(back.rank("аб"), Some(1));
        assert_eq!(back.rank("zz"), None);
        assert_eq!(back.possible_count(), 3);

        let packed = miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 9);
        assert_eq!(LangModel::from_compressed(&packed).unwrap(), m);

        assert_eq!(LangModel::from_bytes(b"nope"), Err(LmError::BadMagic));
        assert_eq!(
            LangModel::from_bytes(&bytes[..bytes.len() - 1]),
            Err(LmError::Truncated)
        );
        let mut extra = bytes.clone();
        extra.push(0);
        assert_eq!(LangModel::from_bytes(&extra), Err(LmError::Truncated));
    }
}
