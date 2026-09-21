//! Minimal Hunspell affix expansion ("unmunch") for single-character flags.
//!
//! Supports `PFX`/`SFX` groups with conditions and cross products, and the
//! `NEEDAFFIX`/`ONLYINCOMPOUND` flags. This is enough for the LibreOffice
//! `ru_RU` and `en_US` dictionaries, which use neither flag aliases nor
//! continuation classes.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;

#[derive(Debug, Clone)]
enum CharClass {
    Any,
    Set { chars: Vec<char>, negated: bool },
}

impl CharClass {
    fn matches(&self, c: char) -> bool {
        match self {
            CharClass::Any => true,
            CharClass::Set { chars, negated } => chars.contains(&c) != *negated,
        }
    }
}

fn parse_condition(cond: &str) -> Result<Vec<CharClass>> {
    if cond == "." {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    let mut chars = cond.chars();
    while let Some(c) = chars.next() {
        match c {
            '.' => out.push(CharClass::Any),
            '[' => {
                let mut set = Vec::new();
                let mut negated = false;
                let mut first = true;
                loop {
                    let Some(c) = chars.next() else {
                        bail!("unterminated class in condition {cond}");
                    };
                    if c == ']' {
                        break;
                    }
                    if first && c == '^' {
                        negated = true;
                    } else {
                        set.push(c);
                    }
                    first = false;
                }
                out.push(CharClass::Set {
                    chars: set,
                    negated,
                });
            }
            other => out.push(CharClass::Set {
                chars: vec![other],
                negated: false,
            }),
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct AffixRule {
    strip: Vec<char>,
    add: String,
    condition: Vec<CharClass>,
}

#[derive(Debug, Clone, Default)]
struct AffixGroup {
    cross: bool,
    rules: Vec<AffixRule>,
}

/// Parsed affix file.
#[derive(Debug, Default)]
pub struct AffixFile {
    prefixes: HashMap<char, AffixGroup>,
    suffixes: HashMap<char, AffixGroup>,
    need_affix: Option<char>,
    only_in_compound: Option<char>,
}

fn empty_zero(s: &str) -> &str {
    if s == "0" { "" } else { s }
}

impl AffixFile {
    /// Parses an `.aff` file.
    pub fn parse(text: &str) -> Result<Self> {
        let mut aff = AffixFile::default();
        for (n, line) in text.lines().enumerate() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let Some(&keyword) = fields.first() else {
                continue;
            };
            let flag_arg = || -> Result<char> {
                fields
                    .get(1)
                    .and_then(|f| f.chars().next())
                    .with_context(|| format!("line {}: missing flag", n + 1))
            };
            match keyword {
                "FLAG" | "AF" | "AM" => bail!("line {}: {keyword} is not supported", n + 1),
                "NEEDAFFIX" => aff.need_affix = Some(flag_arg()?),
                "ONLYINCOMPOUND" => aff.only_in_compound = Some(flag_arg()?),
                "PFX" | "SFX" => {
                    let flag = flag_arg()?;
                    let groups = if keyword == "PFX" {
                        &mut aff.prefixes
                    } else {
                        &mut aff.suffixes
                    };
                    match fields.len() {
                        4 if !groups.contains_key(&flag) => {
                            groups.insert(
                                flag,
                                AffixGroup {
                                    cross: fields[2] == "Y",
                                    rules: Vec::new(),
                                },
                            );
                        }
                        len if len >= 5 => {
                            let group = groups
                                .get_mut(&flag)
                                .with_context(|| format!("line {}: rule before header", n + 1))?;
                            if fields[3].contains('/') {
                                bail!("line {}: continuation classes are not supported", n + 1);
                            }
                            group.rules.push(AffixRule {
                                strip: empty_zero(fields[2]).chars().collect(),
                                add: empty_zero(fields[3]).to_string(),
                                condition: parse_condition(fields[4])?,
                            });
                        }
                        _ => bail!("line {}: malformed affix line", n + 1),
                    }
                }
                _ => {}
            }
        }
        Ok(aff)
    }

    fn apply_suffix(rule: &AffixRule, word: &[char]) -> Option<String> {
        if word.len() < rule.condition.len() || word.len() < rule.strip.len() {
            return None;
        }
        let tail = &word[word.len() - rule.condition.len()..];
        if !rule
            .condition
            .iter()
            .zip(tail)
            .all(|(cls, &c)| cls.matches(c))
        {
            return None;
        }
        let stem_len = word.len() - rule.strip.len();
        if word[stem_len..] != rule.strip[..] {
            return None;
        }
        let mut out: String = word[..stem_len].iter().collect();
        out.push_str(&rule.add);
        Some(out)
    }

    fn apply_prefix(rule: &AffixRule, word: &[char]) -> Option<String> {
        if word.len() < rule.condition.len() || word.len() < rule.strip.len() {
            return None;
        }
        if !rule
            .condition
            .iter()
            .zip(word)
            .all(|(cls, &c)| cls.matches(c))
        {
            return None;
        }
        if word[..rule.strip.len()] != rule.strip[..] {
            return None;
        }
        let mut out = rule.add.clone();
        out.extend(&word[rule.strip.len()..]);
        Some(out)
    }

    /// Calls `emit` for every word form generated by a `.dic` entry.
    pub fn expand(&self, word: &str, flags: &str, emit: &mut impl FnMut(&str)) {
        let flags: Vec<char> = flags.chars().collect();
        if self.only_in_compound.is_some_and(|f| flags.contains(&f)) {
            return;
        }
        if !self.need_affix.is_some_and(|f| flags.contains(&f)) {
            emit(word);
        }
        let chars: Vec<char> = word.chars().collect();
        let mut cross_suffixed: Vec<Vec<char>> = Vec::new();
        for flag in &flags {
            let Some(group) = self.suffixes.get(flag) else {
                continue;
            };
            for rule in &group.rules {
                if let Some(form) = Self::apply_suffix(rule, &chars) {
                    emit(&form);
                    if group.cross {
                        cross_suffixed.push(form.chars().collect());
                    }
                }
            }
        }
        for flag in &flags {
            let Some(group) = self.prefixes.get(flag) else {
                continue;
            };
            for rule in &group.rules {
                if let Some(form) = Self::apply_prefix(rule, &chars) {
                    emit(&form);
                }
                if group.cross {
                    for suffixed in &cross_suffixed {
                        if let Some(form) = Self::apply_prefix(rule, suffixed) {
                            emit(&form);
                        }
                    }
                }
            }
        }
    }
}

/// Iterates over `(word, flags)` entries of a `.dic` file.
pub fn dic_entries(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().skip(1).filter_map(|line| {
        let entry = line.split_whitespace().next()?;
        Some(entry.split_once('/').unwrap_or((entry, "")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = "\
SET UTF-8
NEEDAFFIX n
SFX Z Y 2
SFX Z   ый   о  [лнртв]ый
SFX Z   0    s  [^s]
PFX R Y 1
PFX R   0    re .
SFX A N 1
SFX A   0    ed .
";

    fn forms(word: &str, flags: &str) -> Vec<String> {
        let aff = AffixFile::parse(AFF).unwrap();
        let mut out = Vec::new();
        aff.expand(word, flags, &mut |w| out.push(w.to_string()));
        out
    }

    #[test]
    fn suffix_with_condition_and_strip() {
        assert_eq!(forms("новый", "Z"), vec!["новый", "ново", "новыйs"]);
        assert_eq!(forms("синий", "Z"), vec!["синий", "синийs"]);
        assert_eq!(forms("bus", "Z"), vec!["bus"]);
    }

    #[test]
    fn prefix_and_cross_product() {
        assert_eq!(forms("do", "RZ"), vec!["do", "dos", "redo", "redos"]);
        assert_eq!(forms("play", "AR"), vec!["play", "played", "replay"]);
    }

    #[test]
    fn need_affix_hides_stem() {
        assert_eq!(forms("cat", "nZ"), vec!["cats"]);
    }

    #[test]
    fn dic_parsing() {
        let dic = "3\nслово/Z\nhello\nfoo/AB\tpo:noun\n";
        let entries: Vec<_> = dic_entries(dic).collect();
        assert_eq!(entries, vec![("слово", "Z"), ("hello", ""), ("foo", "AB")]);
    }
}
