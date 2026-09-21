//! Automatic layout detection: was this word typed in the wrong layout?
//!
//! The detector receives the physical keys of a finished word and the layout
//! it was typed in, renders the keys in both layouts and decides. See
//! `.ai/04-language-detection-algorithm.md`. Stages, most reliable first:
//!
//! 1. no letters/digits; user rules; single-letter context and remaining filters;
//! 2. optional built-in rules with independent lexical guards;
//! 3. the word as typed is known → stay (subject to rare-word arbitration);
//! 4. the other rendering is a dictionary word and the typed one is not
//!    plausible (impossible trigrams or much less natural) → switch;
//! 5. neither is a dictionary word: switch only on strong statistical evidence.

use crate::config::{Config, RuleAction};
use crate::keymap::KeyMap;
use crate::lang::Lang;
use crate::layouts::{self, KeyPress};
use crate::lm::{self, LangModel, WordScore};
use crate::rules::RuleSet;
use std::fmt;
use std::sync::Arc;

/// Word knowledge of the supported languages.
pub trait Lexicon: Send + Sync + fmt::Debug {
    /// Trigram model of `lang`.
    fn model(&self, lang: Lang) -> &LangModel;
    /// Whether `word` (in its typed capitalization) is a dictionary word of `lang`.
    fn is_word(&self, lang: Lang, word: &str) -> bool;
}

/// The lexicon built into the binary.
#[cfg(feature = "builtin-data")]
#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinLexicon;

#[cfg(feature = "builtin-data")]
impl Lexicon for BuiltinLexicon {
    fn model(&self, lang: Lang) -> &LangModel {
        crate::data::language_model(lang)
    }

    fn is_word(&self, lang: Lang, word: &str) -> bool {
        crate::data::dictionary(lang).check(word)
    }
}

/// Detector settings taken from the configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectorOptions {
    /// Words with fewer letters are never converted.
    pub min_word_len: u32,
    /// `-1.0` (cautious) ..= `1.0` (eager).
    pub sensitivity: f32,
    /// Never convert words with digits or mixed case.
    pub password_heuristic: bool,
    /// Convert words typed in capitals.
    pub fix_abbreviations: bool,
}

impl Default for DetectorOptions {
    fn default() -> Self {
        Self {
            min_word_len: 1,
            sensitivity: 0.0,
            password_heuristic: true,
            fix_abbreviations: true,
        }
    }
}

impl From<&Config> for DetectorOptions {
    fn from(config: &Config) -> Self {
        let det = &config.troubleshooting.detector;
        Self {
            min_word_len: det.min_word_len,
            sensitivity: det.sensitivity,
            password_heuristic: det.password_heuristic,
            fix_abbreviations: config.advanced.fix_abbreviations,
        }
    }
}

/// Why a word is left as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StayReason {
    /// Fewer letters than `min_word_len`.
    TooShort,
    /// Contains digits.
    Digits,
    /// Mixed capitalization such as `iPhone` or `pAssWord`.
    MixedCase,
    /// Written in capitals and abbreviations are not corrected.
    Abbreviation,
    /// A user rule said so.
    Rule,
    /// An extra rule protects this reading.
    ExtraRule,
    /// The word as typed is a known word.
    KnownWord,
    /// Not enough evidence.
    NotConfident,
}

/// Why a word is converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchReason {
    /// A user rule said so.
    Rule,
    /// Extra rules favour the other reading.
    ExtraRule,
    /// An additional English target after a Russian word or letter.
    ImprovedSwitching,
    /// The other rendering is a dictionary word.
    Dictionary,
    /// Letter statistics strongly favour the other language.
    Statistics,
}

/// Outcome of detection.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Keep the word.
    Stay(StayReason),
    /// Retype the word in layout `to`; `text` is the expected result.
    Switch {
        /// Target language.
        to: Lang,
        /// The word rendered in the target layout.
        text: String,
        /// Evidence.
        reason: SwitchReason,
    },
    /// Probably a typo: implausible in both layouts. Nothing is changed.
    Suspicious,
}

impl Decision {
    /// Target language when the decision is to switch.
    pub fn switch_to(&self) -> Option<Lang> {
        match self {
            Decision::Switch { to, .. } => Some(*to),
            _ => None,
        }
    }
}

/// Letter case pattern of a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasePattern {
    /// `привет`.
    Lower,
    /// `Привет`.
    Capitalized,
    /// `ПРИВЕТ` (two or more letters).
    Upper,
    /// `ПРивет`: two initial capitals, the rest lowercase (at least three letters).
    TwoInitialCaps,
    /// `пРИВЕТ`: typed with Caps Lock and Shift.
    InvertedCapitalized,
    /// Anything else, e.g. `iPhone`.
    Mixed,
    /// No letters.
    NoLetters,
}

/// Classifies the capitalization of the letters of `word`.
pub fn case_pattern(word: &str) -> CasePattern {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    let Some((&first, rest)) = letters.split_first() else {
        return CasePattern::NoLetters;
    };
    let upper = |c: &char| c.is_uppercase();
    let lower = |c: &char| !c.is_uppercase();
    if letters.iter().all(lower) {
        CasePattern::Lower
    } else if letters.len() >= 2 && letters.iter().all(upper) {
        CasePattern::Upper
    } else if first.is_uppercase() && rest.iter().all(lower) {
        CasePattern::Capitalized
    } else if letters.len() >= 3
        && first.is_uppercase()
        && letters[1].is_uppercase()
        && letters[2..].iter().all(lower)
    {
        CasePattern::TwoInitialCaps
    } else if !first.is_uppercase() && rest.iter().all(upper) {
        CasePattern::InvertedCapitalized
    } else {
        CasePattern::Mixed
    }
}

/// Words of one letter: Russian `а в и к о с у я`, English `a` and `I`.
pub fn single_letter_word(lang: Lang, c: char) -> bool {
    let c = lm::to_lower(c);
    match lang {
        Lang::Ru => "авикосуя".contains(c),
        Lang::En => c == 'a' || c == 'i',
    }
}

/// Punctuation that may precede a word of the language. English quotes and
/// brackets are Russian letters (`'` = э, `[` = х), so they are only ignored
/// when checking whether the typed word is known ([`Reading::known`]).
fn leading_punct(lang: Lang) -> &'static [char] {
    match lang {
        Lang::En => &[],
        Lang::Ru => &['"', '(', '«'],
    }
}

/// Leading characters ignored when checking whether a typed word is known.
fn lenient_leading_punct(lang: Lang) -> &'static [char] {
    match lang {
        Lang::En => &['"', '\'', '[', '{', '<', '`'],
        Lang::Ru => &['"', '(', '«'],
    }
}

/// Punctuation that may follow a word of the language.
fn trailing_punct(lang: Lang) -> &'static [char] {
    match lang {
        Lang::En => &[',', '.', ';', ':', '\'', '"', '?', '!', ')', ']', '}', '>'],
        Lang::Ru => &[',', '.', ';', ':', '"', '?', '!', ')', '»'],
    }
}

/// How a word looks in one layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// Language of the layout.
    pub lang: Lang,
    /// Full rendering of the typed keys.
    pub text: String,
    /// Rendering without surrounding punctuation.
    pub core: String,
    /// The core consists of letters of the language and inner joiners only.
    pub valid: bool,
    /// Capitalization of the core.
    pub case: CasePattern,
    /// Found in the dictionary.
    pub in_dictionary: bool,
    /// Frequency rank of the lowercase core.
    pub rank: Option<u32>,
    /// Statistical score of the core (of the full text when invalid).
    pub score: WordScore,
    /// The word without leading quotes is a dictionary or frequent word of at
    /// least two letters (`"hello` is still a known English word).
    pub known_lenient: bool,
}

impl Reading {
    fn known(&self) -> bool {
        self.valid && (self.in_dictionary || self.rank.is_some())
    }

    fn implausible(&self) -> bool {
        !self.valid || self.score.impossible > 0
    }
}

/// Full detection result, useful for logging and tuning.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// The word in the layout it was typed in.
    pub current: Reading,
    /// The word in the other layout.
    pub other: Reading,
    /// Decision.
    pub decision: Decision,
}

/// Minimum cost advantage (bits per transition) for a switch to a word that
/// is only in the dictionary.
const DICT_MARGIN: f32 = 0.8;
/// Minimum cost advantage for a switch to a frequent word.
const FREQ_MARGIN: f32 = 0.0;
/// A word of the other language within this rank can displace a rare one.
const DOMINANT_RANK: u32 = 1000;
/// A word as typed is protected until its rank is this bad.
const RARE_RANK: u32 = 3000;
/// Minimum cost advantage for a purely statistical switch.
const STAT_MARGIN: f32 = 2.0;
/// Short words (up to this many letters) need a frequent target word.
const SHORT_WORD: u32 = 3;
/// Frequency rank a short target word must be within.
const SHORT_RANK: u32 = 3000;

/// The layout detector.
#[derive(Debug, Clone)]
pub struct Detector {
    lexicon: Arc<dyn Lexicon>,
    options: DetectorOptions,
    rules: RuleSet,
    keymaps: [KeyMap; 2],
    #[cfg(feature = "builtin-data")]
    extra_rules: bool,
    #[cfg(feature = "builtin-data")]
    improve_switching: bool,
}

fn lang_index(lang: Lang) -> usize {
    match lang {
        Lang::Ru => 0,
        Lang::En => 1,
    }
}

impl Detector {
    /// Creates a detector with the built-in RU/EN key maps.
    pub fn new(lexicon: Arc<dyn Lexicon>, options: DetectorOptions, rules: RuleSet) -> Self {
        Self {
            lexicon,
            options,
            rules,
            #[cfg(feature = "builtin-data")]
            extra_rules: false,
            #[cfg(feature = "builtin-data")]
            improve_switching: false,
            keymaps: [
                layouts::builtin_keymap(Lang::Ru).clone(),
                layouts::builtin_keymap(Lang::En).clone(),
            ],
        }
    }

    /// Creates a detector with the built-in data and settings from `config`.
    #[cfg(feature = "builtin-data")]
    pub fn from_config(config: &Config) -> Self {
        let mut detector = Self::new(
            Arc::new(BuiltinLexicon),
            DetectorOptions::from(config),
            RuleSet::new(&config.rules),
        );
        detector.extra_rules = config.rules_options.extra_rules;
        detector.improve_switching = config.rules_options.improve_switching;
        detector
    }

    /// Replaces the key map of `lang`, e.g. with the one read from the OS.
    pub fn set_keymap(&mut self, lang: Lang, keymap: KeyMap) {
        self.keymaps[lang_index(lang)] = keymap;
    }

    /// Key map used for `lang`.
    pub fn keymap(&self, lang: Lang) -> &KeyMap {
        &self.keymaps[lang_index(lang)]
    }

    /// Current options.
    pub fn options(&self) -> &DetectorOptions {
        &self.options
    }

    fn read(&self, lang: Lang, keys: &[KeyPress]) -> Reading {
        let text = layouts::render(keys, self.keymap(lang));
        let core = text
            .trim_start_matches(leading_punct(lang))
            .trim_end_matches(trailing_punct(lang))
            .to_string();
        let valid = self.is_valid_core(lang, &core);
        let case = case_pattern(&core);
        let model = self.lexicon.model(lang);
        let lenient = core.trim_start_matches(lenient_leading_punct(lang));
        let known_lenient = lenient.chars().filter(|&c| lm::is_letter(lang, c)).count() >= 2
            && self.is_valid_core(lang, lenient)
            && self.is_known(lang, lenient);
        if !valid {
            return Reading {
                lang,
                score: model.score(&text),
                text,
                core,
                valid,
                case,
                in_dictionary: false,
                rank: None,
                known_lenient,
            };
        }
        let lower = core.to_lowercase();
        Reading {
            lang,
            rank: model.rank(&lower),
            score: model.score(&core),
            in_dictionary: self.in_dictionary(lang, &core),
            text,
            core,
            valid,
            case,
            known_lenient,
        }
    }

    fn is_valid_core(&self, lang: Lang, core: &str) -> bool {
        !core.is_empty()
            && core
                .chars()
                .all(|c| lm::is_letter(lang, c) || c == '-' || (c == '\'' && lang == Lang::En))
            && !core.starts_with(lm::is_joiner)
            && !core.ends_with(lm::is_joiner)
    }

    fn in_dictionary(&self, lang: Lang, core: &str) -> bool {
        let case = case_pattern(core);
        let lower = core.to_lowercase();
        let checked = match case {
            CasePattern::TwoInitialCaps | CasePattern::InvertedCapitalized => {
                let mut chars = lower.chars();
                chars
                    .next()
                    .map(|f| f.to_uppercase().chain(chars).collect::<String>())
                    .unwrap_or_default()
            }
            _ => core.to_string(),
        };
        self.lexicon.is_word(lang, &checked)
            || (checked != lower
                && case != CasePattern::Mixed
                && self.lexicon.is_word(lang, &lower))
    }

    /// A dictionary abbreviation: known in capitals but not as a lowercase
    /// word. `ISO` and `ФСБ` qualify, `ЕДЫ` (capitals of «еды») does not.
    fn is_abbreviation(&self, lang: Lang, core: &str) -> bool {
        let upper = core.to_uppercase();
        self.lexicon.is_word(lang, &upper) && !self.lexicon.is_word(lang, &core.to_lowercase())
    }

    fn is_known(&self, lang: Lang, core: &str) -> bool {
        self.lexicon
            .model(lang)
            .rank(&core.to_lowercase())
            .is_some()
            || self.in_dictionary(lang, core)
    }

    /// Decides what to do with a word typed as `keys` in layout `current`.
    pub fn decide(&self, keys: &[KeyPress], current: Lang) -> Decision {
        self.analyze(keys, current, None).decision
    }

    /// Like [`decide`](Self::decide), with both readings for diagnostics.
    ///
    /// `context` is the language of the previous word on the same line, if
    /// known. It also gates the optional additional short-word corrections.
    pub fn analyze(&self, keys: &[KeyPress], current: Lang, context: Option<Lang>) -> Analysis {
        self.analyze_with_previous_word(keys, current, context, context)
    }

    /// Supplies the immediately preceding token separately from the recent
    /// language context used by legacy one-letter rules (e.g. English `plan b`).
    pub fn analyze_with_previous_word(
        &self,
        keys: &[KeyPress],
        current: Lang,
        context: Option<Lang>,
        previous_word: Option<Lang>,
    ) -> Analysis {
        #[cfg(not(feature = "builtin-data"))]
        let _ = previous_word;
        let cur = self.read(current, keys);
        let alt = self.read(current.other(), keys);
        let decision = self.judge(&cur, &alt, context);
        #[cfg(feature = "builtin-data")]
        let decision = self.improve(&cur, &alt, previous_word, decision);
        Analysis {
            current: cur,
            other: alt,
            decision,
        }
    }

    #[cfg(feature = "builtin-data")]
    fn improve(
        &self,
        cur: &Reading,
        alt: &Reading,
        context: Option<Lang>,
        decision: Decision,
    ) -> Decision {
        use crate::short_words;
        // This layer only adds corrections. Explicit decisions and filters
        // retain their priority, including user Stay rules and built-in vetoes.
        if !self.improve_switching
            || cur.lang != Lang::Ru
            || context != Some(Lang::Ru)
            || matches!(
                decision,
                Decision::Switch { .. }
                    | Decision::Stay(
                        StayReason::Rule
                            | StayReason::ExtraRule
                            | StayReason::TooShort
                            | StayReason::Digits
                            | StayReason::MixedCase
                            | StayReason::Abbreviation
                    )
            )
            || !cur.valid
            || cur.case != CasePattern::Lower
            || !alt.valid
            || alt.case != CasePattern::Lower
            || alt.text != alt.core
            || !short_words::contains(&alt.text)
        {
            return decision;
        }
        if !short_words::is_russian_label(&cur.core) {
            // Title case protects names and their inflections even when the
            // user types lowercase: уфы, луи, рур, вуд. Uppercase-only entries
            // are not sufficient: they may be technical abbreviations.
            let mut chars = cur.core.chars();
            let title: String = chars
                .next()
                .into_iter()
                .flat_map(char::to_uppercase)
                .chain(chars)
                .collect();
            if cur.known()
                || cur.known_lenient
                || short_words::is_russian_name(&cur.core)
                || self.lexicon.is_word(Lang::Ru, &title)
            {
                return decision;
            }
        }
        Self::switch(alt, SwitchReason::ImprovedSwitching)
    }

    /// One-letter words: `z` → `я`, `ш` → `i`, but `я` and `a` stay.
    fn judge_single_letter(cur: &Reading, alt: &Reading, context: Option<Lang>) -> Decision {
        let is_word = |r: &Reading| {
            let mut chars = r.core.chars();
            r.valid
                && chars.next().is_some_and(|c| single_letter_word(r.lang, c))
                && chars.next().is_none()
        };
        if is_word(cur) {
            return Decision::Stay(StayReason::KnownWord);
        }
        if !is_word(alt) || alt.text.chars().count() != 1 {
            return Decision::Stay(StayReason::TooShort);
        }
        // Stray Latin letters are common in English text ("plan b", "vitamin c").
        if cur.lang == Lang::En && context == Some(Lang::En) {
            return Decision::Stay(StayReason::NotConfident);
        }
        Self::switch(alt, SwitchReason::Dictionary)
    }

    fn switch(alt: &Reading, reason: SwitchReason) -> Decision {
        Decision::Switch {
            to: alt.lang,
            text: alt.text.clone(),
            reason,
        }
    }

    fn rare_word_yields(&self, cur: &Reading, alt: &Reading) -> bool {
        alt.valid
            && alt.score.letters >= self.options.min_word_len.max(2)
            && alt.in_dictionary
            && alt.case != CasePattern::Mixed
            && cur.rank.is_none_or(|own| own >= RARE_RANK)
            && alt.score.letters >= cur.score.letters
            && alt.rank.is_some_and(|other| other < DOMINANT_RANK)
    }

    fn judge(&self, cur: &Reading, alt: &Reading, context: Option<Lang>) -> Decision {
        let letters = cur.score.letters.max(alt.score.letters);
        if letters == 0 {
            return Decision::Stay(StayReason::TooShort);
        }
        if cur.text.chars().any(|c| c.is_ascii_digit()) {
            return Decision::Stay(StayReason::Digits);
        }

        match self.rules.check(&[&cur.core, &alt.core]) {
            Some(RuleAction::Stay) => return Decision::Stay(StayReason::Rule),
            Some(RuleAction::Switch) => return Self::switch(alt, SwitchReason::Rule),
            None => {}
        }

        if letters == 1 && self.options.min_word_len <= 1 {
            return Self::judge_single_letter(cur, alt, context);
        }
        if letters < self.options.min_word_len.max(1) {
            return Decision::Stay(StayReason::TooShort);
        }

        if self.options.password_heuristic && cur.valid && cur.case == CasePattern::Mixed {
            return Decision::Stay(StayReason::MixedCase);
        }
        if !self.options.fix_abbreviations && cur.case == CasePattern::Upper {
            return Decision::Stay(StayReason::Abbreviation);
        }
        #[cfg(feature = "builtin-data")]
        if self.extra_rules {
            use crate::extra_rules::{self, RuleVerdict};
            let db = extra_rules::builtin();
            let current = db.find(&cur.text);
            let alternative = db.find(&alt.text);
            // Extra rules provide evidence but do not bypass lexical checks:
            // an exception protects a known word or an all-capitals token.
            // Broad exceptions on unknown gibberish (e.g. туув → need) are
            // diagnostic evidence only and must not suppress our dictionary.
            // Keep the established rare-word arbitration too: dictionary «шт»
            // must still yield to the very frequent English word `in`.
            let protected = (cur.known() || cur.known_lenient || cur.case == CasePattern::Upper)
                && !self.rare_word_yields(cur, alt);
            match extra_rules::resolve(current, alternative, false) {
                RuleVerdict::Stay if protected => return Decision::Stay(StayReason::ExtraRule),
                // Only add corrections of unknown input to an independently
                // known complete reading. Keep existing ambiguous-word policy.
                RuleVerdict::Switch
                    if !cur.known()
                        && !cur.known_lenient
                        && alt.known()
                        && alt.valid
                        && alt.case != CasePattern::Mixed
                        && alt.score.letters >= cur.score.letters
                        && (cur.case != CasePattern::Upper
                            || self.is_abbreviation(alt.lang, &alt.core)) =>
                {
                    return Self::switch(alt, SwitchReason::ExtraRule);
                }
                _ => {}
            }
        }
        if cur.known() || cur.known_lenient {
            // Both readings are words, so frequency decides. A word the user
            // hardly ever types yields to a very common word of the other
            // language: `ye` is the 4665th English word, «ну» the 167th
            // Russian one, and `j,` is a letter with a comma against «об».
            // A word the user does type often is never displaced, however
            // common the other reading: «рук» (1986) keeps its place next to
            // `her` (33), and so do «руки», «руку» and «ем». The other reading
            // must also cover the whole word, or «шею» would yield to `it.`.
            if !self.rare_word_yields(cur, alt) {
                return Decision::Stay(StayReason::KnownWord);
            }
            return Self::switch(alt, SwitchReason::Dictionary);
        }
        if !alt.valid || (self.options.password_heuristic && alt.case == CasePattern::Mixed) {
            return if cur.implausible() && letters >= 3 && !alt.known() {
                Decision::Suspicious
            } else {
                Decision::Stay(StayReason::NotConfident)
            };
        }

        let eager = self.options.sensitivity.clamp(-1.0, 1.0);
        let advantage = cur.score.cost - alt.score.cost;
        if alt.known() && alt.score.letters >= self.options.min_word_len.max(2) {
            let base = if alt.rank.is_some() {
                FREQ_MARGIN
            } else {
                DICT_MARGIN
            };
            let margin = base - eager * 0.5;
            // Nothing protects the typed reading: it is neither a dictionary
            // word nor a frequent one. A real word of the other language then
            // outweighs letter statistics, which are weak for short words.
            let nothing_typed = !cur.in_dictionary && cur.rank.is_none();
            let other_is_word = alt.in_dictionary && alt.score.impossible == 0;
            // A word in capitals is an abbreviation, and neither trigram costs
            // nor word frequencies describe abbreviations: ШЫЩ and ISO are
            // equally unusual letter sequences. Only a dictionary abbreviation
            // of the other language is evidence, and it may well be unusual
            // itself (ФСБ, ГОСТ), so impossible trigrams do not count for it.
            // Capitals of an ordinary word (ЕДЫ for TLS) prove nothing and are
            // left to the general rules below. Two letters are skipped: state
            // and ministry codes collide across the layouts (ТВ and ND).
            let typed_abbreviation = cur.case == CasePattern::Upper;
            let other_abbreviation = typed_abbreviation
                && nothing_typed
                && letters >= 3
                && alt.score.letters >= 3
                && self.is_abbreviation(alt.lang, &alt.core);
            // Letter statistics. Two or three letters leave one or two
            // transitions, so a frequent target word is required as well.
            let statistics = if letters <= SHORT_WORD {
                let rank_limit = (SHORT_RANK as f32 * (1.0 + eager)).max(100.0) as u32;
                alt.rank.is_some_and(|r| r < rank_limit)
                    && (cur.implausible() || advantage > margin)
            } else {
                cur.implausible() || advantage > margin
            };
            // A dictionary word against a string that is no word at all. This
            // carries short words the statistics cannot judge (сфз → cap,
            // ker → лук, шв → id). The evidence must cover the whole word:
            // «СПб» must not yield to the two letters of `CG,`. Capitals of an
            // ordinary word are trusted from four letters on, because shorter
            // ones collide across the layouts (TLS and «ЕДЫ»).
            let other_word_wins = nothing_typed
                && other_is_word
                && alt.score.letters >= cur.score.letters
                && (!typed_abbreviation || letters >= 4);
            if other_abbreviation || statistics || other_word_wins {
                return Self::switch(alt, SwitchReason::Dictionary);
            }
            return Decision::Stay(StayReason::NotConfident);
        }

        if letters >= 4
            && cur.implausible()
            && alt.score.impossible == 0
            && alt.score.has_vowel
            && advantage > STAT_MARGIN * (1.0 - eager * 0.5)
        {
            return Self::switch(alt, SwitchReason::Statistics);
        }
        if cur.implausible() && letters >= 3 {
            return Decision::Suspicious;
        }
        Decision::Stay(StayReason::NotConfident)
    }
}

#[cfg(test)]
#[cfg(feature = "builtin-data")]
mod tests {
    use super::*;

    fn detector() -> Detector {
        Detector::new(
            Arc::new(BuiltinLexicon),
            DetectorOptions::default(),
            RuleSet::default(),
        )
    }

    /// Keys of `text` typed in the layout of `typed_in`.
    fn keys(text: &str, typed_in: Lang) -> Vec<KeyPress> {
        layouts::keys_for_text(text, layouts::builtin_keymap(typed_in)).unwrap()
    }

    /// Decision for `intended` (a word of `lang`) typed while layout `current` was active.
    fn typed(intended: &str, lang: Lang, current: Lang) -> Decision {
        detector().decide(&keys(intended, lang), current)
    }

    #[test]
    fn case_patterns() {
        assert_eq!(case_pattern("привет"), CasePattern::Lower);
        assert_eq!(case_pattern("Привет"), CasePattern::Capitalized);
        assert_eq!(case_pattern("ПРИВЕТ"), CasePattern::Upper);
        assert_eq!(case_pattern("ПРивет"), CasePattern::TwoInitialCaps);
        assert_eq!(case_pattern("пРИВЕТ"), CasePattern::InvertedCapitalized);
        assert_eq!(case_pattern("iPhone"), CasePattern::Mixed);
        assert_eq!(case_pattern("I"), CasePattern::Capitalized);
        assert_eq!(case_pattern("из-за"), CasePattern::Lower);
        assert_eq!(case_pattern("--"), CasePattern::NoLetters);
    }

    #[test]
    fn switches_wrong_layout_words() {
        for word in [
            "привет",
            "Привет",
            "ПРИВЕТ",
            "хорошо",
            "объявление",
            "жизнь",
            "не",
            "мы",
            "из-за",
            "ёлка",
        ] {
            let d = typed(word, Lang::Ru, Lang::En);
            assert_eq!(d.switch_to(), Some(Lang::Ru), "{word}: {d:?}");
        }
        for word in ["hello", "Hello", "world", "keyboard", "the", "don't", "is"] {
            let d = typed(word, Lang::En, Lang::Ru);
            assert_eq!(d.switch_to(), Some(Lang::En), "{word}: {d:?}");
        }
    }

    /// Abbreviations have no frequency rank and unusual letter statistics, so
    /// only the dictionary can tell «ШЫЩ» from «ISO».
    #[test]
    fn switches_abbreviations() {
        for word in [
            "ISO", "USB", "HTTP", "PDF", "API", "FAQ", "USA", "NASA", "CPU",
        ] {
            let d = typed(word, Lang::En, Lang::Ru);
            assert_eq!(d.switch_to(), Some(Lang::En), "{word}: {d:?}");
        }
        for word in ["МГУ", "НДС", "ФСБ", "ГОСТ", "ЗАГС", "СМИ", "ООН", "АЭС"]
        {
            let d = typed(word, Lang::Ru, Lang::En);
            assert_eq!(d.switch_to(), Some(Lang::Ru), "{word}: {d:?}");
        }
    }

    /// Capitals of an ordinary word are not an abbreviation of the other
    /// language, and two-letter codes collide across the layouts.
    #[test]
    fn keeps_correctly_typed_abbreviations() {
        for word in ["ISO", "USB", "HTTP", "PNG", "CEO", "GPS", "TV", "PC", "OK"] {
            let d = typed(word, Lang::En, Lang::En);
            assert_eq!(d.switch_to(), None, "{word}: {d:?}");
        }
        for word in ["ТВ", "ПК", "ЕС", "РФ", "ТСЖ", "МЧС", "ЖКХ", "ИНН", "МВД"]
        {
            let d = typed(word, Lang::Ru, Lang::Ru);
            assert_eq!(d.switch_to(), None, "{word}: {d:?}");
        }
    }

    /// Two or three letters give the trigram model almost no signal, so a
    /// dictionary word of the other language decides even without a rank.
    #[test]
    fn switches_short_words_outside_the_frequent_list() {
        for word in ["cap", "web", "fox", "lab", "flu", "jar", "sip", "id"] {
            let d = typed(word, Lang::En, Lang::Ru);
            assert_eq!(d.switch_to(), Some(Lang::En), "{word}: {d:?}");
        }
        for word in ["лук", "шаг", "зуб", "ухо", "газ", "дым", "еду", "ту"] {
            let d = typed(word, Lang::Ru, Lang::En);
            assert_eq!(d.switch_to(), Some(Lang::Ru), "{word}: {d:?}");
        }
    }

    /// Both readings are words the user types often, so the typed one wins.
    #[test]
    fn keeps_words_that_read_as_words_in_both_layouts() {
        for word in ["ну", "ем", "рук", "руку", "руки", "во", "об"] {
            let d = typed(word, Lang::Ru, Lang::Ru);
            assert_eq!(d.switch_to(), None, "{word}: {d:?}");
        }
        for word in ["her", "here", "tv", "die", "key"] {
            let d = typed(word, Lang::En, Lang::En);
            assert_eq!(d.switch_to(), None, "{word}: {d:?}");
        }
    }

    /// A word the user hardly ever types yields to a very common word of the
    /// other language. `ye` is the 4665th English word and «ну» the 167th
    /// Russian one, so typing «ну» in the English layout is corrected.
    #[test]
    fn rare_words_yield_to_very_common_words_of_the_other_language() {
        for word in ["ну", "об", "во", "уж", "всю", "еще"] {
            let d = typed(word, Lang::Ru, Lang::En);
            assert_eq!(d.switch_to(), Some(Lang::Ru), "{word}: {d:?}");
        }
        // «душ» (1845) and «внук» (6456) are not common enough to displace
        // the English words `lei` and `dyer`, and stay as typed.
        for word in ["душ", "внук"] {
            let d = typed(word, Lang::Ru, Lang::En);
            assert_eq!(d.switch_to(), None, "{word}: {d:?}");
        }
        // The price: `herb` is rarer than `ye` in the corpus, so it yields
        // too. A user rule «herb, совпадать, не переключать» restores it.
        let d = typed("herb", Lang::En, Lang::En);
        assert_eq!(d.switch_to(), Some(Lang::Ru), "herb: {d:?}");
    }

    #[test]
    fn keeps_correct_words() {
        for word in ["привет", "не", "мы", "хорошо", "Москва", "из-за", "в"]
        {
            let d = typed(word, Lang::Ru, Lang::Ru);
            assert!(d.switch_to().is_none(), "{word}: {d:?}");
        }
        // `vs` is a rare English word but `мы` is one of the most frequent
        // Russian words, so `vs` is converted. `here` is frequent in English
        // and stays; `herb` is not and now yields to «руки», see
        // `rare_words_yield_to_very_common_words_of_the_other_language`.
        for word in ["hello", "iPhone", "the", "a", "don't", "Tom", "here"] {
            let d = typed(word, Lang::En, Lang::En);
            assert!(d.switch_to().is_none(), "{word}: {d:?}");
        }
    }

    #[test]
    fn switch_result_text_and_punctuation() {
        let d = typed("привет", Lang::Ru, Lang::En);
        assert_eq!(
            d,
            Decision::Switch {
                to: Lang::Ru,
                text: "привет".to_string(),
                reason: SwitchReason::Dictionary
            }
        );
        let en_keys = keys("hello,", Lang::En);
        assert!(detector().decide(&en_keys, Lang::En).switch_to().is_none());
        let d = detector().decide(&en_keys, Lang::Ru);
        assert_eq!(d.switch_to(), Some(Lang::En), "{d:?}");
    }

    #[test]
    fn single_letters() {
        let det = detector();
        for (letter, lang) in [
            ("я", Lang::Ru),
            ("в", Lang::Ru),
            ("Я", Lang::Ru),
            ("i", Lang::En),
            ("I", Lang::En),
            ("a", Lang::En),
        ] {
            let k = keys(letter, lang);
            assert_eq!(
                det.decide(&k, lang),
                Decision::Stay(StayReason::KnownWord),
                "{letter} typed correctly"
            );
            let d = det.decide(&k, lang.other());
            assert_eq!(
                d.switch_to(),
                Some(lang),
                "{letter} typed in the other layout: {d:?}"
            );
        }
        assert_eq!(
            det.decide(&keys("z", Lang::En), Lang::En),
            Decision::Switch {
                to: Lang::Ru,
                text: "я".into(),
                reason: SwitchReason::Dictionary
            }
        );
        assert_eq!(
            det.decide(&keys("ш", Lang::Ru), Lang::Ru).switch_to(),
            Some(Lang::En)
        );
        assert_eq!(
            det.decide(&keys("x", Lang::En), Lang::En),
            Decision::Stay(StayReason::TooShort)
        );
        assert_eq!(
            det.analyze(&keys("b", Lang::En), Lang::En, Some(Lang::En))
                .decision,
            Decision::Stay(StayReason::NotConfident)
        );
        assert_eq!(
            det.analyze(&keys("b", Lang::En), Lang::En, Some(Lang::Ru))
                .decision
                .switch_to(),
            Some(Lang::Ru)
        );
        assert_eq!(
            det.analyze(&keys("ш", Lang::Ru), Lang::Ru, Some(Lang::Ru))
                .decision
                .switch_to(),
            Some(Lang::En)
        );

        let two = Detector::new(
            Arc::new(BuiltinLexicon),
            DetectorOptions {
                min_word_len: 2,
                ..DetectorOptions::default()
            },
            RuleSet::default(),
        );
        assert_eq!(
            two.decide(&keys("z", Lang::En), Lang::En),
            Decision::Stay(StayReason::TooShort)
        );
    }

    #[test]
    fn filters() {
        let det = detector();
        assert_eq!(
            det.decide(&keys("", Lang::En), Lang::En),
            Decision::Stay(StayReason::TooShort)
        );
        assert_eq!(
            det.decide(&keys("b2b", Lang::En), Lang::Ru),
            Decision::Stay(StayReason::Digits)
        );
        assert_eq!(
            det.decide(&keys("gHbDtN", Lang::En), Lang::En),
            Decision::Stay(StayReason::MixedCase)
        );
    }

    #[test]
    fn rules_override() {
        let rules = RuleSet::new(&[crate::config::Rule {
            pattern: "ghbdtn".into(),
            match_kind: crate::config::MatchKind::Equals,
            case_sensitive: false,
            action: RuleAction::Stay,
            comment: String::new(),
        }]);
        let det = Detector::new(Arc::new(BuiltinLexicon), DetectorOptions::default(), rules);
        assert_eq!(
            det.decide(&keys("привет", Lang::Ru), Lang::En),
            Decision::Stay(StayReason::Rule)
        );
    }

    #[test]
    fn abbreviations_option() {
        let options = DetectorOptions {
            fix_abbreviations: false,
            ..DetectorOptions::default()
        };
        let det = Detector::new(Arc::new(BuiltinLexicon), options, RuleSet::default());
        assert_eq!(
            det.decide(&keys("ПРИВЕТ", Lang::Ru), Lang::En),
            Decision::Stay(StayReason::Abbreviation)
        );
    }
}
