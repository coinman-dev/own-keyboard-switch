//! Range checks and consistency fixes applied after deserialization.

use super::load::ConfigIssue;
use super::{Config, DirectKeys, SwitchKey};
use crate::hotkey::Hotkey;
use crate::lang::Lang;
use std::collections::BTreeMap;

fn clamp_u32(value: &mut u32, min: u32, max: u32, key: &str, issues: &mut Vec<ConfigIssue>) {
    let clamped = (*value).clamp(min, max);
    if clamped != *value {
        issues.push(ConfigIssue::Adjusted {
            key: key.to_string(),
            message: format!("{} is outside {min}..={max}, using {clamped}", *value),
        });
        *value = clamped;
    }
}

fn clamp_f32(
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    key: &str,
    issues: &mut Vec<ConfigIssue>,
) {
    let fixed = if value.is_nan() {
        default
    } else {
        value.clamp(min, max)
    };
    if fixed.to_bits() != value.to_bits() {
        issues.push(ConfigIssue::Adjusted {
            key: key.to_string(),
            message: format!("{} is outside {min}..={max}, using {fixed}", *value),
        });
        *value = fixed;
    }
}

fn retain_counted<T>(
    items: &mut Vec<T>,
    key: &str,
    what: &str,
    keep: impl FnMut(&T) -> bool,
    issues: &mut Vec<ConfigIssue>,
) {
    let before = items.len();
    items.retain(keep);
    let removed = before - items.len();
    if removed > 0 {
        issues.push(ConfigIssue::Adjusted {
            key: key.to_string(),
            message: format!("removed {removed} {what}"),
        });
    }
}

impl Config {
    /// Clamps out-of-range values, removes empty list entries and reports
    /// conflicting hotkeys. Returns what was changed or found.
    pub fn sanitize(&mut self) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();
        let i = &mut issues;

        clamp_u32(
            &mut self.general.floating_indicator_autohide_ms,
            500,
            60_000,
            "general.floating_indicator_autohide_ms",
            i,
        );
        if self.general.language_pair[0] == self.general.language_pair[1] {
            self.general.language_pair = [Lang::Ru, Lang::En];
            i.push(ConfigIssue::Adjusted {
                key: "general.language_pair".to_string(),
                message: "languages must differ, using [\"ru\", \"en\"]".to_string(),
            });
        }

        let s = &mut self.switching;
        clamp_u32(
            &mut s.switch_key_max_hold_ms,
            50,
            2000,
            "switching.switch_key_max_hold_ms",
            i,
        );
        clamp_u32(
            &mut s.layout_switch_delay_ms,
            0,
            2000,
            "switching.layout_switch_delay_ms",
            i,
        );
        clamp_u32(
            &mut s.inject_key_delay_ms,
            0,
            100,
            "switching.inject_key_delay_ms",
            i,
        );
        let d = &s.direct_keys;
        if d.enabled && (d.ru == d.en || d.ru == SwitchKey::None || d.en == SwitchKey::None) {
            s.direct_keys = DirectKeys {
                enabled: false,
                ..d.clone()
            };
            i.push(ConfigIssue::Adjusted {
                key: "switching.direct_keys".to_string(),
                message: "keys must be set and differ; direct keys disabled".to_string(),
            });
        }

        retain_counted(
            &mut self.rules,
            "rules",
            "rules with an empty pattern",
            |r| !r.pattern.trim().is_empty(),
            i,
        );

        let ex = &mut self.exclusions;
        retain_counted(
            &mut ex.executables,
            "exclusions.executables",
            "empty entries",
            |e| !e.path.trim().is_empty(),
            i,
        );
        retain_counted(
            &mut ex.titles,
            "exclusions.titles",
            "empty entries",
            |e| !e.contains.is_empty(),
            i,
        );
        retain_counted(
            &mut ex.folders,
            "exclusions.folders",
            "empty entries",
            |e| !e.path.trim().is_empty(),
            i,
        );

        let det = &mut self.troubleshooting.detector;
        clamp_f32(
            &mut det.sensitivity,
            -1.0,
            1.0,
            0.0,
            "troubleshooting.detector.sensitivity",
            i,
        );
        clamp_u32(
            &mut det.min_word_len,
            1,
            10,
            "troubleshooting.detector.min_word_len",
            i,
        );

        let ar = &mut self.autoreplace;
        clamp_f32(
            &mut ar.list_opacity,
            0.1,
            1.0,
            1.0,
            "autoreplace.list_opacity",
            i,
        );
        retain_counted(
            &mut ar.items,
            "autoreplace.items",
            "entries with an empty abbreviation",
            |it| !it.from.trim().is_empty(),
            i,
        );
        for (index, item) in ar.items.iter_mut().enumerate() {
            let len = i32::try_from(item.to.chars().count()).unwrap_or(i32::MAX);
            if item.cursor_pos < -1 || item.cursor_pos > len {
                i.push(ConfigIssue::Adjusted {
                    key: format!("autoreplace.items[{index}].cursor_pos"),
                    message: format!("{} is outside -1..={len}, using -1", item.cursor_pos),
                });
                item.cursor_pos = -1;
            }
        }

        clamp_u32(
            &mut self.clipboard.history_size,
            1,
            100,
            "clipboard.history_size",
            i,
        );
        clamp_u32(
            &mut self.clipboard.poll_interval_ms,
            100,
            10_000,
            "clipboard.poll_interval_ms",
            i,
        );

        let sp = &mut self.spellcheck;
        clamp_u32(
            &mut sp.max_suggestions,
            1,
            20,
            "spellcheck.max_suggestions",
            i,
        );
        let mut seen = Vec::new();
        sp.languages.retain(|l| {
            let new = !seen.contains(l);
            seen.push(*l);
            new
        });
        if sp.languages.is_empty() {
            sp.languages = vec![Lang::Ru, Lang::En];
            i.push(ConfigIssue::Adjusted {
                key: "spellcheck.languages".to_string(),
                message: "empty list, using [\"ru\", \"en\"]".to_string(),
            });
        }

        clamp_u32(&mut self.log.keep_files, 1, 365, "log.keep_files", i);

        let mut by_hotkey: BTreeMap<String, (Hotkey, Vec<&'static str>)> = BTreeMap::new();
        for (action, binding) in self.hotkeys.iter() {
            if let Some(hotkey) = binding.0 {
                by_hotkey
                    .entry(hotkey.to_string())
                    .or_insert_with(|| (hotkey, Vec::new()))
                    .1
                    .push(action.config_key());
            }
        }
        for (hotkey, (_, actions)) in by_hotkey {
            if actions.len() > 1 {
                i.push(ConfigIssue::DuplicateHotkey { hotkey, actions });
            }
        }

        issues
    }
}
