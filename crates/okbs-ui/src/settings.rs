//! The settings window contents («Настройки»), independent of the windowing code.
//!
//! [`SettingsView`] edits a draft copy of the configuration. OK and «Применить»
//! emit [`SettingsEvent::Apply`]; the application saves the file and passes the
//! configuration to the engine.

use crate::flags::{Flag, flag_for_layout};
use crate::i18n::{Text, hotkey_action_text, tr};
use egui::{Color32, RichText, Ui};
use okbs_core::config::{
    AutoReplaceItem, AutoReplaceTrigger, Config, ExecutableExclusion, FolderExclusion,
    HotkeyAction, HotkeyBinding, MatchKind, Rule, RuleAction, SoundEvent, SoundMode, SwitchKey,
    Theme, TitleExclusion, UiLanguage,
};
use okbs_core::{Hotkey, Lang};

/// Window size in logical points (Windows applies the monitor DPI scale).
pub(crate) const WINDOW_SIZE: [f32; 2] = [800.0, 600.0];

/// Applied once when a GUI context is created, to both light and dark themes.
pub(crate) fn configure_context(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        for font in style.text_styles.values_mut() {
            font.size += 1.0;
        }
        // Settings cells have explicit widths; they must never infer their
        // width from already-wrapped text, as an auto-sized Grid would.
        style.wrap_mode = Some(egui::TextWrapMode::Wrap);
    });
}

/// Sections of the settings window in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// «Общие».
    General,
    /// «Горячие клавиши».
    Hotkeys,
    /// «Правила переключения».
    Rules,
    /// «Программы-исключения».
    Exclusions,
    /// «Устранение проблем».
    Troubleshooting,
    /// «Автозамена».
    Autoreplace,
    /// «Звуки».
    Sounds,
    /// «Проверка орфографии».
    Spellcheck,
}

impl Section {
    /// All sections.
    pub const ALL: [Section; 8] = [
        Section::General,
        Section::Hotkeys,
        Section::Rules,
        Section::Exclusions,
        Section::Troubleshooting,
        Section::Autoreplace,
        Section::Sounds,
        Section::Spellcheck,
    ];

    fn text(self) -> Text {
        match self {
            Section::General => Text::SectionGeneral,
            Section::Hotkeys => Text::SectionHotkeys,
            Section::Rules => Text::SectionRules,
            Section::Exclusions => Text::SectionExclusions,
            Section::Troubleshooting => Text::SectionTroubleshooting,
            Section::Autoreplace => Text::SectionAutoreplace,
            Section::Sounds => Text::SectionSounds,
            Section::Spellcheck => Text::SectionSpellcheck,
        }
    }
}

/// Sound events listed in «Звуки».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundId {
    /// Automatic switch.
    Autoswitch,
    /// Conversion by hotkey.
    ManualConvert,
    /// Layout change.
    LayoutChanged,
    /// Undo.
    Cancel,
    /// Possible typo.
    Suspicious,
    /// Autoreplace.
    Autoreplace,
    /// Case fixed.
    CaseFixed,
    /// Clipboard converted.
    ClipboardConvert,
    /// Error.
    Error,
}

impl SoundId {
    /// All events.
    pub const ALL: [SoundId; 9] = [
        SoundId::Autoswitch,
        SoundId::ManualConvert,
        SoundId::LayoutChanged,
        SoundId::Cancel,
        SoundId::Suspicious,
        SoundId::Autoreplace,
        SoundId::CaseFixed,
        SoundId::ClipboardConvert,
        SoundId::Error,
    ];

    fn text(self) -> Text {
        match self {
            SoundId::Autoswitch => Text::SoundEvAutoswitch,
            SoundId::ManualConvert => Text::SoundEvManualConvert,
            SoundId::LayoutChanged => Text::SoundEvLayoutChanged,
            SoundId::Cancel => Text::SoundEvCancel,
            SoundId::Suspicious => Text::SoundEvSuspicious,
            SoundId::Autoreplace => Text::SoundEvAutoreplace,
            SoundId::CaseFixed => Text::SoundEvCaseFixed,
            SoundId::ClipboardConvert => Text::SoundEvClipboardConvert,
            SoundId::Error => Text::SoundEvError,
        }
    }

    /// Settings of the event in `config`.
    pub fn setting(self, config: &mut Config) -> &mut SoundEvent {
        let e = &mut config.sounds.events;
        match self {
            SoundId::Autoswitch => &mut e.autoswitch,
            SoundId::ManualConvert => &mut e.manual_convert,
            SoundId::LayoutChanged => &mut e.layout_changed,
            SoundId::Cancel => &mut e.cancel,
            SoundId::Suspicious => &mut e.suspicious,
            SoundId::Autoreplace => &mut e.autoreplace,
            SoundId::CaseFixed => &mut e.case_fixed,
            SoundId::ClipboardConvert => &mut e.clipboard_convert,
            SoundId::Error => &mut e.error,
        }
    }
}

/// Requests from the settings window to the application.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsEvent {
    /// Save and apply this configuration.
    Apply(Box<Config>),
    /// Report the next key combination (sent back as [`SettingsInput::HotkeyCaptured`]).
    CaptureHotkey,
    /// Stop waiting for a key combination.
    CancelCapture,
    /// Play a sound for testing.
    PlaySound {
        /// Event.
        sound: SoundId,
        /// Custom WAV file, if set.
        file: Option<String>,
        /// Beep mode.
        beep: bool,
    },
    /// Download a vetted optional Hunspell package.
    DownloadDictionary(String),
    /// Restart the program asking the system for administrator rights.
    RestartElevated,
    /// The window was closed.
    Closed,
}

/// Messages from the application to the settings window.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsInput {
    /// A key combination was captured.
    HotkeyCaptured(Hotkey),
    /// Capture ended without a combination.
    CaptureCancelled,
    /// The configuration changed outside the window (tray menu).
    ConfigChanged(Box<Config>),
    /// Show a proposed switching rule.
    SuggestRule(Rule),
    /// Prefill a new autoreplace entry from text selected in another app.
    AddAutoreplace(String),
    /// «Запускать с правами Администратора» was applied without the rights.
    ElevationRequired {
        /// A restart was already attempted and did not happen.
        failed: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneralTab {
    Basic,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExclusionTab {
    Executable,
    Title,
    Folder,
}

#[derive(Debug, Clone, PartialEq)]
enum Dialog {
    Hotkey {
        action: HotkeyAction,
        text: String,
        capturing: bool,
        error: Option<String>,
    },
    Rule {
        index: Option<usize>,
        rule: Rule,
        suggestion: bool,
    },
    Exclusion {
        tab: ExclusionTab,
        index: Option<usize>,
        value: String,
    },
    AutoReplace {
        index: Option<usize>,
        item: AutoReplaceItem,
        remember_cursor: bool,
    },
    /// «Запускать с правами Администратора» needs a restart to take effect.
    Elevation {
        /// A previous attempt was declined or failed.
        failed: bool,
    },
}

/// An installed keyboard layout shown in the flag settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutEntry {
    /// Locale such as `en-US`, the key of `general.layout_flags`.
    pub locale: String,
    /// Display name, e.g. «Английский (США)».
    pub name: String,
}

/// Flag pictures uploaded to egui, created on first use.
#[derive(Default)]
struct FlagTextures(std::collections::HashMap<Flag, egui::TextureHandle>);

impl Clone for FlagTextures {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for FlagTextures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FlagTextures").field(&self.0.len()).finish()
    }
}

impl FlagTextures {
    fn get(&mut self, ctx: &egui::Context, flag: Flag) -> egui::load::SizedTexture {
        let handle = self.0.entry(flag).or_insert_with(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [32, 32],
                &crate::flags::render(flag, false, false),
            );
            ctx.load_texture(
                format!("flag-{}", flag.code()),
                image,
                egui::TextureOptions::NEAREST,
            )
        });
        egui::load::SizedTexture::new(handle.id(), egui::vec2(24.0, 24.0))
    }
}

/// Stores the flag chosen for `locale`; the region's own flag is not stored.
pub fn set_layout_flag(
    layout_flags: &mut std::collections::BTreeMap<String, String>,
    locale: &str,
    flag: Flag,
) {
    if Flag::for_locale(locale) == Some(flag) {
        layout_flags.remove(locale);
    } else {
        layout_flags.insert(locale.to_string(), flag.code().to_string());
    }
}

/// What the window should do after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    /// Keep the window open.
    Stay,
    /// Close the window.
    Close,
}

/// State of the settings window.
#[derive(Debug, Clone)]
pub struct SettingsView {
    system_language: Lang,
    draft: Config,
    applied: Config,
    section: Section,
    general_tab: GeneralTab,
    exclusion_tab: ExclusionTab,
    selected_hotkey: Option<HotkeyAction>,
    selected_rule: Option<usize>,
    selected_exclusion: Option<usize>,
    selected_autoreplace: Option<usize>,
    selected_sound: SoundId,
    dialog: Option<Dialog>,
    layouts: Vec<LayoutEntry>,
    flag_textures: FlagTextures,
    pending_autoreplace: std::collections::VecDeque<String>,
    new_spell_word: String,
}

fn weak(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).weak().small());
}

const CONTROL_HEIGHT: f32 = 28.0;

/// Roomier controls in the content area and dialogs; the navigation stays compact.
pub(crate) fn content_style(ui: &mut Ui) {
    let spacing = ui.spacing_mut();
    spacing.interact_size = egui::vec2(88.0, CONTROL_HEIGHT);
    spacing.button_padding = egui::vec2(10.0, 5.0);
    spacing.item_spacing = egui::vec2(10.0, 6.0);
    spacing.icon_width = 16.0;
    spacing.icon_width_inner = 9.0;
    spacing.combo_width = 200.0;
    spacing.scroll.dormant_handle_opacity = 0.6;
    spacing.scroll.dormant_background_opacity = 0.15;
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
}

pub(crate) fn action_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text).min_size(egui::vec2(88.0, CONTROL_HEIGHT))
}

/// Explicit column widths, including on the very first frame. Each row fills
/// the parent and grows vertically to fit its tallest cell.
fn table_row(
    ui: &mut Ui,
    proportions: &[f32],
    striped: bool,
    mut cell: impl FnMut(&mut Ui, usize),
) {
    let fill = if striped {
        ui.visuals().faint_bg_color
    } else {
        Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            let usable = width - ui.spacing().item_spacing.x * (proportions.len() - 1) as f32;
            ui.horizontal_top(|ui| {
                for (index, fraction) in proportions.iter().enumerate() {
                    let width = usable * fraction;
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, CONTROL_HEIGHT),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width);
                            cell(ui, index);
                        },
                    );
                }
            });
        });
}

fn cell_label(ui: &mut Ui, text: impl Into<egui::WidgetText>) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), CONTROL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(egui::Label::new(text).wrap());
        },
    );
}

fn cell_selectable(ui: &mut Ui, selected: bool, text: &str) -> egui::Response {
    ui.add(
        egui::Button::selectable(selected, (text, egui::Atom::grow()))
            .min_size(egui::vec2(ui.available_width(), CONTROL_HEIGHT))
            .wrap(),
    )
}

/// A label and a full-width control, aligned with all other form rows.
fn form_row(ui: &mut Ui, label: &str, mut control: impl FnMut(&mut Ui)) {
    table_row(ui, &[0.53, 0.47], false, |ui, column| {
        if column == 0 {
            cell_label(ui, label);
        } else {
            control(ui);
        }
    });
}

fn soon(ui: &mut Ui, lang: Lang) {
    ui.add(
        egui::Label::new(
            RichText::new(format!("({})", tr(Text::InDevelopment, lang)))
                .small()
                .color(Color32::from_rgb(0xB0, 0x80, 0x20)),
        )
        .extend(),
    );
}

fn checkbox(ui: &mut Ui, value: &mut bool, text: Text, lang: Lang, ready: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(value, tr(text, lang));
        if !ready {
            soon(ui, lang);
        }
    });
}

fn switch_key_text(key: SwitchKey) -> Text {
    match key {
        SwitchKey::None => Text::KeyNone,
        SwitchKey::RightCtrl => Text::KeyRightCtrl,
        SwitchKey::LeftCtrl => Text::KeyLeftCtrl,
        SwitchKey::RightShift => Text::KeyRightShift,
        SwitchKey::LeftShift => Text::KeyLeftShift,
        SwitchKey::CapsLock => Text::KeyCapsLock,
        SwitchKey::LeftAlt => Text::KeyLeftAlt,
        SwitchKey::RightAlt => Text::KeyRightAlt,
        SwitchKey::Space => Text::KeySpace,
    }
}

const SWITCH_KEYS: [SwitchKey; 8] = [
    SwitchKey::None,
    SwitchKey::RightCtrl,
    SwitchKey::LeftCtrl,
    SwitchKey::RightShift,
    SwitchKey::LeftShift,
    SwitchKey::CapsLock,
    SwitchKey::LeftAlt,
    SwitchKey::RightAlt,
];

fn switch_key_combo(ui: &mut Ui, id: &str, value: &mut SwitchKey, lang: Lang, with_none: bool) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(tr(switch_key_text(*value), lang))
        .show_ui(ui, |ui| {
            for key in SWITCH_KEYS
                .iter()
                .filter(|k| with_none || **k != SwitchKey::None)
            {
                ui.selectable_value(value, *key, tr(switch_key_text(*key), lang));
            }
        });
}

fn match_kind_text(kind: MatchKind) -> Text {
    match kind {
        MatchKind::Contains => Text::RuleMatchContains,
        MatchKind::StartsWith => Text::RuleMatchStartsWith,
        MatchKind::Equals => Text::RuleMatchEquals,
    }
}

fn rule_action_text(action: RuleAction) -> Text {
    match action {
        RuleAction::Switch => Text::RuleActionSwitch,
        RuleAction::Stay => Text::RuleActionStay,
    }
}

fn trigger_text(trigger: AutoReplaceTrigger) -> Text {
    match trigger {
        AutoReplaceTrigger::Tooltip => Text::TriggerTooltip,
        AutoReplaceTrigger::Space => Text::TriggerSpace,
        AutoReplaceTrigger::Enter => Text::TriggerEnter,
        AutoReplaceTrigger::Tab => Text::TriggerTab,
        AutoReplaceTrigger::Hotkey => Text::TriggerHotkey,
    }
}

/// Buttons «Добавить...», «Изменить», «Удалить» under a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListButton {
    Add,
    Edit,
    Delete,
}

fn list_buttons(ui: &mut Ui, lang: Lang, has_selection: bool) -> Option<ListButton> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        if ui.add(action_button(tr(Text::BtnAdd, lang))).clicked() {
            clicked = Some(ListButton::Add);
        }
        if ui
            .add_enabled(has_selection, action_button(tr(Text::BtnEdit, lang)))
            .clicked()
        {
            clicked = Some(ListButton::Edit);
        }
        if ui
            .add_enabled(has_selection, action_button(tr(Text::BtnDelete, lang)))
            .clicked()
        {
            clicked = Some(ListButton::Delete);
        }
    });
    clicked
}

impl SettingsView {
    /// Opens the window on `section` with `config`.
    pub fn new(config: Config, section: Section, system_language: Lang) -> Self {
        Self {
            system_language,
            draft: config.clone(),
            applied: config,
            section,
            general_tab: GeneralTab::Basic,
            exclusion_tab: ExclusionTab::Executable,
            selected_hotkey: None,
            selected_rule: None,
            selected_exclusion: None,
            selected_autoreplace: None,
            selected_sound: SoundId::Autoswitch,
            dialog: None,
            layouts: Vec::new(),
            flag_textures: FlagTextures::default(),
            pending_autoreplace: std::collections::VecDeque::new(),
            new_spell_word: String::new(),
        }
    }

    /// Installed layouts for the flag settings.
    pub fn set_layouts(&mut self, layouts: Vec<LayoutEntry>) {
        self.layouts = layouts;
    }

    /// Navigate without replacing an unsaved draft.
    pub fn open_section(&mut self, section: Section) {
        self.section = section;
    }

    /// The edited configuration.
    pub fn draft(&self) -> &Config {
        &self.draft
    }

    /// Whether the draft differs from the applied configuration.
    pub fn is_modified(&self) -> bool {
        self.draft != self.applied
    }

    fn lang(&self) -> Lang {
        self.applied
            .general
            .ui_language
            .resolve(self.system_language)
    }

    /// Handles a message from the application.
    pub fn handle(&mut self, input: SettingsInput) {
        match input {
            SettingsInput::HotkeyCaptured(hotkey) => {
                if let Some(Dialog::Hotkey {
                    text,
                    capturing,
                    error,
                    ..
                }) = &mut self.dialog
                {
                    *text = hotkey.to_string();
                    *capturing = false;
                    *error = None;
                }
            }
            SettingsInput::CaptureCancelled => {
                if let Some(Dialog::Hotkey { capturing, .. }) = &mut self.dialog {
                    *capturing = false;
                }
            }
            SettingsInput::ConfigChanged(config) => {
                // The tray menu and the indicator's own menu change these
                // outside the window; the draft follows so that applying it
                // does not undo what the user just did.
                for target in [&mut self.draft, &mut self.applied] {
                    target.general.autoswitch = config.general.autoswitch;
                    target.general.floating_indicator = config.general.floating_indicator;
                    target.general.floating_indicator_locked =
                        config.general.floating_indicator_locked;
                    target.general.floating_indicator_pos = config.general.floating_indicator_pos;
                    target.sounds.enabled = config.sounds.enabled;
                }
            }
            SettingsInput::ElevationRequired { failed } => {
                self.section = Section::General;
                self.general_tab = GeneralTab::Basic;
                self.dialog = Some(Dialog::Elevation { failed });
            }
            SettingsInput::SuggestRule(rule) => {
                self.section = Section::Rules;
                self.dialog = Some(Dialog::Rule {
                    index: None,
                    rule,
                    suggestion: true,
                });
            }
            SettingsInput::AddAutoreplace(text) => {
                self.pending_autoreplace.push_back(text);
                self.open_pending_autoreplace();
            }
        }
    }

    fn open_pending_autoreplace(&mut self) {
        if self.dialog.is_none()
            && let Some(text) = self.pending_autoreplace.pop_front()
        {
            self.section = Section::Autoreplace;
            self.dialog = Some(Dialog::AutoReplace {
                index: None,
                item: AutoReplaceItem {
                    to: text,
                    ..Default::default()
                },
                remember_cursor: false,
            });
        }
    }

    /// Whether a hotkey dialog waits for a key press.
    pub fn is_capturing(&self) -> bool {
        matches!(
            self.dialog,
            Some(Dialog::Hotkey {
                capturing: true,
                ..
            })
        )
    }

    fn apply(&mut self, events: &mut Vec<SettingsEvent>) {
        let mut config = self.draft.clone();
        let _ = config.sanitize();
        self.draft = config.clone();
        self.applied = config.clone();
        events.push(SettingsEvent::Apply(Box::new(config)));
    }

    /// Draws the window contents.
    pub fn ui(&mut self, ui: &mut Ui, events: &mut Vec<SettingsEvent>) -> WindowAction {
        self.open_pending_autoreplace();
        let lang = self.lang();
        let mut action = WindowAction::Stay;
        egui::Panel::bottom("settings_buttons")
            .resizable(false)
            .exact_size(52.0)
            .show(ui, |ui| {
                content_style(ui);
                ui.horizontal_centered(|ui| {
                    ui.label(okbs_core::VERSION);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                self.is_modified(),
                                action_button(tr(Text::BtnApply, lang)),
                            )
                            .clicked()
                        {
                            self.apply(events);
                        }
                        if ui.add(action_button(tr(Text::BtnCancel, lang))).clicked() {
                            action = WindowAction::Close;
                        }
                        if ui.add(action_button(tr(Text::BtnOk, lang))).clicked() {
                            if self.is_modified() {
                                self.apply(events);
                            }
                            action = WindowAction::Close;
                        }
                    });
                });
            });
        egui::Panel::left("settings_sections")
            .resizable(false)
            .exact_size(190.0)
            .show(ui, |ui| {
                content_style(ui);
                // Keep enough horizontal room for the long Russian section names.
                ui.spacing_mut().button_padding.x = 4.0;
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add_space(6.0);
                for section in Section::ALL {
                    if cell_selectable(ui, self.section == section, tr(section.text(), lang))
                        .clicked()
                    {
                        self.section = section;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(18.0);
                    crate::branding::show(ui);
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            content_style(ui);
            let width = ui.available_width();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.heading(tr(self.section.text(), lang));
                    ui.add_space(4.0);
                    match self.section {
                        Section::General => self.general(ui, lang),
                        Section::Hotkeys => self.hotkeys(ui, lang),
                        Section::Rules => self.rules(ui, lang),
                        Section::Exclusions => self.exclusions(ui, lang),
                        Section::Troubleshooting => self.troubleshooting(ui, lang),
                        Section::Autoreplace => self.autoreplace(ui, lang),
                        Section::Sounds => self.sounds(ui, lang, events),
                        Section::Spellcheck => self.spellcheck(ui, lang, events),
                    }
                });
        });
        self.dialogs(ui, lang, events);
        if action == WindowAction::Close && self.is_capturing() {
            events.push(SettingsEvent::CancelCapture);
        }
        action
    }

    fn general(&mut self, ui: &mut Ui, lang: Lang) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.general_tab,
                GeneralTab::Basic,
                tr(Text::TabBasic, lang),
            );
            ui.selectable_value(
                &mut self.general_tab,
                GeneralTab::Advanced,
                tr(Text::TabAdvanced, lang),
            );
        });
        ui.separator();
        let g = &mut self.draft.general;
        match self.general_tab {
            GeneralTab::Basic => {
                checkbox(
                    ui,
                    &mut g.autostart,
                    Text::OptAutostart,
                    lang,
                    cfg!(windows),
                );
                checkbox(ui, &mut g.autoswitch, Text::OptAutoswitch, lang, true);
                if cfg!(windows) {
                    checkbox(ui, &mut g.run_elevated, Text::OptRunElevated, lang, true);
                    if g.run_elevated {
                        ui.indent("run_elevated_hint", |ui| {
                            ui.label(RichText::new(tr(Text::ElevationHint, lang)).weak().small());
                        });
                    }
                }
                checkbox(ui, &mut g.tray_flags, Text::OptTrayFlags, lang, true);
                ui.add_enabled_ui(g.tray_flags, |ui| {
                    checkbox(
                        ui,
                        &mut g.tray_flags_full_brightness,
                        Text::OptTrayFlagsFullBrightness,
                        lang,
                        true,
                    );
                });
                if g.tray_flags && !self.layouts.is_empty() {
                    let ctx = ui.ctx().clone();
                    ui.indent("layout_flags", |ui| {
                        ui.label(tr(Text::OptLayoutFlags, lang));
                        for entry in &self.layouts {
                            table_row(ui, &[0.07, 0.46, 0.47], false, |ui, column| {
                                let current = flag_for_layout(&g.layout_flags, &entry.locale);
                                match column {
                                    0 => match current {
                                        Some(flag) => {
                                            ui.add_space(4.0);
                                            ui.image(self.flag_textures.get(&ctx, flag));
                                        }
                                        None => {
                                            cell_label(ui, "—");
                                        }
                                    },
                                    1 => cell_label(ui, &entry.name),
                                    _ => {
                                        egui::ComboBox::from_id_salt((
                                            "layout_flag",
                                            entry.locale.as_str(),
                                        ))
                                        .width(ui.available_width())
                                        .selected_text(
                                            current.map_or(tr(Text::FlagNotSet, lang), |f| {
                                                f.name(lang)
                                            }),
                                        )
                                        .show_ui(
                                            ui,
                                            |ui| {
                                                for flag in Flag::ALL {
                                                    ui.horizontal(|ui| {
                                                        ui.image(
                                                            self.flag_textures.get(&ctx, flag),
                                                        );
                                                        if ui
                                                            .selectable_label(
                                                                current == Some(flag),
                                                                flag.name(lang),
                                                            )
                                                            .clicked()
                                                        {
                                                            set_layout_flag(
                                                                &mut g.layout_flags,
                                                                &entry.locale,
                                                                flag,
                                                            );
                                                        }
                                                    });
                                                }
                                            },
                                        );
                                    }
                                }
                            });
                        }
                    });
                }
                checkbox(
                    ui,
                    &mut g.hotkeys_off_when_autoswitch_off,
                    Text::OptHotkeysOffWhenAutoswitchOff,
                    lang,
                    true,
                );
                ui.add_space(8.0);
                form_row(ui, tr(Text::OptUiLanguage, lang), |ui| {
                    egui::ComboBox::from_id_salt("ui_language")
                        .width(ui.available_width())
                        .selected_text(ui_language_label(g.ui_language, self.system_language, lang))
                        .show_ui(ui, |ui| {
                            for preference in [UiLanguage::System, UiLanguage::Ru, UiLanguage::En] {
                                ui.selectable_value(
                                    &mut g.ui_language,
                                    preference,
                                    ui_language_label(preference, self.system_language, lang),
                                );
                            }
                        });
                });
                form_row(ui, tr(Text::OptTheme, lang), |ui| {
                    let theme_text = |t: Theme| match t {
                        Theme::System => Text::ThemeSystem,
                        Theme::Light => Text::ThemeLight,
                        Theme::Dark => Text::ThemeDark,
                    };
                    egui::ComboBox::from_id_salt("theme")
                        .width(ui.available_width())
                        .selected_text(tr(theme_text(g.theme), lang))
                        .show_ui(ui, |ui| {
                            for t in [Theme::System, Theme::Light, Theme::Dark] {
                                ui.selectable_value(&mut g.theme, t, tr(theme_text(t), lang));
                            }
                        });
                });
                ui.separator();
                checkbox(
                    ui,
                    &mut g.floating_indicator,
                    Text::OptFloatingIndicator,
                    lang,
                    true,
                );
                ui.add_enabled_ui(g.floating_indicator, |ui| {
                    checkbox(
                        ui,
                        &mut g.floating_indicator_autohide,
                        Text::OptFloatingIndicatorAutohide,
                        lang,
                        true,
                    );
                    checkbox(
                        ui,
                        &mut g.floating_indicator_locked,
                        Text::OptLockIndicator,
                        lang,
                        true,
                    );
                });
            }
            GeneralTab::Advanced => {
                let a = &mut self.draft.advanced;
                checkbox(
                    ui,
                    &mut a.fix_abbreviations,
                    Text::OptFixAbbreviations,
                    lang,
                    true,
                );
                checkbox(
                    ui,
                    &mut a.fix_two_capitals,
                    Text::OptFixTwoCapitals,
                    lang,
                    true,
                );
                checkbox(
                    ui,
                    &mut a.fix_accidental_capslock,
                    Text::OptFixAccidentalCapsLock,
                    lang,
                    true,
                );
                checkbox(
                    ui,
                    &mut a.disable_capslock,
                    Text::OptDisableCapsLock,
                    lang,
                    cfg!(windows),
                );
                checkbox(
                    ui,
                    &mut a.scrolllock_as_capslock,
                    Text::OptScrollLockAsCapsLock,
                    lang,
                    true,
                );
                if cfg!(windows) {
                    checkbox(
                        ui,
                        &mut a.fix_layout_in_menus,
                        Text::OptFixLayoutInMenus,
                        lang,
                        true,
                    );
                }
                checkbox(
                    ui,
                    &mut a.show_clipboard_conversion_window,
                    Text::OptShowClipboardConversionWindow,
                    lang,
                    true,
                );
                checkbox(
                    ui,
                    &mut a.clipboard_history,
                    Text::OptClipboardHistory,
                    lang,
                    cfg!(windows),
                );
                ui.add_enabled_ui(a.clipboard_history, |ui| {
                    checkbox(
                        ui,
                        &mut a.clipboard_history_persist,
                        Text::OptClipboardHistoryPersist,
                        lang,
                        cfg!(windows),
                    );
                });
                checkbox(ui, &mut a.show_tooltips, Text::OptShowTooltips, lang, true);
                checkbox(
                    ui,
                    &mut a.double_space_comma,
                    Text::OptDoubleSpaceComma,
                    lang,
                    true,
                );
            }
        }
        ui.add_space(10.0);
        ui.group(|ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(tr(Text::GroupLayoutSwitching, lang)).strong());
            let s = &mut self.draft.switching;
            form_row(ui, tr(Text::OptSwitchKey, lang), |ui| {
                ui.spacing_mut().combo_width = ui.available_width();
                switch_key_combo(ui, "switch_key", &mut s.switch_key, lang, true);
            });
            ui.add_enabled_ui(s.switch_key != SwitchKey::None, |ui| {
                table_row(ui, &[0.5, 0.5], false, |ui, column| {
                    if column == 0 {
                        ui.checkbox(
                            &mut s.switch_key_only_pair,
                            tr(Text::OptSwitchKeyOnlyPair, lang),
                        );
                    } else {
                        ui.checkbox(
                            &mut s.single_layout_for_all_windows,
                            tr(Text::OptSingleLayout, lang),
                        );
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.direct_keys.enabled, tr(Text::OptDirectKeys, lang));
            });
            ui.add_enabled_ui(s.direct_keys.enabled, |ui| {
                table_row(ui, &[0.5, 0.5], false, |ui, column| {
                    ui.horizontal(|ui| {
                        ui.label(if column == 0 { "RU:" } else { "EN:" });
                        ui.spacing_mut().combo_width = ui.available_width();
                        if column == 0 {
                            switch_key_combo(ui, "direct_ru", &mut s.direct_keys.ru, lang, false);
                        } else {
                            switch_key_combo(ui, "direct_en", &mut s.direct_keys.en, lang, false);
                        }
                    });
                });
            });
        });
    }

    fn hotkeys(&mut self, ui: &mut Ui, lang: Lang) {
        ui.label(tr(Text::HotkeysHint, lang));
        ui.add_space(4.0);
        let mut open = None;
        const COLUMNS: &[f32] = &[0.7, 0.3];
        table_row(ui, COLUMNS, false, |ui, column| {
            cell_label(
                ui,
                RichText::new(tr(
                    if column == 0 {
                        Text::ColumnAction
                    } else {
                        Text::ColumnCombination
                    },
                    lang,
                ))
                .strong(),
            );
        });
        egui::ScrollArea::vertical()
            .id_salt("hotkeys_list")
            .max_height((ui.clip_rect().bottom() - ui.cursor().top() - 50.0).max(160.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, &action) in HotkeyAction::ALL.iter().enumerate() {
                    let selected = self.selected_hotkey == Some(action);
                    table_row(ui, COLUMNS, index % 2 == 0, |ui, column| {
                        if column == 0 {
                            let response =
                                cell_selectable(ui, selected, tr(hotkey_action_text(action), lang));
                            if response.clicked() {
                                self.selected_hotkey = Some(action);
                            }
                            if response.double_clicked() {
                                open = Some(action);
                            }
                        } else {
                            cell_label(ui, self.draft.hotkeys.get(action).to_string());
                        }
                    });
                }
            });
        ui.add_space(6.0);
        if ui
            .add_enabled(
                self.selected_hotkey.is_some(),
                egui::Button::new(tr(Text::BtnAssign, lang)),
            )
            .clicked()
        {
            open = self.selected_hotkey;
        }
        if let Some(action) = open {
            self.selected_hotkey = Some(action);
            self.dialog = Some(Dialog::Hotkey {
                action,
                text: self.draft.hotkeys.get(action).to_string(),
                capturing: false,
                error: None,
            });
        }
    }

    fn rules(&mut self, ui: &mut Ui, lang: Lang) {
        checkbox(
            ui,
            &mut self.draft.rules_options.improve_switching,
            Text::OptImproveSwitching,
            lang,
            true,
        );
        ui.add(egui::Label::new(tr(Text::ImproveSwitchingHint, lang)).wrap());
        ui.separator();
        ui.label(tr(Text::RulesHint, lang));
        ui.add_space(4.0);
        let mut edit = None;
        const COLUMNS: &[f32] = &[0.4, 0.3, 0.3];
        table_row(ui, COLUMNS, false, |ui, column| {
            cell_label(
                ui,
                RichText::new(tr(
                    [
                        Text::ColumnRulePattern,
                        Text::ColumnRuleCondition,
                        Text::ColumnRuleAction,
                    ][column],
                    lang,
                ))
                .strong(),
            );
        });
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, rule) in self.draft.rules.iter().enumerate() {
                table_row(ui, COLUMNS, i % 2 == 0, |ui, column| match column {
                    0 => {
                        let response =
                            cell_selectable(ui, self.selected_rule == Some(i), &rule.pattern);
                        if response.clicked() {
                            self.selected_rule = Some(i);
                        }
                        if response.double_clicked() {
                            edit = Some(i);
                        }
                    }
                    1 => cell_label(ui, tr(match_kind_text(rule.match_kind), lang)),
                    _ => cell_label(ui, tr(rule_action_text(rule.action), lang)),
                });
            }
        });
        ui.add_space(6.0);
        match list_buttons(ui, lang, self.selected_rule.is_some()) {
            Some(ListButton::Add) => {
                self.dialog = Some(Dialog::Rule {
                    index: None,
                    rule: Rule::default(),
                    suggestion: false,
                })
            }
            Some(ListButton::Edit) => edit = self.selected_rule,
            Some(ListButton::Delete) => {
                if let Some(i) = self
                    .selected_rule
                    .take()
                    .filter(|&i| i < self.draft.rules.len())
                {
                    self.draft.rules.remove(i);
                }
            }
            None => {}
        }
        if let Some(i) = edit.filter(|&i| i < self.draft.rules.len()) {
            self.dialog = Some(Dialog::Rule {
                index: Some(i),
                rule: self.draft.rules[i].clone(),
                suggestion: false,
            });
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(tr(Text::OptSuggestRuleAfterCancels, lang));
            ui.add(
                egui::DragValue::new(&mut self.draft.rules_options.suggest_rule_after_cancels)
                    .range(0..=10),
            );
        });
    }

    fn exclusions(&mut self, ui: &mut Ui, lang: Lang) {
        ui.add(egui::Label::new(tr(Text::ExclusionsHint, lang)).wrap());
        ui.horizontal_wrapped(|ui| {
            for (tab, text) in [
                (ExclusionTab::Executable, Text::ExclusionsByExecutable),
                (ExclusionTab::Title, Text::ExclusionsByTitle),
                (ExclusionTab::Folder, Text::ExclusionsByFolder),
            ] {
                if ui
                    .selectable_value(&mut self.exclusion_tab, tab, tr(text, lang))
                    .clicked()
                {
                    self.selected_exclusion = None;
                }
            }
        });
        ui.separator();
        let ex = &self.draft.exclusions;
        let values: Vec<String> = match self.exclusion_tab {
            ExclusionTab::Executable => ex.executables.iter().map(|e| e.path.clone()).collect(),
            ExclusionTab::Title => ex.titles.iter().map(|e| e.contains.clone()).collect(),
            ExclusionTab::Folder => ex.folders.iter().map(|e| e.path.clone()).collect(),
        };
        let mut edit = None;
        for (i, value) in values.iter().enumerate() {
            let response = cell_selectable(ui, self.selected_exclusion == Some(i), value);
            if response.clicked() {
                self.selected_exclusion = Some(i);
            }
            if response.double_clicked() {
                edit = Some(i);
            }
        }
        ui.add_space(6.0);
        match list_buttons(ui, lang, self.selected_exclusion.is_some()) {
            Some(ListButton::Add) => {
                self.dialog = Some(Dialog::Exclusion {
                    tab: self.exclusion_tab,
                    index: None,
                    value: String::new(),
                })
            }
            Some(ListButton::Edit) => edit = self.selected_exclusion,
            Some(ListButton::Delete) => {
                if let Some(i) = self.selected_exclusion.take() {
                    let ex = &mut self.draft.exclusions;
                    match self.exclusion_tab {
                        ExclusionTab::Executable if i < ex.executables.len() => {
                            ex.executables.remove(i);
                        }
                        ExclusionTab::Title if i < ex.titles.len() => {
                            ex.titles.remove(i);
                        }
                        ExclusionTab::Folder if i < ex.folders.len() => {
                            ex.folders.remove(i);
                        }
                        _ => {}
                    }
                }
            }
            None => {}
        }
        if let Some(i) = edit.filter(|&i| i < values.len()) {
            self.dialog = Some(Dialog::Exclusion {
                tab: self.exclusion_tab,
                index: Some(i),
                value: values[i].clone(),
            });
        }
    }

    fn troubleshooting(&mut self, ui: &mut Ui, lang: Lang) {
        let t = &mut self.draft.troubleshooting;
        ui.label(tr(Text::NoSwitchAfterHint, lang));
        for row in 0..4 {
            table_row(ui, &[0.5, 0.5], false, |ui, column| {
                let n = &mut t.no_switch_after;
                let (value, label) = match (row, column) {
                    (0, 0) => (&mut n.backspace, Text::KeyBackspace),
                    (1, 0) => (&mut n.arrow_left, Text::KeyArrowLeft),
                    (2, 0) => (&mut n.arrow_right, Text::KeyArrowRight),
                    (3, 0) => (&mut n.arrow_up, Text::KeyArrowUp),
                    (0, _) => (&mut n.arrow_down, Text::KeyArrowDown),
                    (1, _) => (&mut n.delete, Text::KeyDelete),
                    (2, _) => (&mut n.manual_layout_change, Text::NoSwitchAfterLayoutChange),
                    _ => return,
                };
                ui.checkbox(value, tr(label, lang));
            });
        }
        ui.add_space(8.0);
        checkbox(
            ui,
            &mut t.only_pair_layouts,
            Text::OptOnlyPairLayouts,
            lang,
            true,
        );
        checkbox(
            ui,
            &mut t.no_switch_on_tab_enter,
            Text::OptNoSwitchOnTabEnter,
            lang,
            true,
        );
        checkbox(
            ui,
            &mut t.ignore_excluded_apps_completely,
            Text::OptIgnoreExcludedApps,
            lang,
            true,
        );
        ui.add_space(10.0);
        ui.group(|ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(tr(Text::GroupDiagnostics, lang)).strong());
            ui.checkbox(&mut self.draft.log.debug, tr(Text::OptLogDebug, lang));
            ui.label(RichText::new(tr(Text::LogDebugHint, lang)).weak().small());
        });
        ui.add_space(10.0);
        ui.group(|ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(tr(Text::GroupDetector, lang)).strong());
            let d = &mut t.detector;
            ui.label(tr(Text::OptSensitivity, lang));
            ui.horizontal(|ui| {
                ui.label(tr(Text::SensitivityCautious, lang));
                ui.spacing_mut().slider_width = (ui.available_width() - 95.0).max(120.0);
                ui.add(egui::Slider::new(&mut d.sensitivity, -1.0..=1.0).show_value(false));
                ui.label(tr(Text::SensitivityEager, lang));
            });
            form_row(ui, tr(Text::OptMinWordLen, lang), |ui| {
                ui.add(egui::DragValue::new(&mut d.min_word_len).range(1..=10));
            });
            ui.checkbox(
                &mut d.password_heuristic,
                tr(Text::OptPasswordHeuristic, lang),
            );
        });
        ui.add_space(10.0);
        ui.group(|ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(tr(Text::GroupTiming, lang)).strong());
            let s = &mut self.draft.switching;
            form_row(ui, tr(Text::OptLayoutSwitchDelay, lang), |ui| {
                ui.add(egui::DragValue::new(&mut s.layout_switch_delay_ms).range(0..=2000));
            });
            form_row(ui, tr(Text::OptInjectDelay, lang), |ui| {
                ui.add(egui::DragValue::new(&mut s.inject_key_delay_ms).range(0..=100));
            });
        });
    }

    fn autoreplace(&mut self, ui: &mut Ui, lang: Lang) {
        ui.add(egui::Label::new(tr(Text::AutoreplaceHint, lang)).wrap());
        ui.add(egui::Label::new(tr(Text::AutoreplaceStageHint, lang)).wrap());
        ui.checkbox(
            &mut self.draft.autoreplace.enabled,
            tr(Text::OptAutoreplaceEnabled, lang),
        );
        ui.add_space(4.0);
        let mut edit = None;
        const COLUMNS: &[f32] = &[0.35, 0.65];
        table_row(ui, COLUMNS, false, |ui, column| {
            cell_label(
                ui,
                RichText::new(tr(
                    if column == 0 {
                        Text::ColumnReplaceWhat
                    } else {
                        Text::ColumnReplaceWith
                    },
                    lang,
                ))
                .strong(),
            );
        });
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, item) in self.draft.autoreplace.items.iter().enumerate() {
                table_row(ui, COLUMNS, i % 2 == 0, |ui, column| {
                    if column == 0 {
                        let response =
                            cell_selectable(ui, self.selected_autoreplace == Some(i), &item.from);
                        if response.clicked() {
                            self.selected_autoreplace = Some(i);
                        }
                        if response.double_clicked() {
                            edit = Some(i);
                        }
                    } else {
                        let preview: String = item
                            .to
                            .chars()
                            .take(60)
                            .map(|c| if c == '\n' { '⏎' } else { c })
                            .collect();
                        cell_label(ui, preview);
                    }
                });
            }
        });
        ui.add_space(6.0);
        match list_buttons(ui, lang, self.selected_autoreplace.is_some()) {
            Some(ListButton::Add) => {
                self.dialog = Some(Dialog::AutoReplace {
                    index: None,
                    item: AutoReplaceItem::default(),
                    remember_cursor: false,
                })
            }
            Some(ListButton::Edit) => edit = self.selected_autoreplace,
            Some(ListButton::Delete) => {
                if let Some(i) = self
                    .selected_autoreplace
                    .take()
                    .filter(|&i| i < self.draft.autoreplace.items.len())
                {
                    self.draft.autoreplace.items.remove(i);
                }
            }
            None => {}
        }
        if let Some(i) = edit.filter(|&i| i < self.draft.autoreplace.items.len()) {
            let item = self.draft.autoreplace.items[i].clone();
            self.dialog = Some(Dialog::AutoReplace {
                index: Some(i),
                remember_cursor: item.cursor_pos >= 0,
                item,
            });
        }
        ui.add_space(10.0);
        let a = &mut self.draft.autoreplace;
        ui.checkbox(
            &mut a.also_in_other_layout,
            tr(Text::OptReplaceInOtherLayout, lang),
        );
        checkbox(
            ui,
            &mut a.show_in_tray_menu,
            Text::OptShowInTrayMenu,
            lang,
            cfg!(windows),
        );
        form_row(ui, tr(Text::OptReplaceOn, lang), |ui| {
            egui::ComboBox::from_id_salt("autoreplace_trigger")
                .width(ui.available_width())
                .selected_text(tr(trigger_text(a.trigger), lang))
                .show_ui(ui, |ui| {
                    for t in [
                        AutoReplaceTrigger::Tooltip,
                        AutoReplaceTrigger::Space,
                        AutoReplaceTrigger::Enter,
                        AutoReplaceTrigger::Tab,
                        AutoReplaceTrigger::Hotkey,
                    ] {
                        ui.selectable_value(&mut a.trigger, t, tr(trigger_text(t), lang));
                    }
                });
        });
        form_row(ui, tr(Text::OptListOpacity, lang), |ui| {
            ui.spacing_mut().slider_width =
                (ui.available_width() - ui.spacing().interact_size.x - ui.spacing().item_spacing.x)
                    .max(80.0);
            ui.add(egui::Slider::new(&mut a.list_opacity, 0.1..=1.0));
        });
    }

    fn spellcheck(&mut self, ui: &mut Ui, lang: Lang, events: &mut Vec<SettingsEvent>) {
        let spellcheck = &mut self.draft.spellcheck;
        checkbox(
            ui,
            &mut spellcheck.enabled,
            Text::OptSpellcheckEnabled,
            lang,
            true,
        );
        ui.add_enabled_ui(spellcheck.enabled, |ui| {
            checkbox(
                ui,
                &mut spellcheck.check_typed_words,
                Text::OptSpellcheckTypedWords,
                lang,
                true,
            );
            checkbox(
                ui,
                &mut spellcheck.prefer_selection,
                Text::OptSpellcheckSelectionFirst,
                lang,
                true,
            );
            ui.add_space(6.0);
            ui.label(tr(Text::SpellcheckLanguages, lang));
            ui.horizontal(|ui| {
                for (language, label) in
                    [(Lang::Ru, Text::LangRussian), (Lang::En, Text::LangEnglish)]
                {
                    let mut selected = spellcheck.languages.contains(&language);
                    if ui.checkbox(&mut selected, tr(label, lang)).changed() {
                        if selected {
                            spellcheck.languages.push(language);
                        } else {
                            spellcheck.languages.retain(|item| *item != language);
                        }
                    }
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new(tr(Text::SpellcheckHint, lang)).weak().small());
            ui.add_space(12.0);
            ui.label(tr(Text::SpellcheckExtraDictionaries, lang));
            egui::ComboBox::from_id_salt("english_dictionary")
                .selected_text(match spellcheck.english_dictionary.as_deref() {
                    Some("en-gb") => tr(Text::SpellcheckEnGb, lang),
                    _ => tr(Text::SpellcheckBuiltinEn, lang),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut spellcheck.english_dictionary,
                        None,
                        tr(Text::SpellcheckBuiltinEn, lang),
                    );
                    ui.selectable_value(
                        &mut spellcheck.english_dictionary,
                        Some("en-gb".into()),
                        tr(Text::SpellcheckEnGb, lang),
                    );
                });
            if ui.button(tr(Text::SpellcheckDownloadEnGb, lang)).clicked() {
                events.push(SettingsEvent::DownloadDictionary("en-gb".into()));
            }
            ui.add_space(12.0);
            ui.label(tr(Text::SpellcheckPersonalWords, lang));
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.new_spell_word);
                if ui.button(tr(Text::SpellcheckAddWord, lang)).clicked()
                    && !self.new_spell_word.trim().is_empty()
                {
                    spellcheck
                        .custom_words
                        .push(self.new_spell_word.trim().into());
                    self.new_spell_word.clear();
                }
            });
            let mut remove = None;
            for (index, word) in spellcheck.custom_words.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(word);
                    if ui.small_button("×").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                spellcheck.custom_words.remove(index);
            }
        });
    }

    fn sounds(&mut self, ui: &mut Ui, lang: Lang, events: &mut Vec<SettingsEvent>) {
        ui.checkbox(&mut self.draft.sounds.enabled, tr(Text::MenuSounds, lang));
        ui.label(tr(Text::SoundsHint, lang));
        ui.add_space(4.0);
        ui.add_enabled_ui(self.draft.sounds.enabled, |ui| {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, sound) in SoundId::ALL.into_iter().enumerate() {
                    table_row(ui, &[0.08, 0.92], index % 2 == 0, |ui, column| {
                        if column == 0 {
                            let enabled = &mut sound.setting(&mut self.draft).enabled;
                            ui.checkbox(enabled, "");
                        } else if cell_selectable(
                            ui,
                            self.selected_sound == sound,
                            tr(sound.text(), lang),
                        )
                        .clicked()
                        {
                            self.selected_sound = sound;
                        }
                    });
                }
            });
            ui.add_space(8.0);
            let sounds = &mut self.draft.sounds;
            ui.radio_value(
                &mut sounds.mode,
                SoundMode::File,
                tr(Text::SoundPlayFile, lang),
            );
            ui.radio_value(&mut sounds.mode, SoundMode::Beep, tr(Text::SoundBeep, lang));
            ui.add_space(6.0);
            let beep = sounds.mode == SoundMode::Beep;
            let selected = self.selected_sound;
            ui.label(format!(
                "{}: {}",
                tr(Text::ColumnEvent, lang),
                tr(selected.text(), lang)
            ));
            ui.add_enabled_ui(!beep, |ui| {
                ui.label(tr(Text::SoundFile, lang));
                ui.add(
                    egui::TextEdit::singleline(&mut selected.setting(&mut self.draft).file)
                        .desired_width(ui.available_width()),
                );
            });
            ui.horizontal(|ui| {
                if ui.button(tr(Text::BtnPlay, lang)).clicked() {
                    let file = selected.setting(&mut self.draft).file.clone();
                    events.push(SettingsEvent::PlaySound {
                        sound: selected,
                        file: (!file.trim().is_empty()).then_some(file),
                        beep,
                    });
                }
                if ui.button(tr(Text::BtnDefault, lang)).clicked() {
                    let default = SoundId::ALL
                        .iter()
                        .map(|&s| (s, s.setting(&mut Config::default()).clone()))
                        .collect::<Vec<_>>();
                    for (s, value) in default {
                        *s.setting(&mut self.draft) = value;
                    }
                    self.draft.sounds.mode = SoundMode::File;
                }
            });
        });
    }

    fn dialogs(&mut self, ui: &mut Ui, lang: Lang, events: &mut Vec<SettingsEvent>) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let mut keep = true;
        match &mut dialog {
            Dialog::Hotkey {
                action,
                text,
                capturing,
                error,
            } => {
                let action = *action;
                let mut start_capture = false;
                egui::Modal::new(egui::Id::new("hotkey_dialog")).show(&ctx, |ui| {
                    content_style(ui);
                    ui.set_width(380.0);
                    ui.heading(tr(Text::HotkeyDialogTitle, lang));
                    ui.label(RichText::new(tr(hotkey_action_text(action), lang)).strong());
                    ui.add_space(6.0);
                    if *capturing {
                        ui.label(tr(Text::HotkeyPressPrompt, lang));
                        ui.label(RichText::new(tr(Text::HotkeyWaiting, lang)).italics());
                    } else if ui.button(tr(Text::HotkeyPressAgain, lang)).clicked() {
                        start_capture = true;
                    }
                    ui.add_space(6.0);
                    ui.label(tr(Text::HotkeyTypeHint, lang));
                    ui.add(egui::TextEdit::singleline(text).desired_width(240.0));
                    let parsed = if text.trim().is_empty() {
                        Ok(HotkeyBinding::NONE)
                    } else {
                        text.parse::<Hotkey>().map(HotkeyBinding::some)
                    };
                    match &parsed {
                        Ok(binding) => {
                            if let Some(other) = binding.0.and_then(|h| {
                                self.draft
                                    .hotkeys
                                    .iter()
                                    .find(|(a, b)| *a != action && b.0 == Some(h))
                                    .map(|(a, _)| a)
                            }) {
                                ui.label(
                                    RichText::new(format!(
                                        "{} {}",
                                        tr(Text::HotkeyConflict, lang),
                                        tr(hotkey_action_text(other), lang)
                                    ))
                                    .color(Color32::from_rgb(0xC0, 0x60, 0x20)),
                                );
                            }
                        }
                        Err(e) => {
                            ui.label(
                                RichText::new(format!("{} {e}", tr(Text::HotkeyInvalid, lang)))
                                    .color(Color32::from_rgb(0xC0, 0x30, 0x30)),
                            );
                        }
                    }
                    if let Some(e) = error {
                        weak(ui, e);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(parsed.is_ok(), egui::Button::new(tr(Text::BtnOk, lang)))
                            .clicked()
                            && let Ok(binding) = parsed
                        {
                            *self.draft.hotkeys.get_mut(action) = binding;
                            keep = false;
                        }
                        if ui.button(tr(Text::BtnClear, lang)).clicked() {
                            text.clear();
                        }
                        if ui.button(tr(Text::BtnDefault, lang)).clicked() {
                            *text = okbs_core::config::Hotkeys::default()
                                .get(action)
                                .to_string();
                        }
                        if ui.button(tr(Text::BtnCancel, lang)).clicked() {
                            keep = false;
                        }
                    });
                });
                if start_capture {
                    *capturing = true;
                    events.push(SettingsEvent::CaptureHotkey);
                }
                if !keep && *capturing {
                    events.push(SettingsEvent::CancelCapture);
                }
            }
            Dialog::Rule {
                index,
                rule,
                suggestion,
            } => {
                egui::Modal::new(egui::Id::new("rule_dialog")).show(&ctx, |ui| {
                    content_style(ui);
                    ui.set_width(400.0);
                    ui.heading(tr(Text::RuleDialogTitle, lang));
                    if *suggestion {
                        ui.label(tr(Text::RuleSuggestion, lang));
                    }
                    ui.label(tr(Text::RulePattern, lang));
                    ui.text_edit_singleline(&mut rule.pattern);
                    weak(ui, tr(Text::RulePatternHint, lang));
                    ui.add_space(6.0);
                    for kind in [
                        MatchKind::Contains,
                        MatchKind::StartsWith,
                        MatchKind::Equals,
                    ] {
                        ui.radio_value(&mut rule.match_kind, kind, tr(match_kind_text(kind), lang));
                    }
                    ui.checkbox(&mut rule.case_sensitive, tr(Text::RuleCaseSensitive, lang));
                    ui.add_space(6.0);
                    for action in [RuleAction::Switch, RuleAction::Stay] {
                        ui.radio_value(
                            &mut rule.action,
                            action,
                            tr(rule_action_text(action), lang),
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let valid = !rule.pattern.trim().is_empty();
                        if ui
                            .add_enabled(valid, egui::Button::new(tr(Text::BtnOk, lang)))
                            .clicked()
                        {
                            let mut saved = rule.clone();
                            saved.pattern = saved.pattern.trim().to_string();
                            match index.filter(|&i| i < self.draft.rules.len()) {
                                Some(i) => self.draft.rules[i] = saved,
                                None => {
                                    self.draft.rules.push(saved);
                                    self.selected_rule = Some(self.draft.rules.len() - 1);
                                }
                            }
                            keep = false;
                        }
                        if ui.button(tr(Text::BtnCancel, lang)).clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::Exclusion { tab, index, value } => {
                egui::Modal::new(egui::Id::new("exclusion_dialog")).show(&ctx, |ui| {
                    content_style(ui);
                    ui.set_width(440.0);
                    ui.heading(tr(Text::ExclusionDialogTitle, lang));
                    ui.label(tr(
                        match tab {
                            ExclusionTab::Executable => Text::ExclusionExePrompt,
                            ExclusionTab::Title => Text::ExclusionTitlePrompt,
                            ExclusionTab::Folder => Text::ExclusionFolderPrompt,
                        },
                        lang,
                    ));
                    ui.add(egui::TextEdit::singleline(value).desired_width(420.0));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let valid = !value.trim().is_empty();
                        if ui
                            .add_enabled(valid, egui::Button::new(tr(Text::BtnOk, lang)))
                            .clicked()
                        {
                            let ex = &mut self.draft.exclusions;
                            let v = value.clone();
                            match (*tab, *index) {
                                (ExclusionTab::Executable, Some(i)) if i < ex.executables.len() => {
                                    ex.executables[i].path = v;
                                }
                                (ExclusionTab::Executable, _) => {
                                    ex.executables.push(ExecutableExclusion { path: v })
                                }
                                (ExclusionTab::Title, Some(i)) if i < ex.titles.len() => {
                                    ex.titles[i].contains = v
                                }
                                (ExclusionTab::Title, _) => {
                                    ex.titles.push(TitleExclusion { contains: v })
                                }
                                (ExclusionTab::Folder, Some(i)) if i < ex.folders.len() => {
                                    ex.folders[i].path = v
                                }
                                (ExclusionTab::Folder, _) => {
                                    ex.folders.push(FolderExclusion { path: v })
                                }
                            }
                            keep = false;
                        }
                        if ui.button(tr(Text::BtnCancel, lang)).clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::AutoReplace {
                index,
                item,
                remember_cursor,
            } => {
                egui::Modal::new(egui::Id::new("autoreplace_dialog")).show(&ctx, |ui| {
                    content_style(ui);
                    ui.set_width(440.0);
                    ui.heading(tr(Text::AutoreplaceDialogTitle, lang));
                    ui.label(tr(Text::ColumnReplaceWhat, lang));
                    ui.text_edit_singleline(&mut item.from);
                    ui.label(tr(Text::ColumnReplaceWith, lang));
                    let mut caret = None;
                    let mut editing = false;
                    egui::ScrollArea::vertical()
                        .id_salt("autoreplace_text")
                        .max_height(160.0)
                        .show(ui, |ui| {
                            let output = egui::TextEdit::multiline(&mut item.to)
                                .id(egui::Id::new("autoreplace_value"))
                                .desired_rows(4)
                                .desired_width(420.0)
                                .show(ui);
                            caret = output.cursor_range.map(|range| {
                                i32::try_from(range.primary.index.0).unwrap_or(i32::MAX)
                            });
                            editing = output.response.has_focus();
                        });
                    let toggled = ui
                        .checkbox(remember_cursor, tr(Text::OptRememberCursor, lang))
                        .changed();
                    let len = i32::try_from(item.to.chars().count()).unwrap_or(i32::MAX);
                    if *remember_cursor {
                        if (editing || toggled)
                            && let Some(caret) = caret
                        {
                            item.cursor_pos = caret.min(len);
                        }
                        if item.cursor_pos < 0 {
                            item.cursor_pos = len;
                        }
                        ui.horizontal(|ui| {
                            ui.label(tr(Text::AutoreplaceCursorAt, lang));
                            ui.add(egui::DragValue::new(&mut item.cursor_pos).range(0..=len));
                        });
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let valid = !item.from.trim().is_empty();
                        if ui
                            .add_enabled(valid, egui::Button::new(tr(Text::BtnOk, lang)))
                            .clicked()
                        {
                            let mut saved = item.clone();
                            saved.from = saved.from.trim().to_string();
                            if !*remember_cursor {
                                saved.cursor_pos = -1;
                            }
                            let items = &mut self.draft.autoreplace.items;
                            match index.filter(|&i| i < items.len()) {
                                Some(i) => items[i] = saved,
                                None => items.push(saved),
                            }
                            keep = false;
                        }
                        if ui.button(tr(Text::BtnCancel, lang)).clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::Elevation { failed } => {
                let failed = *failed;
                egui::Modal::new(egui::Id::new("elevation_dialog")).show(&ctx, |ui| {
                    content_style(ui);
                    ui.set_width(380.0);
                    ui.heading(tr(Text::ElevationTitle, lang));
                    ui.add_space(6.0);
                    if failed {
                        ui.label(
                            RichText::new(tr(Text::ElevationFailed, lang))
                                .color(Color32::from_rgb(0xB4, 0x2A, 0x2A)),
                        );
                        ui.add_space(6.0);
                    }
                    ui.label(tr(Text::ElevationRestart, lang));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(action_button(tr(Text::ElevationRestartNow, lang)))
                            .clicked()
                        {
                            events.push(SettingsEvent::RestartElevated);
                            keep = false;
                        }
                        if ui
                            .add(action_button(tr(Text::ElevationLater, lang)))
                            .clicked()
                        {
                            keep = false;
                        }
                    });
                });
            }
        }
        if keep {
            self.dialog = Some(dialog);
        }
    }
}

fn ui_language_label(preference: UiLanguage, system_language: Lang, lang: Lang) -> String {
    let native_name = |language| match language {
        Lang::Ru => tr(Text::LangRussian, Lang::Ru),
        Lang::En => tr(Text::LangEnglish, Lang::En),
    };
    match preference {
        UiLanguage::System => format!(
            "{} ({})",
            tr(Text::LangSystem, lang),
            native_name(system_language)
        ),
        _ => native_name(preference.resolve(system_language)).into(),
    }
}

#[cfg(test)]
#[path = "settings_preview.rs"]
mod preview;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> egui::Context {
        let ctx = egui::Context::default();
        configure_context(&ctx);
        ctx
    }

    fn window_input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                WINDOW_SIZE.into(),
            )),
            ..Default::default()
        }
    }

    fn frame(view: &mut SettingsView, ctx: &egui::Context) -> (Vec<SettingsEvent>, WindowAction) {
        let mut events = Vec::new();
        let mut action = WindowAction::Stay;
        let mut output = ctx.run_ui(window_input(), |ui| {
            action = view.ui(ui, &mut events);
        });
        output.textures_delta.clear();
        (events, action)
    }

    /// Presses and releases the primary button on the widget labelled `label`.
    fn click(view: &mut SettingsView, ctx: &egui::Context, label: &str) -> Vec<SettingsEvent> {
        let mut pos = None;
        for _ in 0..3 {
            let mut output = ctx.run_ui(window_input(), |ui| {
                view.ui(ui, &mut Vec::new());
            });
            output.textures_delta.clear();
            for shape in &output.shapes {
                if let egui::Shape::Text(t) = &shape.shape
                    && t.galley.text() == label
                {
                    pos = Some(t.galley.rect.translate(t.pos.to_vec2()).center());
                }
            }
        }
        let pos = pos.unwrap_or_else(|| panic!("{label:?} is not on screen"));
        let mut events = Vec::new();
        for pressed in [true, false] {
            let mut input = window_input();
            input.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ];
            let mut output = ctx.run_ui(input, |ui| {
                view.ui(ui, &mut events);
            });
            output.textures_delta.clear();
        }
        events
    }

    /// All text drawn at the real settings size and font, with positions.
    fn drawn_texts(view: &mut SettingsView, ctx: &egui::Context) -> Vec<(String, egui::Pos2)> {
        fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos)),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        let mut ok_y: Option<f32> = None;
        for frame in 0..60 {
            let input = window_input();
            let mut events = Vec::new();
            let mut output = ctx.run_ui(input, |ui| {
                view.ui(ui, &mut events);
            });
            output.textures_delta.clear();
            texts.clear();
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut texts);
            }
            let y = texts.iter().find(|(t, _)| t == "ОК").map(|(_, p)| p.y);
            if frame > 0 {
                assert_eq!(
                    y, ok_y,
                    "buttons must not move between frames (frame {frame})"
                );
            }
            ok_y = y;
        }
        texts
    }

    #[test]
    fn improvement_option_and_explanation_are_visible_and_applied() {
        for lang in Lang::ALL {
            let mut config = Config::default();
            config.general.ui_language = lang.into();
            let mut view = SettingsView::new(config, Section::Rules, lang);
            let texts = drawn_texts(&mut view, &test_context());
            assert!(
                texts
                    .iter()
                    .any(|(text, _)| text == tr(Text::OptImproveSwitching, lang))
            );
            assert!(
                texts
                    .iter()
                    .any(|(text, _)| text == tr(Text::ImproveSwitchingHint, lang))
            );
            assert!(!view.draft.rules_options.improve_switching);
            view.draft.rules_options.improve_switching = true;
            let mut events = Vec::new();
            view.apply(&mut events);
            assert!(events.iter().any(|event| matches!(event, SettingsEvent::Apply(config) if config.rules_options.improve_switching)));
        }
    }

    #[test]
    fn layout_has_sections_left_content_center_buttons_bottom() {
        let ctx = test_context();
        let mut view = SettingsView::new(Config::default(), Section::General, Lang::Ru);
        let texts = drawn_texts(&mut view, &ctx);
        let find = |text: &str| -> Vec<egui::Pos2> {
            texts
                .iter()
                .filter(|(t, _)| t == text)
                .map(|(_, p)| *p)
                .collect()
        };
        let general = find("Общие");
        assert_eq!(
            general.len(),
            2,
            "section list entry and heading: {texts:?}"
        );
        let (list, heading) = if general[0].x < general[1].x {
            (general[0], general[1])
        } else {
            (general[1], general[0])
        };
        assert!(
            list.x < 190.0 && list.y < 80.0,
            "section list at top left: {list:?}"
        );
        assert!(
            heading.x > 190.0 && heading.y < 80.0,
            "heading at top of content: {heading:?}"
        );
        let hotkeys = find("Горячие клавиши");
        assert!(
            hotkeys.first().is_some_and(|p| p.x < 190.0 && p.y > list.y),
            "{hotkeys:?}"
        );
        let autoswitch = find("Автопереключение");
        assert!(
            autoswitch
                .iter()
                .any(|p| p.x > 190.0 && p.y > heading.y && p.y < 480.0),
            "{autoswitch:?}"
        );
        let ok = find("ОК");
        assert!(
            ok.first().is_some_and(|p| p.y > 500.0 && p.x > 500.0),
            "OK button at bottom right: {ok:?}"
        );
    }

    #[test]
    fn every_section_renders_in_both_languages() {
        for lang in Lang::ALL {
            let mut config = Config::default();
            config.general.ui_language = lang.into();
            config.rules.push(Rule {
                pattern: "vs".into(),
                ..Rule::default()
            });
            config.autoreplace.items.push(AutoReplaceItem {
                from: "снп".into(),
                to: "С наилучшими\nпожеланиями".into(),
                cursor_pos: -1,
            });
            let ctx = test_context();
            let mut view = SettingsView::new(config, Section::General, Lang::Ru);
            for section in Section::ALL {
                view.section = section;
                let (events, action) = frame(&mut view, &ctx);
                assert!(events.is_empty());
                assert_eq!(action, WindowAction::Stay);
            }
            view.general_tab = GeneralTab::Advanced;
            view.section = Section::General;
            frame(&mut view, &ctx);
        }
    }

    /// Everything the window offers on Windows works; only the Linux port of
    /// «Запускаться при старте» is still missing and may carry the marker.
    #[test]
    fn no_option_is_marked_as_in_development_on_windows() {
        for lang in Lang::ALL {
            let mut config = Config::default();
            config.general.ui_language = lang.into();
            config.general.floating_indicator = true;
            config.general.run_elevated = true;
            let ctx = test_context();
            let mut view = SettingsView::new(config, Section::General, Lang::Ru);
            for section in Section::ALL {
                for tab in [GeneralTab::Basic, GeneralTab::Advanced] {
                    view.section = section;
                    view.general_tab = tab;
                    frame(&mut view, &ctx);
                    // The marker is drawn as a label of its own next to the option.
                    let marked: Vec<f32> = drawn_texts(&mut view, &ctx)
                        .iter()
                        .filter(|(text, _)| text.contains(tr(Text::InDevelopment, lang)))
                        .map(|(_, pos)| pos.y)
                        .collect();
                    // Options that only work on Windows keep the marker on the
                    // other platforms until their Linux port lands.
                    let pending = match (section, tab) {
                        _ if cfg!(windows) => 0,
                        // «Запускаться при старте».
                        (Section::General, GeneralTab::Basic) => 1,
                        // Caps Lock and the two clipboard history options.
                        (Section::General, GeneralTab::Advanced) => 3,
                        // «Показывать список в меню».
                        (Section::Autoreplace, _) => 1,
                        _ => 0,
                    };
                    assert_eq!(
                        marked.len(),
                        pending,
                        "unexpected markers in {section:?} {tab:?}: {marked:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_hotkey_action_shows_its_binding() {
        let ctx = test_context();
        let mut config = Config::default();
        for (action, _) in config.hotkeys.clone().iter() {
            *config.hotkeys.get_mut(action) =
                "Ctrl+Alt+F9".parse().map(HotkeyBinding::some).unwrap();
        }
        let mut view = SettingsView::new(config, Section::Hotkeys, Lang::Ru);
        let texts: Vec<String> = drawn_texts(&mut view, &ctx)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert!(
            texts.iter().filter(|text| *text == "Ctrl+Alt+F9").count() >= 3,
            "bindings of the scrolled-in actions must be shown: {texts:?}"
        );
    }

    #[test]
    fn diagnostics_checkbox_is_off_by_default_and_is_applied() {
        for lang in Lang::ALL {
            let ctx = test_context();
            let mut config = Config::default();
            config.general.ui_language = lang.into();
            let mut view = SettingsView::new(config, Section::Troubleshooting, Lang::Ru);
            assert!(!view.draft.log.debug);
            let shown: Vec<String> = drawn_texts(&mut view, &ctx)
                .into_iter()
                .map(|(text, _)| text)
                .collect();
            assert!(
                shown.iter().any(|t| t == tr(Text::OptLogDebug, lang)),
                "the option must be visible in {lang:?}: {shown:?}"
            );
            // The group sits at the end of a scrolling section, so the value
            // is set directly instead of clicking at a scrolled position.
            view.draft.log.debug = true;
            let mut events = Vec::new();
            view.apply(&mut events);
            assert!(
                matches!(events.as_slice(), [SettingsEvent::Apply(config)] if config.log.debug),
                "applying carries the option to the program"
            );
        }
    }

    #[test]
    fn elevation_dialog_offers_a_restart_or_closes() {
        for restart in [true, false] {
            let ctx = test_context();
            let mut view = SettingsView::new(Config::default(), Section::Hotkeys, Lang::Ru);
            view.handle(SettingsInput::ElevationRequired { failed: true });
            assert_eq!(view.section, Section::General);
            assert!(matches!(
                view.dialog,
                Some(Dialog::Elevation { failed: true })
            ));
            frame(&mut view, &ctx);
            let label = if restart {
                tr(Text::ElevationRestartNow, Lang::Ru)
            } else {
                tr(Text::ElevationLater, Lang::Ru)
            };
            let events = click(&mut view, &ctx, label);
            assert!(view.dialog.is_none(), "the dialog closes either way");
            assert_eq!(
                events.contains(&SettingsEvent::RestartElevated),
                restart,
                "only «{label}» asks for the restart"
            );
        }
    }

    #[test]
    fn dialogs_render_and_capture_flow() {
        let ctx = test_context();
        let mut view = SettingsView::new(Config::default(), Section::Hotkeys, Lang::Ru);
        view.dialog = Some(Dialog::Hotkey {
            action: HotkeyAction::CancelOrConvertLastWord,
            text: "Break".into(),
            capturing: true,
            error: None,
        });
        assert!(view.is_capturing());
        frame(&mut view, &ctx);
        view.handle(SettingsInput::HotkeyCaptured("Ctrl+F11".parse().unwrap()));
        assert!(!view.is_capturing());
        match &view.dialog {
            Some(Dialog::Hotkey { text, .. }) => assert_eq!(text, "Ctrl+F11"),
            other => panic!("{other:?}"),
        }

        view.handle(SettingsInput::SuggestRule(Rule {
            pattern: "ghbdtn".into(),
            match_kind: MatchKind::StartsWith,
            ..Rule::default()
        }));
        assert_eq!(view.section, Section::Rules);
        frame(&mut view, &ctx);

        for dialog in [
            Dialog::Exclusion {
                tab: ExclusionTab::Title,
                index: None,
                value: "Code".into(),
            },
            Dialog::AutoReplace {
                index: None,
                item: AutoReplaceItem::default(),
                remember_cursor: true,
            },
        ] {
            view.dialog = Some(dialog);
            frame(&mut view, &ctx);
        }
    }

    #[test]
    fn flag_settings_render_and_store_only_changes() {
        let ctx = test_context();
        let mut config = Config::default();
        config.general.tray_flags = true;
        let mut view = SettingsView::new(config, Section::General, Lang::Ru);
        view.set_layouts(vec![
            LayoutEntry {
                locale: "ru-RU".into(),
                name: "Русский (Россия)".into(),
            },
            LayoutEntry {
                locale: "en-US".into(),
                name: "Английский (США)".into(),
            },
            LayoutEntry {
                locale: "ja-JP".into(),
                name: "Японский".into(),
            },
        ]);
        let texts = drawn_texts(&mut view, &ctx);
        assert!(
            texts.iter().any(|(t, _)| t == "Английский (США)"),
            "{texts:?}"
        );
        assert!(texts.iter().any(|(t, _)| t == "США"));

        let flags = &mut view.draft.general.layout_flags;
        set_layout_flag(flags, "en-US", Flag::Gb);
        assert_eq!(flags.get("en-US").map(String::as_str), Some("gb"));
        set_layout_flag(flags, "en-US", Flag::Us);
        assert!(flags.is_empty(), "the region's own flag is not stored");
        set_layout_flag(flags, "ja-JP", Flag::Us);
        assert_eq!(flags.get("ja-JP").map(String::as_str), Some("us"));
    }

    #[test]
    fn apply_sanitizes_and_tracks_modification() {
        let mut view = SettingsView::new(Config::default(), Section::General, Lang::Ru);
        assert!(!view.is_modified());
        view.draft.general.autoswitch = false;
        view.draft.rules.push(Rule::default());
        assert!(view.is_modified());
        let mut events = Vec::new();
        view.apply(&mut events);
        assert!(!view.is_modified());
        match &events[..] {
            [SettingsEvent::Apply(config)] => {
                assert!(!config.general.autoswitch);
                assert!(config.rules.is_empty(), "empty rule removed by sanitize");
            }
            other => panic!("{other:?}"),
        }
        let mut changed = Config::default();
        changed.sounds.enabled = false;
        view.handle(SettingsInput::ConfigChanged(Box::new(changed)));
        assert!(!view.draft().sounds.enabled);
        assert!(!view.is_modified());
    }

    #[test]
    fn language_follows_the_system_unless_a_manual_choice_is_applied() {
        for system in Lang::ALL {
            let mut view = SettingsView::new(Config::default(), Section::General, system);
            assert_eq!(view.lang(), system);
            view.draft.general.ui_language = system.other().into();
            assert_eq!(view.lang(), system, "editing a draft must wait for Apply");
            view.apply(&mut Vec::new());
            assert_eq!(view.lang(), system.other());
            view.draft.general.ui_language = UiLanguage::System;
            let mut events = Vec::new();
            view.apply(&mut events);
            assert_eq!(view.lang(), system);
            assert!(
                matches!(events.as_slice(), [SettingsEvent::Apply(config)] if config.general.ui_language == UiLanguage::System)
            );
        }
    }

    #[test]
    fn sidebar_rows_match_the_sound_list_and_translate_without_reopening() {
        let ctx = test_context();
        let mut view = SettingsView::new(Config::default(), Section::Sounds, Lang::Ru);
        for lang in Lang::ALL {
            view.draft.general.ui_language = lang.into();
            view.apply(&mut Vec::new());
            for _ in 0..3 {
                let mut output = ctx.run_ui(window_input(), |ui| {
                    view.ui(ui, &mut Vec::new());
                });
                output.textures_delta.clear();
                let find = |label: &str, sidebar: bool| {
                    output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(t)
                                if t.galley.text() == label && (t.pos.x < 190.0) == sidebar =>
                            {
                                Some(t)
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("missing {label:?}"))
                };
                let sound_a = find(tr(Text::SoundEvAutoswitch, lang), false);
                let sound_b = find(tr(Text::SoundEvManualConvert, lang), false);
                let row_step = sound_b.pos.y - sound_a.pos.y;
                let mut previous: Option<f32> = None;
                for section in Section::ALL {
                    let t = find(tr(section.text(), lang), true);
                    assert_eq!(
                        t.galley.rows.len(),
                        1,
                        "sidebar label must not wrap: {section:?}"
                    );
                    assert!(t.pos.x + t.galley.rect.right() < 190.0);
                    if let Some(y) = previous {
                        assert!(
                            (t.pos.y - y - row_step).abs() < 1.0,
                            "sidebar spacing must match the right-hand list"
                        );
                    }
                    previous = Some(t.pos.y);
                }
            }
        }
    }

    #[test]
    fn long_text_stays_inside_the_fixed_window_in_all_sections() {
        fn check_shape(shape: &egui::Shape, clip: egui::Rect, section: Section) {
            match shape {
                egui::Shape::Text(t) => {
                    let rect = t.galley.rect.translate(t.pos.to_vec2());
                    if clip.intersects(rect) {
                        assert!(
                            rect.right() <= WINDOW_SIZE[0] - 4.0,
                            "{section:?}: text goes past the right edge: {:?} {rect:?}",
                            t.galley.text()
                        );
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        check_shape(shape, clip, section);
                    }
                }
                egui::Shape::Rect(r) if r.rect.left() >= 190.0 && clip.intersects(r.rect) => {
                    assert!(
                        r.rect.right() <= WINDOW_SIZE[0] + 1.0,
                        "{section:?}: control goes past the right edge: {:?}",
                        r.rect
                    );
                }
                _ => {}
            }
        }
        for lang in Lang::ALL {
            let mut config = Config::default();
            config.general.ui_language = lang.into();
            config.general.tray_flags = true;
            config.rules.push(Rule {
                pattern: "long_pattern".repeat(20),
                ..Default::default()
            });
            config.autoreplace.items.push(AutoReplaceItem {
                from: "abbreviation".repeat(20),
                to: "Длинный текст автозамены ".repeat(20),
                cursor_pos: -1,
            });
            config.exclusions.executables.push(ExecutableExclusion {
                path: format!("C:/{}.exe", "long_folder/".repeat(25)),
            });
            let mut view = SettingsView::new(config, Section::General, Lang::Ru);
            view.set_layouts(vec![LayoutEntry {
                locale: "en-US".into(),
                name: "English (United States, custom keyboard layout) ".repeat(4),
            }]);
            for section in Section::ALL {
                for advanced in [false, true] {
                    let ctx = test_context();
                    view.section = section;
                    view.general_tab = if advanced {
                        GeneralTab::Advanced
                    } else {
                        GeneralTab::Basic
                    };
                    for _ in 0..3 {
                        let mut output = ctx.run_ui(window_input(), |ui| {
                            view.ui(ui, &mut Vec::new());
                        });
                        output.textures_delta.clear();
                        for shape in &output.shapes {
                            check_shape(&shape.shape, shape.clip_rect, section);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn autoreplace_help_wraps_and_all_font_styles_grow_by_one() {
        let ctx = egui::Context::default();
        let before = [egui::Theme::Light, egui::Theme::Dark].map(|theme| ctx.style_of(theme));
        configure_context(&ctx);
        for (theme, old) in [egui::Theme::Light, egui::Theme::Dark]
            .into_iter()
            .zip(before)
        {
            for (name, font) in &old.text_styles {
                assert_eq!(ctx.style_of(theme).text_styles[name].size, font.size + 1.0);
            }
        }
        let mut view = SettingsView::new(Config::default(), Section::Autoreplace, Lang::Ru);
        for _ in 0..3 {
            let mut output = ctx.run_ui(window_input(), |ui| {
                view.ui(ui, &mut Vec::new());
            });
            output.textures_delta.clear();
            let hint = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t)
                        if t.galley.text() == tr(Text::AutoreplaceHint, Lang::Ru) =>
                    {
                        Some(t)
                    }
                    _ => None,
                })
                .expect("help must be visible");
            assert!(
                hint.galley.rows.len() > 1,
                "help must wrap to multiple lines"
            );
        }
    }

    #[test]
    fn short_labels_keep_one_line_and_columns_align_at_common_dpi_scales() {
        for lang in Lang::ALL {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for section in [Section::General, Section::Troubleshooting, Section::Hotkeys] {
                    let ctx = test_context();
                    ctx.set_pixels_per_point(scale);
                    let mut config = Config::default();
                    config.general.ui_language = lang.into();
                    config.general.tray_flags = true;
                    let mut view = SettingsView::new(config, section, Lang::Ru);
                    view.set_layouts(vec![
                        LayoutEntry {
                            locale: "en-US".into(),
                            name: "Английский (США)".into(),
                        },
                        LayoutEntry {
                            locale: "ru-RU".into(),
                            name: "Русский (Россия)".into(),
                        },
                    ]);
                    for frame in 0..4 {
                        let mut output = ctx.run_ui(window_input(), |ui| {
                            view.ui(ui, &mut Vec::new());
                        });
                        output.textures_delta.clear();
                        if frame == 0 {
                            continue;
                        }
                        let find = |label: &str| {
                            output
                                .shapes
                                .iter()
                                .find_map(|shape| match &shape.shape {
                                    egui::Shape::Text(t) if t.galley.text() == label => Some(t),
                                    _ => None,
                                })
                                .unwrap_or_else(|| {
                                    panic!("{section:?} {lang:?} scale={scale}: missing {label:?}")
                                })
                        };
                        let labels: Vec<_> = match section {
                            Section::General => vec![
                                "Английский (США)",
                                "Русский (Россия)",
                                tr(Text::OptUiLanguage, lang),
                                tr(Text::OptTheme, lang),
                            ],
                            Section::Troubleshooting => vec![
                                tr(Text::KeyBackspace, lang),
                                tr(Text::KeyArrowLeft, lang),
                                tr(Text::KeyArrowRight, lang),
                                tr(Text::KeyArrowUp, lang),
                            ],
                            Section::Hotkeys => vec![
                                tr(Text::ColumnAction, lang),
                                tr(Text::ColumnCombination, lang),
                                "Break",
                                "Shift+Break",
                            ],
                            _ => unreachable!(),
                        };
                        for label in &labels {
                            let t = find(label);
                            assert_eq!(
                                t.galley.rows.len(),
                                1,
                                "{section:?} scale={scale}: short label wrapped: {label:?}"
                            );
                        }
                        let left = |label| {
                            let t = find(label);
                            t.pos.x + t.galley.rect.left()
                        };
                        if section == Section::Troubleshooting {
                            for label in &labels[1..] {
                                assert!(
                                    (left(label) - left(labels[0])).abs() <= 1.5,
                                    "checkbox column must align"
                                );
                            }
                        } else if section == Section::General {
                            assert!(
                                (left(labels[0]) - left(labels[1])).abs() <= 1.5,
                                "layout names must align"
                            );
                            assert!(
                                (left(labels[2]) - left(labels[3])).abs() <= 1.5,
                                "form labels must align"
                            );
                        } else {
                            assert!(
                                left(labels[1]) > 570.0,
                                "hotkey table must use the right side of the pane"
                            );
                            assert!(
                                (left(labels[1]) - left("Break")).abs() <= 1.5,
                                "header must align with bindings"
                            );
                            let assign = find(tr(Text::BtnAssign, lang));
                            assert!(
                                assign.pos.y + assign.galley.rect.bottom() < WINDOW_SIZE[1] - 52.0,
                                "Assign must stay visible below the scrolling list"
                            );
                            for shape in &output.shapes {
                                if let egui::Shape::Text(t) = &shape.shape
                                    && t.galley.text().contains(tr(Text::InDevelopment, lang))
                                {
                                    assert_eq!(
                                        t.galley.rows.len(),
                                        1,
                                        "status must not split between rows"
                                    );
                                }
                            }
                        }
                        for label in [Text::BtnOk, Text::BtnCancel, Text::BtnApply] {
                            let t = find(tr(label, lang));
                            let text_rect = t.galley.rect.translate(t.pos.to_vec2());
                            assert!(
                                output.shapes.iter().any(|shape| match &shape.shape {
                                    egui::Shape::Rect(r) =>
                                        r.rect.contains_rect(text_rect)
                                            && (87.0..=150.0).contains(&r.rect.width())
                                            && (27.0..=40.0).contains(&r.rect.height()),
                                    _ => false,
                                }),
                                "bottom button must have a full-size hit area: {label:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn selected_text_is_saved_only_after_confirming_the_entry_and_applying_settings() {
        let ctx = test_context();
        let mut view = SettingsView::new(Config::default(), Section::General, Lang::Ru);
        view.draft.general.autoswitch = false;
        let selected = "Подпись\nВторая строка 👋";
        view.handle(SettingsInput::AddAutoreplace(selected.into()));
        assert_eq!(view.section, Section::Autoreplace);
        assert!(view.draft.autoreplace.items.is_empty());
        match view.dialog.as_mut() {
            Some(Dialog::AutoReplace { item, .. }) => {
                assert_eq!(item.to, selected);
                assert!(item.from.is_empty());
                item.from = "снп".into();
            }
            _ => panic!("new entry dialog expected"),
        }
        let mut button_pos = None;
        for _ in 0..3 {
            let mut output = ctx.run_ui(window_input(), |ui| {
                assert_eq!(view.ui(ui, &mut Vec::new()), WindowAction::Stay);
            });
            output.textures_delta.clear();
            for shape in &output.shapes {
                if let egui::Shape::Text(t) = &shape.shape
                    && t.galley.text() == tr(Text::BtnOk, Lang::Ru)
                {
                    button_pos = Some(t.galley.rect.translate(t.pos.to_vec2()).center());
                }
            }
        }
        let pos = button_pos.expect("dialog OK button");
        for pressed in [true, false] {
            let mut input = window_input();
            input.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ];
            let mut events = Vec::new();
            let mut output = ctx.run_ui(input, |ui| {
                view.ui(ui, &mut events);
            });
            output.textures_delta.clear();
            assert!(
                events.is_empty(),
                "dialog confirmation must not apply the config yet"
            );
        }
        assert!(view.dialog.is_none());
        assert_eq!(view.draft.autoreplace.items[0].to, selected);
        assert!(view.applied.autoreplace.items.is_empty());
        assert!(
            !view.draft.general.autoswitch,
            "other draft changes must survive"
        );
        let mut events = Vec::new();
        view.apply(&mut events);
        assert!(matches!(events.as_slice(), [SettingsEvent::Apply(config)]
            if config.autoreplace.items[0].from == "снп" && config.autoreplace.items[0].to == selected));
    }

    #[test]
    fn new_selection_waits_for_an_existing_dialog_without_losing_edits() {
        let mut view = SettingsView::new(Config::default(), Section::Rules, Lang::Ru);
        let rule = Rule {
            pattern: "unsaved".into(),
            ..Default::default()
        };
        view.handle(SettingsInput::SuggestRule(rule.clone()));
        let previous = view.dialog.clone();
        view.handle(SettingsInput::AddAutoreplace("selected text".into()));
        assert_eq!(view.dialog, previous);
        view.dialog = None; // Existing editor finished or was cancelled.
        view.open_pending_autoreplace();
        assert!(
            matches!(&view.dialog, Some(Dialog::AutoReplace { item, .. }) if item.to == "selected text")
        );
        assert!(view.pending_autoreplace.is_empty());
    }

    #[test]
    fn remembered_caret_tracks_the_position_in_the_replacement_editor() {
        let ctx = test_context();
        let mut view = SettingsView::new(Config::default(), Section::Autoreplace, Lang::Ru);
        view.dialog = Some(Dialog::AutoReplace {
            index: None,
            item: AutoReplaceItem {
                from: "tag".into(),
                to: "<b></b>".into(),
                cursor_pos: -1,
            },
            remember_cursor: true,
        });
        for _ in 0..3 {
            frame(&mut view, &ctx);
        }
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("autoreplace_value")));
        for key in [
            egui::Key::End,
            egui::Key::ArrowLeft,
            egui::Key::ArrowLeft,
            egui::Key::ArrowLeft,
            egui::Key::ArrowLeft,
        ] {
            let mut input = window_input();
            input.events = [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                })
                .collect();
            let mut output = ctx.run_ui(input, |ui| {
                view.ui(ui, &mut Vec::new());
            });
            output.textures_delta.clear();
        }
        assert!(matches!(
            view.dialog,
            Some(Dialog::AutoReplace {
                item: AutoReplaceItem { cursor_pos: 3, .. },
                ..
            })
        ));
    }

    #[test]
    fn long_selected_text_does_not_push_dialog_buttons_beyond_the_window() {
        let ctx = test_context();
        let mut view = SettingsView::new(Config::default(), Section::Autoreplace, Lang::Ru);
        view.handle(SettingsInput::AddAutoreplace(
            "Большой фрагмент текста\n".repeat(100),
        ));
        for frame in 0..3 {
            let mut output = ctx.run_ui(window_input(), |ui| {
                view.ui(ui, &mut Vec::new());
            });
            output.textures_delta.clear();
            // A newly opened Modal may spend its first frame measuring its contents.
            if frame == 0 {
                continue;
            }
            let buttons: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(t) if t.galley.text() == tr(Text::BtnCancel, Lang::Ru) => {
                        Some(t.galley.rect.translate(t.pos.to_vec2()))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                buttons.len(),
                2,
                "both main and dialog cancel buttons must be visible (frame {frame}, buttons {buttons:?})"
            );
            assert!(
                buttons
                    .iter()
                    .all(|r| r.top() >= 0.0 && r.bottom() < WINDOW_SIZE[1])
            );
        }
    }
}
