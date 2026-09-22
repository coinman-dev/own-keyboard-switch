//! User configuration (`config.toml`).
//!
//! The schema covers the application's user-facing settings. All sections use
//! `#[serde(default)]`, so a partial file is valid and missing keys take the
//! defaults below.

mod hotkeys;
mod load;
mod sanitize;

pub use hotkeys::{HotkeyAction, HotkeyBinding, Hotkeys};
pub use load::{
    ConfigError, ConfigIssue, LoadOutcome, from_toml_str, load_or_create, save, to_toml_string,
};

use crate::keys::PhysKey;
use crate::lang::Lang;
use serde::{Deserialize, Serialize};

/// Version of the configuration schema written by this build.
pub const CURRENT_VERSION: u32 = 3;

/// Complete user configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Schema version, used for migrations.
    pub version: u32,
    /// Общие → Основные.
    pub general: General,
    /// Общие → Дополнительные.
    pub advanced: Advanced,
    /// Общие → блок «Переключение раскладки».
    pub switching: Switching,
    /// Горячие клавиши.
    pub hotkeys: Hotkeys,
    /// Правила переключения: общие опции.
    pub rules_options: RulesOptions,
    /// Правила переключения: пользовательские правила.
    pub rules: Vec<Rule>,
    /// Программы-исключения.
    pub exclusions: Exclusions,
    /// Устранение проблем.
    pub troubleshooting: Troubleshooting,
    /// Автозамена.
    pub autoreplace: AutoReplace,
    /// Звуки.
    pub sounds: Sounds,
    /// Буфер обмена.
    pub clipboard: Clipboard,
    /// Проверка орфографии.
    pub spellcheck: Spellcheck,
    /// Linux-specific options.
    pub linux: Linux,
    /// Logging.
    pub log: Log,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            general: General::default(),
            advanced: Advanced::default(),
            switching: Switching::default(),
            hotkeys: Hotkeys::default(),
            rules_options: RulesOptions::default(),
            rules: Vec::new(),
            exclusions: Exclusions::default(),
            troubleshooting: Troubleshooting::default(),
            autoreplace: AutoReplace::default(),
            sounds: Sounds::default(),
            clipboard: Clipboard::default(),
            spellcheck: Spellcheck::default(),
            linux: Linux::default(),
            log: Log::default(),
        }
    }
}

/// Settings window colour theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the OS setting.
    #[default]
    System,
    /// Light theme.
    Light,
    /// Dark theme.
    Dark,
}

/// Interface language preference, kept separate from the detected system language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiLanguage {
    /// Use the current user's system interface language.
    #[default]
    System,
    /// Always use Russian.
    Ru,
    /// Always use English.
    En,
}

impl UiLanguage {
    /// Resolves the preference without changing the value stored in config.toml.
    pub const fn resolve(self, system_language: Lang) -> Lang {
        match self {
            Self::System => system_language,
            Self::Ru => Lang::Ru,
            Self::En => Lang::En,
        }
    }
}

impl From<Lang> for UiLanguage {
    fn from(lang: Lang) -> Self {
        match lang {
            Lang::Ru => Self::Ru,
            Lang::En => Self::En,
        }
    }
}

/// Общие → Основные.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// «Запускаться при старте».
    pub autostart: bool,
    /// «Автопереключение».
    pub autoswitch: bool,
    /// «Запускать с правами Администратора» (Windows only).
    pub run_elevated: bool,
    /// «Показывать плавающий индикатор».
    pub floating_indicator: bool,
    /// «Скрывать плавающий индикатор после смены раскладки».
    pub floating_indicator_autohide: bool,
    /// How long the floating indicator stays visible after a layout change.
    pub floating_indicator_autohide_ms: u32,
    /// Saved position of the floating indicator, in screen pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floating_indicator_pos: Option<[i32; 2]>,
    /// «Закрепить индикатор».
    pub floating_indicator_locked: bool,
    /// «Сделать значок в виде флагов стран».
    pub tray_flags: bool,
    /// «Всегда показывать флаги в полную яркость».
    pub tray_flags_full_brightness: bool,
    /// Flag chosen for a layout: locale (`en-US`) → flag code (`gb`).
    /// Layouts without an entry use the flag of their region.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub layout_flags: std::collections::BTreeMap<String, String>,
    /// «Отключать горячие клавиши при отключении автопереключения».
    pub hotkeys_off_when_autoswitch_off: bool,
    /// Layout pair the detector switches between.
    pub language_pair: [Lang; 2],
    /// Interface language.
    pub ui_language: UiLanguage,
    /// Settings window theme.
    pub theme: Theme,
}

impl Default for General {
    fn default() -> Self {
        Self {
            autostart: true,
            autoswitch: true,
            run_elevated: false,
            floating_indicator: false,
            floating_indicator_autohide: false,
            floating_indicator_autohide_ms: 2000,
            floating_indicator_pos: None,
            floating_indicator_locked: false,
            tray_flags: false,
            tray_flags_full_brightness: false,
            layout_flags: std::collections::BTreeMap::new(),
            hotkeys_off_when_autoswitch_off: false,
            language_pair: [Lang::Ru, Lang::En],
            ui_language: UiLanguage::System,
            theme: Theme::System,
        }
    }
}

/// Общие → Дополнительные.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Advanced {
    /// «Исправлять аббревиатуры».
    pub fix_abbreviations: bool,
    /// «Исправлять две заглавные буквы в начале слова».
    pub fix_two_capitals: bool,
    /// «Исправлять случайное нажатие Caps Lock».
    pub fix_accidental_capslock: bool,
    /// «Отключить кнопку Caps Lock».
    pub disable_capslock: bool,
    /// «Использовать Scroll Lock как Caps Lock».
    pub scrolllock_as_capslock: bool,
    /// «Исправлять раскладку при работе с меню, содержащим горячие клавиши» (Windows only).
    pub fix_layout_in_menus: bool,
    /// «Показывать окно с результатами конвертации буфера».
    pub show_clipboard_conversion_window: bool,
    /// «Следить за буфером обмена».
    pub clipboard_history: bool,
    /// «Сохранять историю буфера обмена после перезагрузки».
    pub clipboard_history_persist: bool,
    /// «Показывать всплывающие подсказки».
    pub show_tooltips: bool,
    /// «Запятая по двойному нажатию клавиши Пробел».
    pub double_space_comma: bool,
}

impl Default for Advanced {
    fn default() -> Self {
        Self {
            fix_abbreviations: true,
            fix_two_capitals: true,
            fix_accidental_capslock: true,
            disable_capslock: false,
            scrolllock_as_capslock: false,
            fix_layout_in_menus: false,
            show_clipboard_conversion_window: true,
            clipboard_history: true,
            clipboard_history_persist: false,
            show_tooltips: true,
            double_space_comma: false,
        }
    }
}

/// A single key that switches the layout on a short press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchKey {
    /// Disabled.
    #[default]
    None,
    /// Right Ctrl.
    RightCtrl,
    /// Left Ctrl.
    LeftCtrl,
    /// Right Shift.
    RightShift,
    /// Left Shift.
    LeftShift,
    /// Caps Lock.
    CapsLock,
    /// Left Alt.
    LeftAlt,
    /// Right Alt.
    RightAlt,
    /// Space.
    Space,
}

impl SwitchKey {
    /// The physical key, or `None` when disabled.
    pub const fn phys_key(self) -> Option<PhysKey> {
        match self {
            SwitchKey::None => None,
            SwitchKey::RightCtrl => Some(PhysKey::ControlRight),
            SwitchKey::LeftCtrl => Some(PhysKey::ControlLeft),
            SwitchKey::RightShift => Some(PhysKey::ShiftRight),
            SwitchKey::LeftShift => Some(PhysKey::ShiftLeft),
            SwitchKey::CapsLock => Some(PhysKey::CapsLock),
            SwitchKey::LeftAlt => Some(PhysKey::AltLeft),
            SwitchKey::RightAlt => Some(PhysKey::AltRight),
            SwitchKey::Space => Some(PhysKey::Space),
        }
    }
}

/// «Дополнительно переключать по: левый Shift — рус., правый Shift — англ.».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DirectKeys {
    /// Whether direct selection keys are active.
    pub enabled: bool,
    /// Key that selects the Russian layout.
    pub ru: SwitchKey,
    /// Key that selects the English layout.
    pub en: SwitchKey,
}

impl Default for DirectKeys {
    fn default() -> Self {
        Self {
            enabled: false,
            ru: SwitchKey::LeftShift,
            en: SwitchKey::RightShift,
        }
    }
}

/// Общие → блок «Переключение раскладки».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Switching {
    /// «Переключать по:».
    pub switch_key: SwitchKey,
    /// A press longer than this is treated as holding a modifier, not a switch.
    pub switch_key_max_hold_ms: u32,
    /// «Только русский/английский».
    pub switch_key_only_pair: bool,
    /// «Единая раскладка».
    pub single_layout_for_all_windows: bool,
    /// «Дополнительно переключать по:».
    pub direct_keys: DirectKeys,
    /// Pause between switching the layout and retyping a word.
    pub layout_switch_delay_ms: u32,
    /// Pause between injected key events.
    pub inject_key_delay_ms: u32,
}

impl Default for Switching {
    fn default() -> Self {
        Self {
            switch_key: SwitchKey::None,
            switch_key_max_hold_ms: 200,
            switch_key_only_pair: false,
            single_layout_for_all_windows: false,
            direct_keys: DirectKeys::default(),
            layout_switch_delay_ms: if cfg!(windows) { 30 } else { 80 },
            inject_key_delay_ms: if cfg!(windows) { 0 } else { 2 },
        }
    }
}

/// Правила переключения: общие опции.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RulesOptions {
    /// «Предлагать добавить правило после N отмен подряд»; `0` disables the suggestion.
    pub suggest_rule_after_cancels: u32,
    /// Extra rules that complement dictionaries and language statistics.
    #[serde(alias = "builtin_rules")]
    pub extra_rules: bool,
    /// Recognise additional lowercase English words after a Russian token.
    pub improve_switching: bool,
}

impl Default for RulesOptions {
    fn default() -> Self {
        Self {
            suggest_rule_after_cancels: 2,
            extra_rules: true,
            improve_switching: false,
        }
    }
}

/// How a rule pattern is matched against a word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    /// «Содержать данное сочетание букв».
    #[default]
    Contains,
    /// «Начинаться с данных букв».
    StartsWith,
    /// «Совпадать с данным сочетанием».
    Equals,
}

/// What to do with a word that matches a rule.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    /// Не переводить в другую раскладку.
    #[default]
    Stay,
    /// Переводить в другую раскладку.
    Switch,
}

/// A user switching rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    /// Word or letter combination.
    pub pattern: String,
    /// Matching mode.
    #[serde(rename = "match")]
    pub match_kind: MatchKind,
    /// «Учитывать регистр».
    pub case_sensitive: bool,
    /// Action for matching words.
    pub action: RuleAction,
    /// Free-form note shown in the settings window.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            match_kind: MatchKind::Contains,
            case_sensitive: false,
            action: RuleAction::Stay,
            comment: String::new(),
        }
    }
}

/// «По файлу приложения».
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutableExclusion {
    /// Full path to the executable (Linux: path or application id).
    pub path: String,
}

/// «По заголовку окна».
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TitleExclusion {
    /// Case-sensitive substring of the window title.
    pub contains: String,
}

/// «По папке с программами».
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderExclusion {
    /// Folder whose programs are excluded.
    pub path: String,
}

/// Программы-исключения.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Exclusions {
    /// By executable.
    pub executables: Vec<ExecutableExclusion>,
    /// By window title.
    pub titles: Vec<TitleExclusion>,
    /// By folder.
    pub folders: Vec<FolderExclusion>,
}

/// «Не переключать раскладку, если перед вводом были нажаты».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoSwitchAfter {
    /// Backspace.
    pub backspace: bool,
    /// Стрелка влево.
    pub arrow_left: bool,
    /// Стрелка вправо.
    pub arrow_right: bool,
    /// Стрелка вверх.
    pub arrow_up: bool,
    /// Стрелка вниз.
    pub arrow_down: bool,
    /// Delete.
    pub delete: bool,
    /// «Вы сменили раскладку».
    pub manual_layout_change: bool,
}

impl Default for NoSwitchAfter {
    fn default() -> Self {
        Self {
            backspace: true,
            arrow_left: false,
            arrow_right: false,
            arrow_up: false,
            arrow_down: false,
            delete: false,
            manual_layout_change: true,
        }
    }
}

/// Tuning of the layout detector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectorOptions {
    /// Sensitivity shift of the statistical threshold, `-1.0..=1.0`.
    pub sensitivity: f32,
    /// Shorter words are never converted automatically; `1` also converts
    /// one-letter words (`z` → `я`).
    pub min_word_len: u32,
    /// Do not convert words that look like passwords.
    pub password_heuristic: bool,
}

impl Default for DetectorOptions {
    fn default() -> Self {
        Self {
            sensitivity: 0.0,
            min_word_len: 1,
            password_heuristic: true,
        }
    }
}

/// Устранение проблем.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Troubleshooting {
    /// «Не переключать раскладку, если перед вводом были нажаты».
    pub no_switch_after: NoSwitchAfter,
    /// «Учитывать ввод только в русской и английской раскладках».
    pub only_pair_layouts: bool,
    /// «Не переключать раскладку по клавишам Tab и Enter».
    pub no_switch_on_tab_enter: bool,
    /// «Не взаимодействовать с программами-исключениями».
    pub ignore_excluded_apps_completely: bool,
    /// Detector tuning.
    pub detector: DetectorOptions,
}

impl Default for Troubleshooting {
    fn default() -> Self {
        Self {
            no_switch_after: NoSwitchAfter::default(),
            only_pair_layouts: true,
            no_switch_on_tab_enter: false,
            ignore_excluded_apps_completely: false,
            detector: DetectorOptions::default(),
        }
    }
}

/// When an abbreviation is expanded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoReplaceTrigger {
    /// Show a tooltip, accept with Enter or Tab.
    #[default]
    Tooltip,
    /// Replace on Space.
    Space,
    /// Replace on Enter.
    Enter,
    /// Replace on Tab.
    Tab,
    /// Replace only by the «Показать меню вставки автозамены» hotkey.
    Hotkey,
}

/// One autoreplace entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoReplaceItem {
    /// «Что заменять».
    pub from: String,
    /// «На что заменять».
    pub to: String,
    /// «Запоминать позицию курсора»: character index in `to`, `-1` = end of text.
    pub cursor_pos: i32,
}

impl Default for AutoReplaceItem {
    fn default() -> Self {
        Self {
            from: String::new(),
            to: String::new(),
            cursor_pos: -1,
        }
    }
}

/// Автозамена.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoReplace {
    /// Master switch.
    pub enabled: bool,
    /// «Заменять по».
    pub trigger: AutoReplaceTrigger,
    /// «Заменять при наборе в другой раскладке».
    pub also_in_other_layout: bool,
    /// «Показывать список в меню».
    pub show_in_tray_menu: bool,
    /// Opacity of the autoreplace list window, `0.1..=1.0`.
    pub list_opacity: f32,
    /// Entries.
    pub items: Vec<AutoReplaceItem>,
}

impl Default for AutoReplace {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger: AutoReplaceTrigger::Tooltip,
            also_in_other_layout: true,
            show_in_tray_menu: true,
            list_opacity: 1.0,
            items: Vec::new(),
        }
    }
}

/// «Проиграть звуковой файл» / «Сигнал динамика системного блока».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundMode {
    /// Play a WAV file.
    #[default]
    File,
    /// Short beep.
    Beep,
}

/// Sound settings of one event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundEvent {
    /// Whether the event makes a sound.
    pub enabled: bool,
    /// Custom WAV file; empty = built-in sound.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub file: String,
}

impl SoundEvent {
    const fn on() -> Self {
        Self {
            enabled: true,
            file: String::new(),
        }
    }

    const fn off() -> Self {
        Self {
            enabled: false,
            file: String::new(),
        }
    }
}

impl Default for SoundEvent {
    fn default() -> Self {
        Self::on()
    }
}

/// Events that can make a sound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundEvents {
    /// Automatic layout switch.
    pub autoswitch: SoundEvent,
    /// Manual conversion by hotkey.
    pub manual_convert: SoundEvent,
    /// Layout changed by the user.
    pub layout_changed: SoundEvent,
    /// Conversion cancelled.
    pub cancel: SoundEvent,
    /// Possible typo (indicator turns red).
    pub suspicious: SoundEvent,
    /// Autoreplace performed.
    pub autoreplace: SoundEvent,
    /// Caps Lock or double capital corrected.
    pub case_fixed: SoundEvent,
    /// Clipboard converted.
    pub clipboard_convert: SoundEvent,
    /// Conversion impossible.
    pub error: SoundEvent,
}

impl Default for SoundEvents {
    fn default() -> Self {
        Self {
            autoswitch: SoundEvent::on(),
            manual_convert: SoundEvent::on(),
            layout_changed: SoundEvent::off(),
            cancel: SoundEvent::on(),
            suspicious: SoundEvent::off(),
            autoreplace: SoundEvent::on(),
            case_fixed: SoundEvent::on(),
            clipboard_convert: SoundEvent::on(),
            error: SoundEvent::on(),
        }
    }
}

/// Звуки.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sounds {
    /// «Звуковые эффекты».
    pub enabled: bool,
    /// Playback mode.
    pub mode: SoundMode,
    /// Per-event settings.
    pub events: SoundEvents,
}

impl Default for Sounds {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: SoundMode::File,
            events: SoundEvents::default(),
        }
    }
}

/// Буфер обмена.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clipboard {
    /// Number of remembered clipboard texts.
    pub history_size: u32,
    /// Polling interval where the OS has no change notifications.
    pub poll_interval_ms: u32,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self {
            history_size: 30,
            poll_interval_ms: 500,
        }
    }
}

/// Проверка орфографии.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spellcheck {
    /// Master switch.
    pub enabled: bool,
    /// Check a finished word after layout switching and autoreplace have run.
    pub check_typed_words: bool,
    /// A spellcheck command uses selected text before falling back to clipboard text.
    pub prefer_selection: bool,
    /// Dictionaries to use.
    pub languages: Vec<Lang>,
    /// Suggestions shown per misspelled word.
    pub max_suggestions: u32,
    /// Show a window with suggestions instead of fixing the clipboard silently.
    pub show_result_window: bool,
}

impl Default for Spellcheck {
    fn default() -> Self {
        Self {
            enabled: true,
            check_typed_words: false,
            prefer_selection: true,
            languages: vec![Lang::Ru, Lang::En],
            max_suggestions: 5,
            show_result_window: true,
        }
    }
}

/// How the current layout is read and changed on Linux.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutBackend {
    /// Detect from the session.
    #[default]
    Auto,
    /// KDE Plasma D-Bus (`org.kde.keyboard`).
    Kde,
    /// X11 XKB extension.
    X11,
    /// Bundled GNOME Shell extension.
    GnomeExtension,
    /// GNOME without the extension: gsettings + injected switch shortcut.
    GnomeFallback,
    /// Internal counter synchronised with observed switch shortcuts.
    Internal,
}

/// Linux-specific options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Linux {
    /// Layout backend.
    pub layout_backend: LayoutBackend,
    /// Shortcut injected on GNOME without the extension; `auto` reads it from gsettings.
    pub gnome_switch_hotkey: String,
    /// Whether the bundled GNOME Shell extension was installed by the program.
    pub gnome_extension_installed: bool,
    /// Input devices to read; empty = all keyboards.
    pub devices: Vec<String>,
}

impl Default for Linux {
    fn default() -> Self {
        Self {
            layout_backend: LayoutBackend::Auto,
            gnome_switch_hotkey: "auto".to_string(),
            gnome_extension_installed: false,
            devices: Vec::new(),
        }
    }
}

/// Log verbosity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Errors only.
    Error,
    /// Warnings and errors.
    Warn,
    /// Normal operation.
    #[default]
    Info,
    /// Detailed diagnostics.
    Debug,
    /// Everything.
    Trace,
}

/// Logging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Log {
    /// «Диагностика (подробный журнал)»: raises the verbosity to `debug`
    /// whatever `level` says, for a problem report.
    pub debug: bool,
    /// Verbosity used while `debug` is off.
    pub level: LogLevel,
    /// Number of daily log files to keep.
    pub keep_files: u32,
}

impl Default for Log {
    fn default() -> Self {
        Self {
            debug: false,
            level: LogLevel::Info,
            keep_files: 7,
        }
    }
}

impl Log {
    /// Verbosity the program actually logs at.
    #[must_use]
    pub fn effective_level(&self) -> LogLevel {
        if self.debug {
            self.level.max(LogLevel::Debug)
        } else {
            self.level
        }
    }
}
