//! Whether a misspelled typed word may be fixed without asking, and with
//! which of the dictionary's suggestions.
//!
//! Hunspell orders suggestions by its own similarity, not by how likely the
//! user meant them, so its first suggestion is not taken on trust. A
//! suggestion is applied only when it explains the word as a small typing
//! slip — a neighbouring key, two swapped letters, a missing, extra or
//! doubled letter — and is clearly better than every other suggestion.
//! Thresholds are tuned on generated typos of held-out Tatoeba words
//! (`tests/typo_eval.rs`).

use crate::keys::PhysKey;
use crate::lang::Lang;
use crate::layouts::builtin_keymap;
use crate::lm;

/// Decision thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Policy {
    /// Shorter words are too ambiguous to change silently.
    pub min_letters: usize,
    /// Largest typing cost of an applied correction.
    pub max_cost: f32,
    /// Required lead of the best suggestion over the next one.
    pub min_margin: f32,
    /// Added to a suggestion outside the frequent words, scaled down for
    /// frequent ones: common words are the more likely intent.
    pub rarity: f32,
}

/// The thresholds used by the program.
pub const POLICY: Policy = Policy {
    min_letters: 4,
    max_cost: 0.6,
    min_margin: 0.5,
    rarity: 0.8,
};

/// Cost of a substitution by a neighbouring key, a swap of adjacent letters
/// and an extra or missing copy of a neighbouring letter.
const SLIP: f32 = 0.6;
/// Cost of any other substitution, insertion or deletion.
const EDIT: f32 = 1.0;
/// Cost of typing е for ё.
const YO: f32 = 0.3;
/// Length of the frequency list of the language models.
const RANKED: f32 = 20_000.0;

/// Position of a key on a standard keyboard, in key widths.
fn key_position(key: PhysKey) -> Option<(f32, f32)> {
    let (code, extended) = key.win_scancode();
    if extended {
        return None;
    }
    let code = f32::from(code);
    match code as u16 {
        0x02..=0x0D => Some((code - 2.0 - 0.5, 0.0)),
        0x29 => Some((-1.5, 0.0)),
        0x10..=0x1B => Some((code - 16.0, 1.0)),
        0x1E..=0x28 => Some((code - 30.0 + 0.25, 2.0)),
        0x2C..=0x35 => Some((code - 44.0 + 0.75, 3.0)),
        _ => None,
    }
}

/// Keys pressed by mistake instead of each other: horizontally or
/// diagonally adjacent on the keyboard.
fn keys_adjacent(a: PhysKey, b: PhysKey) -> bool {
    match (key_position(a), key_position(b)) {
        (Some((ax, ay)), Some((bx, by))) => {
            let (dx, dy) = (ax - bx, ay - by);
            a != b && dx * dx + dy * dy < 1.6
        }
        _ => false,
    }
}

/// Whether two letters of `lang` are typed with neighbouring keys.
pub fn letters_adjacent(lang: Lang, a: char, b: char) -> bool {
    let map = builtin_keymap(lang);
    match (map.find(lm::to_lower(a)), map.find(lm::to_lower(b))) {
        (Some((ka, _)), Some((kb, _))) => keys_adjacent(ka, kb),
        _ => false,
    }
}

/// Letters typed with a key next to the one of `c`.
pub fn neighbours(lang: Lang, c: char) -> Vec<char> {
    let map = builtin_keymap(lang);
    let Some((key, _)) = map.find(lm::to_lower(c)) else {
        return Vec::new();
    };
    map.iter()
        .filter(|&(other, normal, _)| keys_adjacent(key, other) && normal.is_some())
        .filter_map(|(_, normal, _)| normal)
        .filter(|&n| lm::is_letter(lang, n))
        .collect()
}

fn substitution(lang: Lang, typed: char, intended: char) -> f32 {
    if typed == intended {
        0.0
    } else if (typed, intended) == ('е', 'ё') {
        YO
    } else if letters_adjacent(lang, typed, intended) {
        SLIP
    } else {
        EDIT
    }
}

/// An extra letter: a repeated neighbour or a neighbouring key pressed too.
fn insertion(lang: Lang, extra: char, before: Option<char>, after: Option<char>) -> f32 {
    let near =
        |other: Option<char>| other.is_some_and(|o| o == extra || letters_adjacent(lang, o, extra));
    if near(before) || near(after) {
        SLIP
    } else {
        EDIT
    }
}

/// A missing letter; losing one of a doubled pair is the usual slip.
fn deletion(missing: char, before: Option<char>, after: Option<char>) -> f32 {
    if before == Some(missing) || after == Some(missing) {
        SLIP
    } else {
        EDIT
    }
}

/// How costly a typing slip turns `intended` into `typed`: a weighted
/// Damerau–Levenshtein distance over lowercase letters.
pub fn typing_cost(typed: &str, intended: &str, lang: Lang) -> f32 {
    let t: Vec<char> = typed.chars().map(lm::to_lower).collect();
    let i: Vec<char> = intended.chars().map(lm::to_lower).collect();
    let (n, m) = (t.len(), i.len());
    // d[a][b]: cost of typing t[..a] when meaning i[..b].
    let mut d = vec![vec![0.0f32; m + 1]; n + 1];
    for a in 1..=n {
        d[a][0] = d[a - 1][0]
            + insertion(
                lang,
                t[a - 1],
                a.checked_sub(2).map(|k| t[k]),
                t.get(a).copied(),
            );
    }
    for b in 1..=m {
        d[0][b] =
            d[0][b - 1] + deletion(i[b - 1], b.checked_sub(2).map(|k| i[k]), i.get(b).copied());
    }
    for a in 1..=n {
        for b in 1..=m {
            let mut best = d[a - 1][b - 1] + substitution(lang, t[a - 1], i[b - 1]);
            best = best.min(
                d[a - 1][b]
                    + insertion(
                        lang,
                        t[a - 1],
                        a.checked_sub(2).map(|k| t[k]),
                        t.get(a).copied(),
                    ),
            );
            best = best.min(
                d[a][b - 1] + deletion(i[b - 1], b.checked_sub(2).map(|k| i[k]), i.get(b).copied()),
            );
            if a > 1 && b > 1 && t[a - 1] == i[b - 2] && t[a - 2] == i[b - 1] {
                best = best.min(d[a - 2][b - 2] + SLIP);
            }
            d[a][b] = best;
        }
    }
    d[n][m]
}

/// Words the user may have written this way on purpose: any capital letter
/// marks a name, an abbreviation or an identifier, which a dictionary often
/// lacks (`Колумб`, `NASA`, `getValue`).
fn has_capitals(word: &str) -> bool {
    word.chars().any(char::is_uppercase)
}

/// `typed` is the British spelling of the American `suggestion` (`centre`,
/// `travelled`, `realise`, `colour`, `fulfil`): a choice, not a typo. The
/// built-in dictionary is American, so only this direction occurs.
fn british_spelling(typed: &str, suggestion: &str) -> bool {
    let typed = typed.to_lowercase();
    let suggestion = suggestion.to_lowercase();
    if suggestion == format!("{typed}l") && (typed.ends_with("il") || typed.ends_with("ol")) {
        return true;
    }
    let mut w = typed;
    for (british, american) in [
        ("our", "or"),
        ("yse", "yze"),
        ("ise", "ize"),
        ("isi", "izi"),
        ("isa", "iza"),
        ("lled", "led"),
        ("lling", "ling"),
        ("ller", "ler"),
        ("ogue", "og"),
        ("oe", "e"),
        ("ae", "e"),
        ("ghurt", "gurt"),
    ] {
        w = w.replace(british, american);
    }
    for (british, american) in [("tre", "ter"), ("tres", "ters"), ("tred", "tered")] {
        if let Some(stem) = w.strip_suffix(british) {
            w = format!("{stem}{american}");
        }
    }
    w == suggestion
}

/// A suggestion scored for one typed word.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    /// The suggestion as Hunspell spells it.
    pub word: String,
    /// Typing cost from the suggestion to the typed word.
    pub cost: f32,
    /// Cost plus the rarity penalty; lower is better.
    pub score: f32,
}

/// Scores the usable suggestions of `word`, best first. Suggestions of
/// several words, of another script or differing only in case are dropped.
pub fn score_suggestions(
    word: &str,
    suggestions: &[String],
    lang: Lang,
    policy: &Policy,
    rank: impl Fn(&str) -> Option<u32>,
) -> Vec<Scored> {
    let lower = word.to_lowercase();
    let mut scored: Vec<Scored> = suggestions
        .iter()
        .filter(|s| {
            s.chars()
                .all(|c| lm::is_letter(lang, c) || lm::is_joiner(c))
                && s.to_lowercase() != lower
        })
        .map(|s| {
            let cost = typing_cost(word, s, lang);
            let rarity = match rank(&s.to_lowercase()) {
                Some(position) => policy.rarity * (position as f32 / RANKED).min(1.0),
                None => policy.rarity,
            };
            Scored {
                word: s.clone(),
                cost,
                score: cost + rarity,
            }
        })
        .collect();
    scored.sort_by(|a, b| a.score.total_cmp(&b.score));
    scored
}

/// The correction to apply without asking, or `None` to leave the word.
pub fn automatic_correction(
    word: &str,
    suggestions: &[String],
    lang: Lang,
    policy: &Policy,
    rank: impl Fn(&str) -> Option<u32>,
) -> Option<String> {
    if word.chars().filter(|c| c.is_alphabetic()).count() < policy.min_letters || has_capitals(word)
    {
        return None;
    }
    let scored = score_suggestions(word, suggestions, lang, policy, rank);
    let best = scored.first()?;
    if best.cost > policy.max_cost || (lang == Lang::En && british_spelling(word, &best.word)) {
        return None;
    }
    if let Some(second) = scored.get(1)
        && second.score - best.score < policy.min_margin
    {
        return None;
    }
    Some(best.word.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slips_cost_less_than_other_edits() {
        let en = |typed, intended| typing_cost(typed, intended, Lang::En);
        let ru = |typed, intended| typing_cost(typed, intended, Lang::Ru);
        assert_eq!(en("world", "world"), 0.0);
        assert_eq!(en("wprld", "world"), SLIP, "o and p are neighbours");
        assert_eq!(en("wzrld", "world"), EDIT);
        assert_eq!(en("wrold", "world"), SLIP, "swapped letters");
        assert_eq!(en("speling", "spelling"), SLIP, "one of a double lost");
        assert_eq!(en("worrld", "world"), SLIP, "a letter doubled");
        assert_eq!(ru("паботает", "работает"), SLIP, "р and п are neighbours");
        assert_eq!(ru("ещe", "ещё").min(ru("еще", "ещё")), YO);
        assert_eq!(en("World", "world"), 0.0, "case is ignored");
    }

    #[test]
    fn neighbours_follow_the_layout() {
        let q = neighbours(Lang::En, 'g');
        for c in ['f', 'h', 't', 'y', 'v', 'b'] {
            assert!(q.contains(&c), "{c} next to g: {q:?}");
        }
        assert!(!q.contains(&'p'));
        assert!(neighbours(Lang::Ru, 'п').contains(&'р'));
    }

    /// Frequency ranks of a few words, as the language models give them.
    fn rank(word: &str) -> Option<u32> {
        match word {
            "world" => Some(300),
            "fast" => Some(800),
            "spelling" => Some(3000),
            "fist" => Some(5000),
            _ => None,
        }
    }

    #[test]
    fn a_clear_slip_is_fixed_and_doubt_is_left_alone() {
        let fix = |word: &str, suggestions: &[&str]| {
            let suggestions: Vec<String> = suggestions.iter().map(|s| s.to_string()).collect();
            automatic_correction(word, &suggestions, Lang::En, &POLICY, rank)
        };
        assert_eq!(fix("wrold", &["world", "wold"]).as_deref(), Some("world"));
        assert_eq!(fix("wrold", &["wold", "world"]).as_deref(), Some("world"));
        assert_eq!(
            fix("speling", &["spieling", "spelling"]).as_deref(),
            Some("spelling")
        );
        // Two plausible slips.
        assert_eq!(fix("fost", &["fist", "fast", "foist"]), None);
        assert_eq!(fix("cst", &["cat", "cut"]), None, "too short");
        // Nothing close enough.
        assert_eq!(fix("xqzv", &["quiz"]), None);
        // Names, abbreviations and identifiers are often missing from a dictionary.
        assert_eq!(fix("Wrold", &["World"]), None);
        assert_eq!(fix("NASS", &["NASA"]), None);
        assert_eq!(fix("getValeu", &["getValue"]), None);
        // British spelling is a choice.
        assert_eq!(fix("centre", &["center"]), None);
        assert_eq!(fix("travelled", &["traveled"]), None);
        assert_eq!(fix("realise", &["realize"]), None);
        assert_eq!(fix("fulfil", &["fulfill"]), None);
        assert_eq!(fix("colour", &["color"]), None);
        // Case-only differences and split words are not offered silently.
        assert_eq!(fix("london", &["London"]), None);
        assert_eq!(fix("helloworld", &["hello world"]), None);
    }

    #[test]
    fn frequent_words_win_close_calls() {
        let suggestions = vec!["thin".to_string(), "then".to_string()];
        let rank = |w: &str| match w {
            "then" => Some(40),
            _ => None,
        };
        let scored = score_suggestions("thwn", &suggestions, Lang::En, &POLICY, rank);
        assert_eq!(scored[0].word, "then");
    }
}
