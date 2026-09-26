//! «Экспорт...» and «Импорт...» of switching rules.
//!
//! The file uses the `[[rules]]` tables of `config.toml`, so a whole
//! configuration file can be imported as well.

use super::{ConfigError, Rule};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

const HEADER: &str = "\
# Own Keyboard Switch — правила переключения.
# Загрузить: «Настройки → Правила переключения → Импорт...».

";

/// Rules read from a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedRules {
    /// Valid rules in file order.
    pub rules: Vec<Rule>,
    /// Entries that were not rules or had an empty pattern.
    pub skipped: usize,
}

/// How imported rules changed the list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// New rules appended to the list.
    pub added: usize,
    /// Existing rules whose action or comment changed.
    pub updated: usize,
    /// Rules that were already in the list.
    pub unchanged: usize,
}

#[derive(Serialize)]
struct RulesFile<'a> {
    rules: &'a [Rule],
}

/// Text of an exported rules file.
pub fn rules_to_toml(rules: &[Rule]) -> Result<String, ConfigError> {
    Ok(format!(
        "{HEADER}{}",
        toml::to_string_pretty(&RulesFile { rules })?
    ))
}

/// Reads rules exported by [`rules_to_toml`] or taken from `config.toml`.
/// A malformed entry is skipped; the error is for text without any rules list.
pub fn rules_from_toml(text: &str) -> Result<ImportedRules, String> {
    let table = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .parse::<Table>()
        .map_err(|err| err.to_string().trim().to_string())?;
    let Some(Value::Array(items)) = table.get("rules") else {
        return Err("the file has no [[rules]] list".into());
    };
    let mut imported = ImportedRules::default();
    for item in items {
        match Rule::deserialize(item.clone()) {
            Ok(mut rule) if !rule.pattern.trim().is_empty() => {
                rule.pattern = rule.pattern.trim().to_string();
                imported.rules.push(rule);
            }
            _ => imported.skipped += 1,
        }
    }
    Ok(imported)
}

/// Whether two rules describe the same letters under the same condition.
fn same_pattern(a: &Rule, b: &Rule) -> bool {
    a.match_kind == b.match_kind
        && a.case_sensitive == b.case_sensitive
        && if a.case_sensitive {
            a.pattern == b.pattern
        } else {
            a.pattern.to_lowercase() == b.pattern.to_lowercase()
        }
}

/// Adds `imported` to `rules`. A rule for the same letters and condition
/// takes the imported action instead of being listed twice.
pub fn merge_rules(rules: &mut Vec<Rule>, imported: Vec<Rule>) -> MergeReport {
    let mut report = MergeReport::default();
    for rule in imported {
        match rules
            .iter_mut()
            .find(|existing| same_pattern(existing, &rule))
        {
            Some(existing) if *existing == rule => report.unchanged += 1,
            Some(existing) => {
                *existing = rule;
                report.updated += 1;
            }
            None => {
                rules.push(rule);
                report.added += 1;
            }
        }
    }
    report
}
