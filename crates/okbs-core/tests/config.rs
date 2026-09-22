//! Integration tests of configuration loading, saving and recovery.

use okbs_core::config::{
    self, AutoReplaceTrigger, Config, ConfigIssue, LayoutBackend, MatchKind, RuleAction, SwitchKey,
    UiLanguage,
};
use okbs_core::{Lang, PhysKey};
use pretty_assertions::assert_eq;

#[test]
fn default_roundtrip_is_lossless() {
    let text = config::to_toml_string(&Config::default()).unwrap();
    let outcome = config::from_toml_str(&text);
    assert_eq!(outcome.issues, vec![]);
    assert_eq!(outcome.config, Config::default());
}

#[test]
fn improved_switching_defaults_off_and_roundtrips() {
    assert!(
        !config::from_toml_str("[rules_options]\nextra_rules = true\n")
            .config
            .rules_options
            .improve_switching
    );
    let loaded = config::from_toml_str("[rules_options]\nimprove_switching = true\n");
    assert!(loaded.issues.is_empty());
    assert!(loaded.config.rules_options.improve_switching);
    let saved = config::to_toml_string(&loaded.config).expect("serialize");
    assert!(
        config::from_toml_str(&saved)
            .config
            .rules_options
            .improve_switching
    );
}

#[test]
fn extra_rules_accepts_the_legacy_key_and_saves_the_new_name() {
    let loaded = config::from_toml_str("[rules_options]\nbuiltin_rules = false\n");
    assert!(loaded.issues.is_empty());
    assert!(!loaded.config.rules_options.extra_rules);
    let saved = config::to_toml_string(&loaded.config).unwrap();
    assert!(saved.contains("extra_rules = false"));
    assert!(!saved.contains("builtin_rules"));
}

#[test]
fn ui_language_preferences_resolve_and_roundtrip_without_losing_automatic_mode() {
    assert_eq!(Config::default().general.ui_language, UiLanguage::System);
    for (value, preference) in [
        ("system", UiLanguage::System),
        ("ru", UiLanguage::Ru),
        ("en", UiLanguage::En),
    ] {
        let loaded = config::from_toml_str(&format!("[general]\nui_language = {value:?}\n"));
        assert!(loaded.issues.is_empty());
        assert_eq!(loaded.config.general.ui_language, preference);
        for system in Lang::ALL {
            let expected = match preference {
                UiLanguage::System => system,
                UiLanguage::Ru => Lang::Ru,
                UiLanguage::En => Lang::En,
            };
            assert_eq!(preference.resolve(system), expected);
        }
        let serialized = config::to_toml_string(&loaded.config).unwrap();
        assert!(serialized.contains(&format!("ui_language = {value:?}")));
        assert_eq!(
            config::from_toml_str(&serialized)
                .config
                .general
                .ui_language,
            preference
        );
    }
}

#[test]
fn version_2_keeps_explicit_ui_language_and_other_preferences() {
    for lang in ["ru", "en"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, format!("version = 2\n[general]\nui_language = {lang:?}\nautoswitch = false\n[troubleshooting.detector]\nmin_word_len = 2\n")).unwrap();
        let loaded = config::load_or_create(&path).unwrap();
        assert!(!loaded.config.general.autoswitch);
        assert_eq!(
            loaded.config.general.ui_language,
            if lang == "ru" {
                UiLanguage::Ru
            } else {
                UiLanguage::En
            }
        );
        assert_eq!(loaded.config.troubleshooting.detector.min_word_len, 2);
        assert_eq!(loaded.config.version, config::CURRENT_VERSION);
        assert!(loaded.issues.contains(&ConfigIssue::Migrated {
            from: 2,
            to: config::CURRENT_VERSION
        }));
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains(&format!("ui_language = {lang:?}")));
        assert!(config::from_toml_str(&saved).issues.is_empty());
    }
}

#[test]
fn invalid_ui_language_resets_only_the_language_to_system() {
    let loaded = config::from_toml_str("[general]\nui_language = 'unknown'\nautoswitch = false\n");
    assert_eq!(loaded.config.general.ui_language, UiLanguage::System);
    assert!(!loaded.config.general.autoswitch);
    assert!(loaded.issues.iter().any(|issue| matches!(issue, ConfigIssue::InvalidValue { key, .. } if key == "general.ui_language")));
}

#[test]
fn default_file_mentions_every_section() {
    let text = config::to_toml_string(&Config::default()).unwrap();
    for section in [
        "[general]",
        "[advanced]",
        "[switching]",
        "[switching.direct_keys]",
        "[hotkeys]",
        "[rules_options]",
        "[exclusions]",
        "[troubleshooting.no_switch_after]",
        "[troubleshooting.detector]",
        "[autoreplace]",
        "[sounds.events.autoswitch]",
        "[clipboard]",
        "[spellcheck]",
        "[linux]",
        "[log]",
    ] {
        assert!(text.contains(section), "missing {section} in:\n{text}");
    }
    assert!(text.contains("cancel_or_convert_last_word = \"Break\""));
    assert!(text.contains("number_to_words = \"\""));
}

#[test]
fn empty_file_gives_defaults() {
    let outcome = config::from_toml_str("");
    assert_eq!(outcome.issues, vec![]);
    assert_eq!(outcome.config, Config::default());
}

#[test]
fn partial_file_keeps_other_defaults() {
    let outcome = config::from_toml_str(
        r#"
        [general]
        autoswitch = false

        [switching]
        switch_key = "right_ctrl"

        [hotkeys]
        cancel_or_convert_last_word = "F11"
        toggle_autoswitch = "Ctrl+Win+Alt+K"

        [[rules]]
        pattern = "ghbd"
        match = "starts_with"
        action = "switch"

        [[autoreplace.items]]
        from = "снп"
        to = "С наилучшими пожеланиями"

        [linux]
        layout_backend = "gnome-extension"
        "#,
    );
    assert_eq!(outcome.issues, vec![]);
    let c = outcome.config;
    assert!(!c.general.autoswitch);
    assert!(c.general.autostart);
    assert_eq!(c.switching.switch_key, SwitchKey::RightCtrl);
    assert_eq!(
        c.switching.switch_key.phys_key(),
        Some(PhysKey::ControlRight)
    );
    assert_eq!(c.hotkeys.cancel_or_convert_last_word.to_string(), "F11");
    assert_eq!(c.hotkeys.toggle_autoswitch.to_string(), "Ctrl+Alt+Win+K");
    assert_eq!(c.hotkeys.paste_plain.to_string(), "Ctrl+Alt+V");
    assert_eq!(c.rules.len(), 1);
    assert_eq!(c.rules[0].match_kind, MatchKind::StartsWith);
    assert_eq!(c.rules[0].action, RuleAction::Switch);
    assert!(!c.rules[0].case_sensitive);
    assert_eq!(c.autoreplace.items[0].cursor_pos, -1);
    assert_eq!(c.autoreplace.trigger, AutoReplaceTrigger::Tooltip);
    assert_eq!(c.linux.layout_backend, LayoutBackend::GnomeExtension);
}

#[test]
fn syntax_error_gives_defaults() {
    let outcome = config::from_toml_str("[general\nautoswitch = ");
    assert_eq!(outcome.config, Config::default());
    assert!(matches!(
        outcome.issues.as_slice(),
        [ConfigIssue::SyntaxError { .. }]
    ));
}

#[test]
fn invalid_values_reset_only_their_keys() {
    let outcome = config::from_toml_str(
        r#"
        [general]
        autoswitch = false
        autostart = "yes"
        language_pair = ["ru"]

        [hotkeys]
        paste_plain = "Ctrl+Nope"
        toggle_sounds = "Ctrl+Win+Alt+S"

        [troubleshooting.no_switch_after]
        backspace = false
        delete = 5
        "#,
    );
    let c = &outcome.config;
    assert!(!c.general.autoswitch);
    assert!(c.general.autostart);
    assert_eq!(c.general.language_pair, [Lang::Ru, Lang::En]);
    assert_eq!(c.hotkeys.paste_plain.to_string(), "Ctrl+Alt+V");
    assert_eq!(c.hotkeys.toggle_sounds.to_string(), "Ctrl+Alt+Win+S");
    assert!(!c.troubleshooting.no_switch_after.backspace);
    assert!(!c.troubleshooting.no_switch_after.delete);

    let invalid: Vec<&str> = outcome
        .issues
        .iter()
        .filter_map(|i| match i {
            ConfigIssue::InvalidValue { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        invalid,
        vec![
            "general.autostart",
            "general.language_pair",
            "hotkeys.paste_plain",
            "troubleshooting.no_switch_after.delete",
        ]
    );
}

#[test]
fn bad_list_elements_are_dropped() {
    let outcome = config::from_toml_str(
        r#"
        [[rules]]
        pattern = "one"

        [[rules]]
        pattern = "two"
        match = "sometimes"

        [[rules]]
        pattern = "three"
        action = "switch"

        [spellcheck]
        languages = ["ru", "de", "en"]
        "#,
    );
    let patterns: Vec<&str> = outcome
        .config
        .rules
        .iter()
        .map(|r| r.pattern.as_str())
        .collect();
    assert_eq!(patterns, vec!["one", "three"]);
    assert_eq!(
        outcome.config.spellcheck.languages,
        vec![Lang::Ru, Lang::En]
    );
    let keys: Vec<String> = outcome
        .issues
        .iter()
        .filter(|i| i.is_error())
        .map(|i| match i {
            ConfigIssue::InvalidValue { key, .. } => key.clone(),
            other => other.to_string(),
        })
        .collect();
    assert_eq!(keys, vec!["rules[1]", "spellcheck.languages[1]"]);
}

#[test]
fn unknown_keys_are_reported_not_fatal() {
    let outcome = config::from_toml_str(
        r#"
        future_section = 1
        [general]
        autoswitch = false
        brand_new_option = true
        "#,
    );
    assert!(!outcome.config.general.autoswitch);
    let unknown: Vec<&str> = outcome
        .issues
        .iter()
        .filter_map(|i| match i {
            ConfigIssue::UnknownKey { key } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(unknown, vec!["future_section", "general.brand_new_option"]);
    assert!(!outcome.issues.iter().any(ConfigIssue::is_error));
}

#[test]
fn out_of_range_values_are_clamped() {
    let outcome = config::from_toml_str(
        r#"
        [troubleshooting.detector]
        sensitivity = 7.5
        min_word_len = 0

        [clipboard]
        history_size = 1000

        [autoreplace]
        list_opacity = 0

        [[autoreplace.items]]
        from = "  "
        to = "empty"

        [[autoreplace.items]]
        from = "адр"
        to = "Москва"
        cursor_pos = 42

        [switching.direct_keys]
        enabled = true
        ru = "left_shift"
        en = "left_shift"
        "#,
    );
    let c = &outcome.config;
    assert_eq!(c.troubleshooting.detector.sensitivity, 1.0);
    assert_eq!(c.troubleshooting.detector.min_word_len, 1);
    assert_eq!(c.clipboard.history_size, 100);
    assert_eq!(c.autoreplace.list_opacity, 0.1);
    assert_eq!(c.autoreplace.items.len(), 1);
    assert_eq!(c.autoreplace.items[0].cursor_pos, -1);
    assert!(!c.switching.direct_keys.enabled);
    assert!(!outcome.issues.iter().any(ConfigIssue::is_error));
    assert_eq!(
        outcome
            .issues
            .iter()
            .filter(|i| matches!(i, ConfigIssue::Adjusted { .. }))
            .count(),
        7
    );
}

#[test]
fn duplicate_hotkeys_are_reported() {
    let outcome = config::from_toml_str(
        r#"
        [hotkeys]
        toggle_sounds = "Shift+Break"
        "#,
    );
    assert_eq!(
        outcome.issues,
        vec![ConfigIssue::DuplicateHotkey {
            hotkey: "Shift+Break".to_string(),
            actions: vec!["convert_selection_layout", "toggle_sounds"],
        }]
    );
}

#[test]
fn version_1_is_migrated_and_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[general]\nautoswitch = false\n[troubleshooting.detector]\nmin_word_len = 2\n",
    )
    .unwrap();
    let outcome = config::load_or_create(&path).unwrap();
    assert_eq!(outcome.config.troubleshooting.detector.min_word_len, 1);
    assert!(!outcome.config.general.autoswitch);
    assert!(outcome.issues.contains(&ConfigIssue::Migrated {
        from: 1,
        to: config::CURRENT_VERSION
    }));
    assert!(!outcome.issues.iter().any(ConfigIssue::is_warning));
    let again = config::load_or_create(&path).unwrap();
    assert_eq!(again.issues, vec![]);
    assert_eq!(again.config.troubleshooting.detector.min_word_len, 1);

    let custom =
        config::from_toml_str("version = 1\n[troubleshooting.detector]\nmin_word_len = 3\n");
    assert_eq!(custom.config.troubleshooting.detector.min_word_len, 3);
}

#[test]
fn newer_version_is_reported() {
    let outcome = config::from_toml_str("version = 99\n[general]\nautoswitch = false\n");
    assert!(!outcome.config.general.autoswitch);
    assert!(outcome.issues.contains(&ConfigIssue::NewerVersion {
        found: 99,
        supported: config::CURRENT_VERSION,
    }));
}

#[test]
fn load_or_create_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("config.toml");
    let outcome = config::load_or_create(&path).unwrap();
    assert_eq!(outcome.config, Config::default());
    assert!(matches!(
        outcome.issues.as_slice(),
        [ConfigIssue::CreatedDefault { .. }]
    ));
    assert!(path.exists());

    let again = config::load_or_create(&path).unwrap();
    assert_eq!(again.issues, vec![]);
    assert_eq!(again.config, Config::default());
}

#[test]
fn load_or_create_backs_up_invalid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "[general]\nautoswitch = false\nautostart = 3\n";
    std::fs::write(&path, original).unwrap();

    let outcome = config::load_or_create(&path).unwrap();
    assert!(!outcome.config.general.autoswitch);
    let backup = outcome
        .issues
        .iter()
        .find_map(|i| match i {
            ConfigIssue::BackupCreated { path } => Some(path.clone()),
            _ => None,
        })
        .expect("backup issue");
    assert_eq!(std::fs::read_to_string(backup).unwrap(), original);

    let reloaded = config::load_or_create(&path).unwrap();
    assert_eq!(reloaded.issues, vec![]);
    assert!(!reloaded.config.general.autoswitch);
    assert!(reloaded.config.general.autostart);
}

#[test]
fn load_or_create_handles_non_utf8_and_bom() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, b"\xff\xfe[general]").unwrap();
    let outcome = config::load_or_create(&path).unwrap();
    assert!(matches!(
        outcome.issues.first(),
        Some(ConfigIssue::SyntaxError { .. })
    ));
    assert_eq!(config::load_or_create(&path).unwrap().issues, vec![]);

    std::fs::write(&path, "\u{feff}[general]\nautoswitch = false\n").unwrap();
    let outcome = config::load_or_create(&path).unwrap();
    assert_eq!(outcome.issues, vec![]);
    assert!(!outcome.config.general.autoswitch);
}

#[test]
fn newer_version_file_is_not_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "version = 7\n[general]\nautostart = \"later\"\n";
    std::fs::write(&path, original).unwrap();
    let outcome = config::load_or_create(&path).unwrap();
    assert!(outcome.issues.iter().any(ConfigIssue::is_error));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn save_is_atomic_and_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut cfg = Config::default();
    cfg.general.autoswitch = false;
    cfg.general.floating_indicator_pos = Some([300, 300]);
    cfg.exclusions.titles.push(config::TitleExclusion {
        contains: "Visual Studio Code".to_string(),
    });
    config::save(&path, &cfg).unwrap();
    assert!(!dir.path().join("config.toml.tmp").exists());
    let loaded = config::load_or_create(&path).unwrap();
    assert_eq!(loaded.issues, vec![]);
    assert_eq!(loaded.config, cfg);
}

#[test]
fn logging_is_disabled_by_default_and_exposes_three_ui_levels() {
    use okbs_core::config::{Log, LogLevel};
    let configured = Log {
        enabled: true,
        level: LogLevel::Error,
        keep_files: 7,
    };
    assert!(configured.enabled);
    assert_eq!(
        Log::LEVELS,
        [LogLevel::Error, LogLevel::Info, LogLevel::Debug]
    );
    assert!(!Log::default().enabled, "logging is off by default");
}

/// Configurations written before the option existed keep working.
#[test]
fn a_config_without_the_diagnostics_key_loads_with_it_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[log]\nlevel = \"warn\"\nkeep_files = 3\n").unwrap();
    let loaded = config::load_or_create(&path).unwrap();
    assert!(!loaded.config.log.enabled);
    assert_eq!(loaded.config.log.keep_files, 3);
    assert!(!loaded.issues.iter().any(ConfigIssue::is_error));

    std::fs::write(&path, "[log]\ndebug = true\n").unwrap();
    let loaded = config::load_or_create(&path).unwrap();
    assert!(
        loaded.config.log.enabled,
        "the old debug key enables logging"
    );
    assert_eq!(loaded.config.log.level, config::LogLevel::Info);
}
