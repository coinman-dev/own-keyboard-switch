//! User switching rules («Правила переключения»).

use crate::config::{MatchKind, Rule, RuleAction};

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompiledRule {
    pattern: String,
    kind: MatchKind,
    case_sensitive: bool,
    action: RuleAction,
}

impl CompiledRule {
    fn matches(&self, word: &str) -> bool {
        let lowered;
        let word = if self.case_sensitive {
            word
        } else {
            lowered = word.to_lowercase();
            &lowered
        };
        match self.kind {
            MatchKind::Contains => word.contains(&self.pattern),
            MatchKind::StartsWith => word.starts_with(&self.pattern),
            MatchKind::Equals => word == self.pattern,
        }
    }
}

/// Rules ready for matching, in configuration order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleSet {
    rules: Vec<CompiledRule>,
}

impl RuleSet {
    /// Compiles configuration rules; empty patterns are ignored.
    pub fn new(rules: &[Rule]) -> Self {
        Self {
            rules: rules
                .iter()
                .filter(|r| !r.pattern.trim().is_empty())
                .map(|r| CompiledRule {
                    pattern: if r.case_sensitive {
                        r.pattern.trim().to_string()
                    } else {
                        r.pattern.trim().to_lowercase()
                    },
                    kind: r.match_kind,
                    case_sensitive: r.case_sensitive,
                    action: r.action,
                })
                .collect(),
        }
    }

    /// Number of rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether there are no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Action of the first rule matching any of `renderings` (the word as it
    /// looks in each layout).
    pub fn check(&self, renderings: &[&str]) -> Option<RuleAction> {
        self.rules
            .iter()
            .find(|rule| renderings.iter().any(|w| rule.matches(w)))
            .map(|rule| rule.action)
    }
}

const RU_ENDINGS: &[&str] = &[
    "ями", "ами", "ого", "его", "ому", "ему", "ыми", "ими", "ешь", "ишь", "ете", "ите", "ют", "ят",
    "ую", "юю", "ая", "яя", "ое", "ее", "ой", "ей", "ый", "ий", "ом", "ем", "ах", "ях", "ов", "ев",
    "ть", "ла", "ло", "ли", "а", "я", "о", "е", "ы", "и", "у", "ю", "ь",
];
const EN_ENDINGS: &[&str] = &["ing", "ies", "ed", "es", "er", "ly", "s"];

/// Rule proposed after the user cancelled the conversion of `word` several
/// times («Предлагать добавить правило после N отмен подряд»).
///
/// Short words get an exact rule; longer words get a «starts with» rule on
/// their stem, which keeps a learned correction useful for inflected forms.
pub fn suggest_rule(word: &str, action: RuleAction) -> Rule {
    let word = word.trim().to_lowercase();
    let letters = word.chars().count();
    let (pattern, match_kind) = if letters <= 4 {
        (word, MatchKind::Equals)
    } else {
        let is_cyrillic = word.chars().any(|c| ('а'..='я').contains(&c) || c == 'ё');
        let endings = if is_cyrillic { RU_ENDINGS } else { EN_ENDINGS };
        let stem = endings
            .iter()
            .filter_map(|e| word.strip_suffix(e))
            .find(|stem| stem.chars().count() >= 4)
            .unwrap_or(&word)
            .to_string();
        (stem, MatchKind::StartsWith)
    };
    Rule {
        pattern,
        match_kind,
        case_sensitive: false,
        action,
        comment: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, kind: MatchKind, case_sensitive: bool, action: RuleAction) -> Rule {
        Rule {
            pattern: pattern.to_string(),
            match_kind: kind,
            case_sensitive,
            action,
            comment: String::new(),
        }
    }

    #[test]
    fn matching_modes() {
        let set = RuleSet::new(&[
            rule("vs", MatchKind::Equals, false, RuleAction::Stay),
            rule("ghbd", MatchKind::StartsWith, false, RuleAction::Switch),
            rule("ЁЖ", MatchKind::Contains, true, RuleAction::Stay),
            rule("  ", MatchKind::Contains, false, RuleAction::Switch),
        ]);
        assert_eq!(set.len(), 3);
        assert_eq!(set.check(&["VS", "мы"]), Some(RuleAction::Stay));
        assert_eq!(set.check(&["vsx", "мыч"]), None);
        assert_eq!(set.check(&["Ghbdtn", "Привет"]), Some(RuleAction::Switch));
        assert_eq!(set.check(&["xghbd"]), None);
        assert_eq!(set.check(&["ёжик"]), None);
        assert_eq!(set.check(&["ЁЖИК"]), Some(RuleAction::Stay));
    }

    #[test]
    fn suggested_rules() {
        let r = suggest_rule("Vs", RuleAction::Stay);
        assert_eq!(
            (r.pattern.as_str(), r.match_kind),
            ("vs", MatchKind::Equals)
        );
        let r = suggest_rule("компилятора", RuleAction::Stay);
        assert_eq!(
            (r.pattern.as_str(), r.match_kind),
            ("компилятор", MatchKind::StartsWith)
        );
        let r = suggest_rule("kubectl", RuleAction::Stay);
        assert_eq!(
            (r.pattern.as_str(), r.match_kind),
            ("kubectl", MatchKind::StartsWith)
        );
        let r = suggest_rule("deploying", RuleAction::Switch);
        assert_eq!(r.pattern, "deploy");
        let set = RuleSet::new(&[suggest_rule("компилятора", RuleAction::Stay)]);
        assert_eq!(set.check(&["компилятором"]), Some(RuleAction::Stay));
    }

    #[test]
    fn first_rule_wins() {
        let set = RuleSet::new(&[
            rule("abc", MatchKind::Contains, false, RuleAction::Switch),
            rule("abc", MatchKind::Equals, false, RuleAction::Stay),
        ]);
        assert_eq!(set.check(&["abc"]), Some(RuleAction::Switch));
        assert!(RuleSet::default().is_empty());
    }
}
