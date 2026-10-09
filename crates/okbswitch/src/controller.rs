//! Glue between the engine, the tray and the settings window. Runs on the main thread.

use crate::clipboard_history::History;
use crate::settings::Settings;
use crossbeam_channel::{Receiver, Sender, unbounded};
use okbs_core::config::{Config, Rule, TypedSpellcheckMode};
use okbs_engine::sounds::{self, Sound};
use okbs_engine::{Command, EngineHandle, Event, TextOp, UiRequest};
use okbs_platform::{
    AutoreplaceLabels, AutoreplaceUi, Autostart, Elevation, FileDialogs, FileRequest,
    FloatingIndicator, IndicatorEvent, IndicatorLabels, IndicatorState, InputTarget, LayoutInfo,
    SoundPlayer, SystemSettings, WindowControl,
};
use okbs_ui::clipboard_history::{
    ClipboardHistoryConfig, ClipboardHistoryLabels, ClipboardHistoryWindow, HistoryChoice,
};
use okbs_ui::icon::{ICON_SIZE, IconState};
use okbs_ui::settings::{
    DictionaryState, LayoutEntry, RulesTransfer, Section, SettingsEvent, SettingsInput, SoundId,
};
use okbs_ui::text_result::TextResult;
use okbs_ui::tray::{Tray, TrayCommand, TrayLayout, TrayState};
use okbs_ui::window::SettingsWindow;
use okbs_ui::{Text, tr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the icon stays amber after a possible typo.
const ALERT: Duration = Duration::from_millis(1200);

/// Name offered by «Экспорт...» of switching rules.
const RULES_FILE_NAME: &str = "okbswitch-rules.toml";

struct SpellingJob {
    id: u64,
    text: String,
    settings: okbs_core::config::Spellcheck,
    interactive: bool,
    target: Option<InputTarget>,
}

struct SpellingResult {
    job: SpellingJob,
    misspellings: Vec<okbs_core::spell::Misspelling>,
}

#[derive(Debug)]
struct DictionaryProvider {
    root: PathBuf,
    selected_english: Option<String>,
    selected_russian: Option<String>,
    english: Option<spellbook::Dictionary>,
    russian: Option<spellbook::Dictionary>,
}

impl DictionaryProvider {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            selected_english: None,
            selected_russian: None,
            english: None,
            russian: None,
        }
    }

    fn select(&mut self, english: Option<&str>, russian: Option<&str>) {
        if self.selected_english.as_deref() != english
            || (english.is_some() && self.english.is_none())
        {
            self.selected_english = english.map(str::to_string);
            self.english = english.and_then(|id| self.load(id));
        }
        if self.selected_russian.as_deref() != russian
            || (russian.is_some() && self.russian.is_none())
        {
            self.selected_russian = russian.map(str::to_string);
            self.russian = russian.and_then(|id| self.load(id));
        }
    }

    fn load(&self, id: &str) -> Option<spellbook::Dictionary> {
        crate::dictionaries::load(&self.root, crate::dictionaries::package(id)?).ok()
    }

    fn dictionary(&self, lang: okbs_core::Lang) -> &spellbook::Dictionary {
        match (lang, self.english.as_ref(), self.russian.as_ref()) {
            (okbs_core::Lang::En, Some(dictionary), _) => dictionary,
            (okbs_core::Lang::Ru, _, Some(dictionary)) => dictionary,
            _ => okbs_core::data::dictionary(lang),
        }
    }
}

/// One worker keeps dictionary suggestions off the input and tray threads.
fn spelling_worker(
    root: PathBuf,
) -> std::io::Result<(Sender<SpellingJob>, Receiver<SpellingResult>)> {
    let (requests, input) = unbounded::<SpellingJob>();
    let (output, results) = unbounded();
    std::thread::Builder::new()
        .name("okbs-spelling".into())
        .spawn(move || {
            let mut dictionaries = DictionaryProvider::new(root);
            while let Ok(mut job) = input.recv() {
                // Rapid repeated requests only need the most recent snapshot.
                for newer in input.try_iter() {
                    tracing::debug!(target: "okbs_spelling", skipped_job = job.id,
                        newer_job = newer.id, "queued check superseded");
                    job = newer;
                }
                let started = Instant::now();
                dictionaries.select(
                    job.settings.english_dictionary.as_deref(),
                    job.settings.russian_dictionary.as_deref(),
                );
                // Automatic correction compares every correction the
                // dictionary offers, however few suggestions are shown.
                let limit =
                    if job.interactive && job.settings.typed_mode == TypedSpellcheckMode::Auto {
                        usize::MAX
                    } else {
                        job.settings.max_suggestions as usize
                    };
                let misspellings = okbs_core::spell::check_text_with_words(
                    &job.text,
                    &job.settings.languages,
                    limit,
                    &job.settings.custom_words,
                    |lang| dictionaries.dictionary(lang),
                );
                tracing::debug!(target: "okbs_spelling", job = job.id,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    errors = misspellings.len(),
                    suggestions = misspellings.iter().map(|m| m.suggestions.len()).sum::<usize>(),
                    "dictionary check completed");
                if output.send(SpellingResult { job, misspellings }).is_err() {
                    break;
                }
            }
        })?;
    Ok((requests, results))
}

/// The correction applied without asking, see [`okbs_core::typo`]. A word
/// with several misspellings is left alone.
fn automatic_correction(misspellings: &[okbs_core::spell::Misspelling]) -> Option<String> {
    let [only] = misspellings else {
        return None;
    };
    let model = okbs_core::data::language_model(only.lang);
    okbs_core::typo::automatic_correction(
        &only.word,
        &only.suggestions,
        only.lang,
        &okbs_core::typo::POLICY,
        |word| model.rank(word),
    )
}

/// «Импорт...»: asks for a file and reads the switching rules in it.
fn import_rules(dialogs: &dyn FileDialogs, request: &FileRequest) -> RulesTransfer {
    let path = match dialogs.open(request) {
        Ok(Some(path)) => path,
        Ok(None) => return RulesTransfer::Cancelled,
        Err(err) => {
            tracing::warn!("cannot show the open file dialog: {err}");
            return RulesTransfer::Failed(err.to_string());
        }
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => return RulesTransfer::Failed(format!("{}: {err}", path.display())),
    };
    match okbs_core::config::rules_from_toml(&text) {
        Ok(imported) => {
            tracing::info!(
                rules = imported.rules.len(),
                skipped = imported.skipped,
                "switching rules imported"
            );
            RulesTransfer::Imported(imported)
        }
        Err(message) => RulesTransfer::Failed(format!("{}: {message}", path.display())),
    }
}

/// «Экспорт...»: asks where to save `rules` and writes them.
fn export_rules(dialogs: &dyn FileDialogs, request: &FileRequest, rules: &[Rule]) -> RulesTransfer {
    let path = match dialogs.save(request) {
        Ok(Some(path)) => path,
        Ok(None) => return RulesTransfer::Cancelled,
        Err(err) => {
            tracing::warn!("cannot show the save file dialog: {err}");
            return RulesTransfer::Failed(err.to_string());
        }
    };
    let written = okbs_core::config::rules_to_toml(rules)
        .map_err(|err| err.to_string())
        .and_then(|text| std::fs::write(&path, text).map_err(|err| err.to_string()));
    match written {
        Ok(()) => {
            tracing::info!(rules = rules.len(), "switching rules exported");
            RulesTransfer::Exported {
                count: rules.len(),
                path: path.display().to_string(),
            }
        }
        Err(message) => RulesTransfer::Failed(format!("{}: {message}", path.display())),
    }
}

/// Callback run after a configuration is applied.
pub type ApplyHook = Box<dyn Fn(&Config) -> okbs_platform::Result<()>>;

/// Lists installed keyboard layouts.
pub type LayoutLister = Box<dyn Fn() -> Vec<LayoutInfo>>;

pub type AutoreplaceUiFactory =
    Box<dyn FnOnce(&SettingsWindow) -> okbs_platform::Result<Box<dyn AutoreplaceUi>>>;

/// Creates the floating indicator on the thread that pumps messages.
pub type IndicatorFactory = Box<
    dyn FnOnce(
        &IndicatorState,
        IndicatorLabels,
    ) -> okbs_platform::Result<Box<dyn FloatingIndicator>>,
>;

/// Platform services used by the controller.
pub type PopupPosition = Box<dyn Fn(Option<InputTarget>) -> Option<[f32; 2]>>;
pub type PopupPlacement = Box<dyn Fn(&str, [f32; 2]) -> okbs_platform::Result<()>>;
#[derive(Default)]
pub struct PlatformHooks {
    /// Reuse the first-run UI's event loop rather than creating a second one.
    pub settings_window: Option<(SettingsWindow, Receiver<SettingsEvent>)>,
    pub cursor_position: Option<Box<dyn Fn() -> [f32; 2]>>,
    pub spelling_position: Option<PopupPosition>,
    pub popup_placement: Option<PopupPlacement>,
    pub autoreplace_ui: Option<AutoreplaceUiFactory>,
    /// Installed layouts, for the flag settings and the tray icon.
    pub list_layouts: Option<LayoutLister>,
    /// Sound output for «Проиграть» in the settings window.
    pub sound: Option<Box<dyn SoundPlayer>>,
    /// «Запускаться при старте».
    pub autostart: Option<Box<dyn Autostart>>,
    /// «Запускать с правами Администратора».
    pub elevation: Option<Box<dyn Elevation>>,
    /// «Свернуть активное окно» and «Развернуть/восстановить активное окно».
    pub window_control: Option<Box<dyn WindowControl>>,
    /// «Показывать плавающий индикатор».
    pub indicator: Option<IndicatorFactory>,
    /// Where «Сохранять историю буфера обмена после перезагрузки» writes.
    pub history_file: Option<PathBuf>,
    /// Called after a new configuration is applied (e.g. to update the hook filter).
    pub on_apply: Option<ApplyHook>,
    /// «Импорт...» and «Экспорт...» of switching rules.
    pub file_dialogs: Option<Arc<dyn FileDialogs>>,
    /// «Системные настройки клавиатуры» in the tray menu.
    pub system_settings: Option<Box<dyn SystemSettings>>,
}

/// The application state owned by the main thread.
pub struct Controller {
    cursor_position: Option<Box<dyn Fn() -> [f32; 2]>>,
    spelling_position: Option<PopupPosition>,
    popup_placement: Option<PopupPlacement>,
    autoreplace_ui: Option<Box<dyn AutoreplaceUi>>,
    settings: Settings,
    engine: EngineHandle,
    tray: Option<Tray>,
    use_tray: bool,
    state: TrayState,
    window: Option<SettingsWindow>,
    window_events: Receiver<SettingsEvent>,
    sound: Option<Box<dyn SoundPlayer>>,
    autostart: Option<Box<dyn Autostart>>,
    on_apply: Option<ApplyHook>,
    list_layouts: Option<LayoutLister>,
    layouts: Vec<LayoutInfo>,
    layout: Option<LayoutInfo>,
    alert_until: Option<Instant>,
    spelling: Option<(Sender<SpellingJob>, Receiver<SpellingResult>)>,
    spelling_id: u64,
    /// Typed-word checks up to this job lost their word before the result came.
    spelling_expired: u64,
    dictionary_root: PathBuf,
    system_language: okbs_core::Lang,
    elevation: Option<Box<dyn Elevation>>,
    window_control: Option<Box<dyn WindowControl>>,
    indicator: Option<Box<dyn FloatingIndicator>>,
    history: Option<ClipboardHistoryWindow>,
    history_entries: History,
    /// Application the entry picked in the history window goes back to.
    history_target: Option<InputTarget>,
    /// Set when the user confirmed a restart with administrator rights.
    restart_elevated: bool,
    file_dialogs: Option<Arc<dyn FileDialogs>>,
    system_settings: Option<Box<dyn SystemSettings>>,
}

/// Indicator settings taken from «Общие → Основные».
fn indicator_state(config: &Config) -> IndicatorState {
    IndicatorState {
        visible: config.general.floating_indicator,
        autohide: config.general.floating_indicator_autohide,
        autohide_ms: config.general.floating_indicator_autohide_ms,
        position: config.general.floating_indicator_pos,
        locked: config.general.floating_indicator_locked,
    }
}

/// Where a popup opened by a hotkey or the tray menu appears, in physical
/// pixels. Placement is refined by the window itself once it is on screen.
fn cursor_position() -> [f32; 2] {
    #[cfg(windows)]
    {
        okbs_platform_windows::focus::cursor_position()
    }
    #[cfg(not(windows))]
    {
        [24.0, 24.0]
    }
}

fn spelling_popup_position(_target: Option<InputTarget>) -> Option<[f32; 2]> {
    #[cfg(windows)]
    {
        use okbs_platform_windows::focus;
        _target
            .and_then(|target| {
                focus::caret_position(target).or_else(|| focus::input_target_position(target))
            })
            .or_else(focus::foreground_window_position)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Position of `layout` in the list the tray menu was built from.
fn layout_index(layouts: &[LayoutInfo], layout: Option<&LayoutInfo>) -> Option<usize> {
    let layout = layout?;
    layouts.iter().position(|l| l.id == layout.id)
}

fn indicator_labels(lang: okbs_core::Lang) -> IndicatorLabels {
    IndicatorLabels {
        title: tr(Text::AppName, lang).into(),
        lock: tr(Text::OptLockIndicator, lang).into(),
        settings: tr(Text::MenuSettings, lang).into(),
        hide: tr(Text::IndicatorHide, lang).into(),
    }
}

impl Controller {
    /// Creates the tray (unless disabled) and the settings window thread.
    pub fn new(
        mut settings: Settings,
        engine: EngineHandle,
        platform: PlatformHooks,
        layout: Option<LayoutInfo>,
        use_tray: bool,
    ) -> Self {
        let dictionary_root = settings
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("dictionaries");
        if let Some(autostart) = &platform.autostart {
            // Show the real state; the registry is only written when the user changes the option.
            match autostart.is_enabled() {
                Ok(on) => settings.config.general.autostart = on,
                Err(err) => tracing::warn!("cannot read the autostart state: {err}"),
            }
        }
        let layouts = platform
            .list_layouts
            .as_ref()
            .map(|list| list())
            .unwrap_or_default();
        let state = TrayState {
            icon: IconState::for_layout(
                &settings.config.general,
                layout.as_ref().map(|l| l.locale.as_str()),
                layout.as_ref().and_then(|l| l.lang),
                settings.config.general.autoswitch,
                false,
            ),
            sounds: settings.config.sounds.enabled,
            layout: layout_index(&layouts, layout.as_ref()),
        };
        let system_language = crate::locale::system_ui_language();
        let (window, window_events) = match platform.settings_window {
            Some((window, events)) => (Some(window), events),
            None => {
                let (events_tx, events) = unbounded();
                let window = SettingsWindow::spawn(events_tx, system_language)
                    .map_err(|err| {
                        tracing::error!("cannot start the settings window thread: {err}")
                    })
                    .ok();
                (window, events)
            }
        };
        let autoreplace_ui = platform.autoreplace_ui.and_then(|create| {
            let window = window.as_ref()?;
            create(window)
                .map_err(|err| tracing::warn!("autoreplace popup unavailable: {err}"))
                .ok()
        });
        let history = window.as_ref().map(SettingsWindow::clipboard_history);
        let mut history_entries = History::new(
            platform.history_file,
            settings.config.clipboard.history_size as usize,
        );
        if settings.config.advanced.clipboard_history_persist {
            history_entries.load();
        }
        let lang = settings.config.general.ui_language.resolve(system_language);
        let indicator = platform.indicator.and_then(|create| {
            create(&indicator_state(&settings.config), indicator_labels(lang))
                .map_err(|err| tracing::warn!("floating indicator unavailable: {err}"))
                .ok()
        });
        let mut controller = Self {
            cursor_position: platform.cursor_position,
            spelling_position: platform.spelling_position,
            popup_placement: platform.popup_placement,
            autoreplace_ui,
            settings,
            engine,
            tray: None,
            use_tray,
            state,
            window,
            window_events,
            sound: platform.sound,
            autostart: platform.autostart,
            on_apply: platform.on_apply,
            list_layouts: platform.list_layouts,
            layouts,
            layout,
            alert_until: None,
            spelling: spelling_worker(dictionary_root.clone())
                .map_err(|err| tracing::error!("cannot start spelling worker: {err}"))
                .ok(),
            spelling_id: 0,
            spelling_expired: 0,
            dictionary_root,
            system_language,
            elevation: platform.elevation,
            window_control: platform.window_control,
            indicator,
            history,
            history_entries,
            history_target: None,
            restart_elevated: false,
            file_dialogs: platform.file_dialogs,
            system_settings: platform.system_settings,
        };
        if let Some(window) = &controller.window {
            for package in crate::dictionaries::PACKAGES {
                window.send(SettingsInput::DictionaryState {
                    id: package.id.into(),
                    state: if crate::dictionaries::is_installed(
                        &controller.dictionary_root,
                        *package,
                    ) {
                        DictionaryState::Available
                    } else {
                        DictionaryState::Unavailable
                    },
                });
            }
        }
        controller.rebuild_tray();
        controller.configure_autoreplace_ui();
        controller.configure_history();
        controller.reconcile_autostart();
        controller.refresh_icon();
        controller
    }

    /// Recomputes the icon from the layout, the settings and the alert state.
    fn refresh_icon(&mut self) {
        self.refresh_icon_inner(false);
    }

    /// The same, after the layout changed: the indicator shows itself again
    /// for a moment when «Скрывать плавающий индикатор» is on.
    fn refresh_layout_icon(&mut self) {
        self.refresh_icon_inner(true);
    }

    fn refresh_icon_inner(&mut self, layout_changed: bool) {
        self.state.layout = layout_index(&self.layouts, self.layout.as_ref());
        let general = &self.settings.config.general;
        self.state.icon = IconState::for_layout(
            general,
            self.layout.as_ref().map(|l| l.locale.as_str()),
            self.layout.as_ref().and_then(|l| l.lang),
            general.autoswitch,
            self.alert_until.is_some(),
        );
        let picture = okbs_ui::icon::render(self.state.icon);
        if let Some(indicator) = &mut self.indicator
            && let Err(err) = indicator.set_icon(&picture, ICON_SIZE, layout_changed)
        {
            tracing::warn!("cannot draw the floating indicator: {err}");
        }
    }

    fn configure_indicator(&mut self) {
        let lang = self
            .settings
            .config
            .general
            .ui_language
            .resolve(self.system_language);
        let state = indicator_state(&self.settings.config);
        if let Some(indicator) = &mut self.indicator
            && let Err(err) = indicator.configure(&state, indicator_labels(lang))
        {
            tracing::warn!("cannot configure the floating indicator: {err}");
        }
    }

    /// Sends the current texts and labels to the history window.
    fn configure_history(&mut self) {
        let config = self.history_config();
        if let Some(history) = &self.history {
            history.configure(config);
        }
    }

    fn history_config(&self) -> ClipboardHistoryConfig {
        let lang = self
            .settings
            .config
            .general
            .ui_language
            .resolve(self.system_language);
        let empty = if self.settings.config.advanced.clipboard_history {
            Text::ClipboardHistoryEmpty
        } else {
            Text::ClipboardHistoryOff
        };
        ClipboardHistoryConfig {
            entries: self.history_entries.entries().to_vec(),
            labels: ClipboardHistoryLabels {
                title: tr(Text::ClipboardHistoryTitle, lang).into(),
                insert: tr(Text::BtnInsert, lang).into(),
                close: tr(Text::BtnClose, lang).into(),
                clear: tr(Text::BtnClear, lang).into(),
                empty: tr(empty, lang).into(),
                help: tr(Text::ClipboardHistoryHelp, lang).into(),
            },
            theme: self.settings.config.general.theme,
        }
    }

    fn show_history(&mut self, target: Option<InputTarget>) {
        self.history_target = target;
        let config = self.history_config();
        if let Some(history) = &self.history {
            let position = self
                .cursor_position
                .as_ref()
                .map_or_else(cursor_position, |position| position());
            if let Some(place) = &self.popup_placement {
                self.run_window_command(
                    "clipboard history placement",
                    place(&config.labels.title, position),
                );
            }
            history.show(config, position);
        }
    }

    fn run_window_command(&self, what: &str, result: okbs_platform::Result<()>) {
        match result {
            Ok(()) => tracing::debug!("{what}"),
            Err(err) => tracing::warn!("{what}: {err}"),
        }
    }

    fn layout_entries(&mut self) -> Vec<LayoutEntry> {
        if let Some(list) = &self.list_layouts {
            self.layouts = list();
        }
        let mut entries: Vec<LayoutEntry> = Vec::new();
        for layout in &self.layouts {
            if !entries.iter().any(|e| e.locale == layout.locale) {
                entries.push(LayoutEntry {
                    locale: layout.locale.clone(),
                    name: layout.name.clone(),
                });
            }
        }
        entries
    }

    fn rebuild_tray(&mut self) {
        self.tray = None;
        if !self.use_tray {
            return;
        }
        match Tray::new(
            self.state,
            self.settings
                .config
                .general
                .ui_language
                .resolve(self.system_language),
            &self.settings.config.autoreplace,
            &self
                .layouts
                .iter()
                .map(|layout| TrayLayout {
                    name: layout.name.clone(),
                    flag: okbs_ui::flags::flag_for_layout(
                        &self.settings.config.general.layout_flags,
                        &layout.locale,
                    ),
                })
                .collect::<Vec<_>>(),
        ) {
            Ok(tray) => self.tray = Some(tray),
            Err(err) => tracing::error!("{err}"),
        }
    }

    fn configure_autoreplace_ui(&mut self) {
        let lang = self
            .settings
            .config
            .general
            .ui_language
            .resolve(self.system_language);
        if let Some(popup) = &mut self.autoreplace_ui
            && let Err(err) = popup.configure(
                &self.settings.config.autoreplace,
                autoreplace_labels(lang),
                self.settings.config.general.theme,
            )
        {
            tracing::warn!("cannot configure autoreplace popup: {err}");
        }
    }

    /// Opens the settings window.
    pub fn open_settings(&mut self, section: Section) {
        let layouts = self.layout_entries();
        if let Some(window) = &self.window {
            window.open(&self.settings.config, section, None, layouts);
        }
    }

    /// First start: offers terminals and IDEs as excluded programs.
    pub fn suggest_exclusions(&mut self) {
        let layouts = self.layout_entries();
        if let Some(window) = &self.window {
            window.suggest_exclusions(&self.settings.config, layouts);
        }
    }

    /// «Импорт...» (`export` is `None`) and «Экспорт...» of switching rules.
    /// The file dialog waits for the user, so it runs on a thread of its own
    /// and the answer comes back to the window.
    fn transfer_rules(&self, export: Option<Vec<Rule>>) {
        let Some(window) = &self.window else {
            return;
        };
        let notifier = window.notifier();
        let Some(dialogs) = self.file_dialogs.clone() else {
            notifier.send(SettingsInput::RulesTransfer(RulesTransfer::Failed(
                "file dialogs are not available on this system".into(),
            )));
            return;
        };
        let lang = self
            .settings
            .config
            .general
            .ui_language
            .resolve(self.system_language);
        let title = if export.is_some() {
            Text::RulesExportTitle
        } else {
            Text::RulesImportTitle
        };
        let request = FileRequest {
            title: tr(title, lang).into(),
            kind: tr(Text::RulesFileKind, lang).into(),
            extension: "toml".into(),
            file_name: RULES_FILE_NAME.into(),
            owner: tr(Text::SettingsWindowTitle, lang).into(),
        };
        let answer = notifier.clone();
        let spawned = std::thread::Builder::new()
            .name("okbs-rules-file".into())
            .spawn(move || {
                let outcome = match export {
                    Some(rules) => export_rules(dialogs.as_ref(), &request, &rules),
                    None => import_rules(dialogs.as_ref(), &request),
                };
                answer.send(SettingsInput::RulesTransfer(outcome));
            });
        if let Err(err) = spawned {
            tracing::error!("cannot start the file dialog thread: {err}");
            notifier.send(SettingsInput::RulesTransfer(RulesTransfer::Failed(
                err.to_string(),
            )));
        }
    }

    fn play(&self, sound: SoundId, file: Option<String>, beep: bool) {
        let Some(player) = &self.sound else { return };
        let builtin = match sound {
            SoundId::Autoswitch => Sound::Autoswitch,
            SoundId::ManualConvert => Sound::ManualConvert,
            SoundId::LayoutChanged => Sound::LayoutChanged,
            SoundId::Cancel => Sound::Cancel,
            SoundId::Suspicious => Sound::Suspicious,
            SoundId::Autoreplace => Sound::Autoreplace,
            SoundId::CaseFixed => Sound::CaseFixed,
            SoundId::ClipboardConvert => Sound::ClipboardConvert,
            SoundId::Error => Sound::Error,
            SoundId::SpellingError => Sound::SpellingError,
            SoundId::SpellingCorrected => Sound::SpellingCorrected,
        };
        let result = if beep {
            player.beep()
        } else if let Some(file) = file {
            player.play_file(Path::new(&file))
        } else {
            player.play_wav(sounds::wav(builtin))
        };
        if let Err(err) = result {
            tracing::warn!("cannot play sound: {err}");
        }
    }

    fn notify_window(&self) {
        if let Some(window) = &self.window {
            window.send(SettingsInput::ConfigChanged(Box::new(
                self.settings.config.clone(),
            )));
        }
    }

    fn play_configured(&self, sound: SoundId) {
        let sounds = &self.settings.config.sounds;
        let event = match sound {
            SoundId::Autoswitch => &sounds.events.autoswitch,
            SoundId::ManualConvert => &sounds.events.manual_convert,
            SoundId::LayoutChanged => &sounds.events.layout_changed,
            SoundId::Cancel => &sounds.events.cancel,
            SoundId::Suspicious => &sounds.events.suspicious,
            SoundId::Autoreplace => &sounds.events.autoreplace,
            SoundId::CaseFixed => &sounds.events.case_fixed,
            SoundId::ClipboardConvert => &sounds.events.clipboard_convert,
            SoundId::Error => &sounds.events.error,
            SoundId::SpellingError => &sounds.events.spelling_error,
            SoundId::SpellingCorrected => &sounds.events.spelling_corrected,
        };
        if sounds.enabled && event.enabled {
            self.play(
                sound,
                (!event.file.is_empty()).then(|| event.file.clone()),
                matches!(sounds.mode, okbs_core::config::SoundMode::Beep),
            );
        }
    }

    fn handle_engine_event(&mut self, event: Event) -> bool {
        match event {
            Event::AutoreplaceHint(index) => {
                if let Some(popup) = &mut self.autoreplace_ui
                    && let Err(err) = popup.hint(index)
                {
                    tracing::warn!("cannot show autoreplace hint: {err}");
                }
            }
            Event::AutoreplaceList { toggle, target } => {
                if let Some(popup) = &mut self.autoreplace_ui
                    && let Err(err) = popup.show_list(toggle, target)
                {
                    tracing::warn!("cannot show autoreplace list: {err}");
                }
            }
            Event::ConfigApplied => {
                if let Some(popup) = &mut self.autoreplace_ui {
                    let _ = popup.hint(None);
                }
            }
            Event::Stopped => return false,
            Event::LayoutChanged(layout) => {
                // A layout added in the system after start appears in the menu.
                if layout_index(&self.layouts, layout.as_ref()).is_none()
                    && layout.is_some()
                    && let Some(list) = &self.list_layouts
                {
                    let layouts = list();
                    if layouts != self.layouts {
                        self.layouts = layouts;
                        self.rebuild_tray();
                    }
                }
                self.layout = layout;
                self.refresh_layout_icon();
            }
            Event::AutoswitchChanged(on) => {
                self.settings.update(|c| c.general.autoswitch = on);
                self.refresh_icon();
                self.notify_window();
            }
            Event::SoundsChanged(on) => {
                self.state.sounds = on;
                self.settings.update(|c| c.sounds.enabled = on);
                self.notify_window();
            }
            Event::Converted { to, .. } => {
                if self.layout.as_ref().and_then(|l| l.lang) != Some(to) {
                    self.layout = self.layouts.iter().find(|l| l.lang == Some(to)).cloned();
                    self.refresh_layout_icon();
                }
            }
            Event::Suspicious => {
                self.alert_until = Some(Instant::now() + ALERT);
                self.refresh_icon();
            }
            Event::Error(message) => tracing::warn!("{message}"),
            Event::SpellingCorrected => self.play_configured(SoundId::SpellingCorrected),
            Event::SpellingReplacementFinished {
                request_id,
                success,
            } => {
                tracing::debug!(target: "okbs_spelling", request_id, success, "replacement acknowledged by engine");
                if let Some(window) = &self.window {
                    window.send(SettingsInput::SpellingReplacementFinished {
                        request_id,
                        success,
                    });
                }
            }
            Event::SpellingExpired => {
                self.spelling_expired = self.spelling_id;
                if let Some(window) = &self.window {
                    window.send(SettingsInput::SpellingExpired);
                }
            }
            Event::SuggestRule(rule) => {
                let layouts = self.layout_entries();
                if let Some(window) = &self.window {
                    window.open(&self.settings.config, Section::Rules, Some(rule), layouts);
                }
            }
            Event::AutoreplaceSelection(text) => {
                let layouts = self.layout_entries();
                if let Some(window) = &self.window {
                    window.add_autoreplace(&self.settings.config, text, layouts);
                }
            }
            Event::HotkeyCaptured(hotkey) => {
                if let Some(window) = &self.window {
                    window.send(SettingsInput::HotkeyCaptured(hotkey));
                }
            }
            Event::CaptureCancelled => {
                if let Some(window) = &self.window {
                    window.send(SettingsInput::CaptureCancelled);
                }
            }
            Event::Ui(UiRequest::OpenSettings) => self.open_settings(Section::General),
            Event::Ui(UiRequest::OpenAutoreplaceSettings) => {
                self.open_settings(Section::Autoreplace)
            }
            Event::Ui(UiRequest::MinimizeWindow) => {
                if let Some(control) = &self.window_control {
                    self.run_window_command(
                        "minimize the active window",
                        control.minimize_active(),
                    );
                }
            }
            Event::Ui(UiRequest::ToggleMaximizeWindow) => {
                if let Some(control) = &self.window_control {
                    self.run_window_command(
                        "maximize or restore the active window",
                        control.toggle_maximize_active(),
                    );
                }
            }
            Event::ClipboardHistory { target } => self.show_history(target),
            // The payload is clipboard text: it is stored, never logged.
            Event::ClipboardText(text) => {
                if self.history_entries.push(text) {
                    self.persist_history();
                    if self.history.as_ref().is_some_and(|h| h.is_visible()) {
                        self.configure_history();
                    }
                }
            }
            Event::ClipboardConverted { result, .. } => {
                if self
                    .settings
                    .config
                    .advanced
                    .show_clipboard_conversion_window
                    && let Some(window) = &self.window
                {
                    window.show_text(&self.settings.config, TextResult::conversion(result));
                }
            }
            Event::CheckSpelling {
                text,
                settings,
                interactive,
                target,
            } => {
                self.spelling_id = self.spelling_id.wrapping_add(1);
                tracing::debug!(target: "okbs_spelling", job = self.spelling_id, ?target,
                    interactive, chars = text.chars().count(), mode = ?settings.typed_mode,
                    worker_available = self.spelling.is_some(), "check requested");
                if let Some((worker, _)) = &self.spelling {
                    let _ = worker.send(SpellingJob {
                        id: self.spelling_id,
                        text,
                        settings,
                        interactive,
                        target,
                    });
                }
            }
            // Events may contain clipboard/typed text; do not log Debug payloads.
            _ => {}
        }
        true
    }

    fn handle_window_event(&mut self, event: SettingsEvent) {
        match event {
            SettingsEvent::Apply(config) => {
                if let Some(on_apply) = &self.on_apply
                    && let Err(err) = on_apply(&config)
                {
                    tracing::warn!("configuration rejected by platform: {err}");
                    if let Some(window) = &self.window {
                        window.send(SettingsInput::ApplyRejected {
                            previous: Box::new(self.settings.config.clone()),
                            attempted: config,
                        });
                    }
                    return;
                }
                let backend_changed =
                    config.linux.layout_backend != self.settings.config.linux.layout_backend;
                let language_changed =
                    config.general.ui_language != self.settings.config.general.ui_language;
                let autoreplace_changed = config.autoreplace != self.settings.config.autoreplace;
                let flags_changed =
                    config.general.layout_flags != self.settings.config.general.layout_flags;
                let elevation_changed =
                    config.general.run_elevated != self.settings.config.general.run_elevated;
                if (config.general.autostart != self.settings.config.general.autostart
                    || elevation_changed)
                    && let Some(autostart) = &self.autostart
                {
                    let on = config.general.autostart;
                    let elevated = config.general.run_elevated;
                    let result = std::env::current_exe()
                        .map_err(okbs_platform::PlatformError::from)
                        .and_then(|exe| autostart.set_enabled(on, &exe, elevated));
                    match result {
                        Ok(()) => tracing::info!(on, elevated, "autostart changed"),
                        Err(err) => tracing::warn!("cannot change autostart: {err}"),
                    }
                }
                self.state.sounds = config.sounds.enabled;
                crate::logging::set_level(config.log.enabled, config.log.level);
                self.settings.replace((*config).clone());
                self.refresh_icon();
                self.engine.send(Command::ApplyConfig(config));
                tracing::info!(
                    typed_spelling = self.settings.config.spellcheck.check_typed_words,
                    spelling_on_command = self.settings.config.spellcheck.check_on_command,
                    spelling_mode = ?self.settings.config.spellcheck.typed_mode,
                    "settings applied"
                );
                self.configure_autoreplace_ui();
                self.configure_indicator();
                self.history_entries
                    .set_capacity(self.settings.config.clipboard.history_size as usize);
                if self.settings.config.advanced.clipboard_history_persist {
                    self.persist_history();
                } else if let Err(err) = self.history_entries.remove_file() {
                    // Switching the option off must not leave the texts on disk.
                    tracing::warn!("cannot delete the saved clipboard history: {err}");
                }
                self.configure_history();
                if language_changed || autoreplace_changed || flags_changed || backend_changed {
                    if let Some(list) = &self.list_layouts {
                        self.layouts = list();
                    }
                    self.rebuild_tray();
                }
                // Asking for the rights is the last step: the answer may end
                // this process, and the configuration is already saved.
                if elevation_changed && self.settings.config.general.run_elevated {
                    self.ask_for_elevation(false);
                }
            }
            SettingsEvent::RestartElevated => self.restart_elevated(),
            SettingsEvent::ImportRules => self.transfer_rules(None),
            SettingsEvent::ExportRules(rules) => self.transfer_rules(Some(rules)),
            SettingsEvent::SkipSpelling(word) => {
                self.engine.send(Command::DeclineSpelling(word));
            }
            SettingsEvent::AddSpellingWord(word) => {
                self.settings.update(|config| {
                    config.spellcheck.custom_words.push(word);
                    let _ = config.sanitize();
                });
                self.engine
                    .send(Command::ApplyConfig(Box::new(self.settings.config.clone())));
                self.notify_window();
            }
            SettingsEvent::CaptureHotkey => {
                self.engine.send(Command::CaptureHotkey);
            }
            SettingsEvent::LinuxSetup(_) => {}
            SettingsEvent::CancelCapture | SettingsEvent::Closed => {
                self.engine.send(Command::CancelCapture);
            }
            SettingsEvent::PlaySound { sound, file, beep } => self.play(sound, file, beep),
            SettingsEvent::DownloadDictionary(id) => {
                let Some(package) = crate::dictionaries::package(&id) else {
                    tracing::warn!(%id, "unknown dictionary package requested");
                    return;
                };
                let root = self.dictionary_root.clone();
                let notifier = self.window.as_ref().map(SettingsWindow::notifier);
                if let Some(notifier) = &notifier {
                    notifier.send(SettingsInput::DictionaryState {
                        id: id.clone(),
                        state: DictionaryState::Downloading,
                    });
                }
                std::thread::spawn(move || match crate::dictionaries::install(&root, package) {
                    Ok(()) => {
                        tracing::info!(package = package.id, "dictionary downloaded");
                        if let Some(notifier) = notifier {
                            notifier.send(SettingsInput::DictionaryState {
                                id: package.id.into(),
                                state: DictionaryState::Available,
                            });
                        }
                    }
                    Err(err) => {
                        tracing::warn!(package = package.id, "dictionary download failed: {err:#}");
                        if let Some(notifier) = notifier {
                            notifier.send(SettingsInput::DictionaryState {
                                id: package.id.into(),
                                state: DictionaryState::Failed,
                            });
                            notifier.send(SettingsInput::DictionaryError {
                                id: package.id.into(),
                                message: format!("{err:#}"),
                            });
                        }
                    }
                });
            }
            SettingsEvent::DeleteDictionary(id) => {
                let Some(package) = crate::dictionaries::package(&id) else {
                    tracing::warn!(%id, "unknown dictionary package requested for deletion");
                    return;
                };
                match crate::dictionaries::uninstall(&self.dictionary_root, package) {
                    Ok(()) => {
                        tracing::info!(package = package.id, "dictionary removed");
                        // Checks go back to the built-in dictionary at once.
                        let spellcheck = &self.settings.config.spellcheck;
                        if [
                            &spellcheck.english_dictionary,
                            &spellcheck.russian_dictionary,
                        ]
                        .into_iter()
                        .any(|selected| selected.as_deref() == Some(package.id))
                        {
                            self.settings.update(|config| {
                                for selected in [
                                    &mut config.spellcheck.english_dictionary,
                                    &mut config.spellcheck.russian_dictionary,
                                ] {
                                    if selected.as_deref() == Some(package.id) {
                                        *selected = None;
                                    }
                                }
                            });
                            self.engine
                                .send(Command::ApplyConfig(Box::new(self.settings.config.clone())));
                        }
                        if let Some(window) = &self.window {
                            window.send(SettingsInput::DictionaryState {
                                id,
                                state: DictionaryState::Unavailable,
                            });
                        }
                    }
                    Err(err) => {
                        tracing::error!(package = package.id, "dictionary removal failed: {err:#}");
                        if let Some(window) = &self.window {
                            window.send(SettingsInput::DictionaryError {
                                id,
                                message: format!("{err:#}"),
                            });
                        }
                    }
                }
            }
            SettingsEvent::ReplaceSpelling {
                request_id,
                target,
                original,
                corrected,
            } => {
                tracing::debug!(target: "okbs_spelling", request_id, ?target,
                    original_chars = original.chars().count(),
                    corrected_chars = corrected.chars().count(),
                    "forwarding popup replacement to engine");
                if !self.engine.send(Command::ReplaceTypedSpelling {
                    request_id,
                    target: InputTarget {
                        window: target.0,
                        control: target.1,
                    },
                    original,
                    corrected,
                }) && let Some(window) = &self.window
                {
                    window.send(SettingsInput::SpellingReplacementFinished {
                        request_id,
                        success: false,
                    });
                }
            }
        }
    }

    /// Shows the restart question unless the program is already elevated.
    fn ask_for_elevation(&mut self, failed: bool) {
        if self.elevation.as_ref().is_none_or(|e| e.is_elevated()) {
            return;
        }
        let layouts = self.layout_entries();
        if let Some(window) = &self.window {
            window.ask_elevation(&self.settings.config, failed, layouts);
        }
    }

    /// Moves the logon registration between the `Run` key and the task with
    /// the highest run level once the program has the rights to do it.
    fn reconcile_autostart(&mut self) {
        let (Some(autostart), Some(elevation)) = (&self.autostart, &self.elevation) else {
            return;
        };
        let general = &self.settings.config.general;
        if !general.autostart || !elevation.is_elevated() {
            return;
        }
        if autostart.elevated_at_login().unwrap_or(false) == general.run_elevated {
            return;
        }
        let elevated = general.run_elevated;
        let result = std::env::current_exe()
            .map_err(okbs_platform::PlatformError::from)
            .and_then(|exe| autostart.set_enabled(true, &exe, elevated));
        match result {
            Ok(()) => tracing::info!(elevated, "logon registration moved"),
            Err(err) => tracing::warn!("cannot move the logon registration: {err}"),
        }
    }

    /// Starts the program again with administrator rights and stops this one.
    fn restart_elevated(&mut self) {
        let Some(elevation) = &self.elevation else {
            return;
        };
        let arguments = vec![
            "--restarting".to_string(),
            "--config".to_string(),
            self.settings.path.display().to_string(),
        ];
        let result = std::env::current_exe()
            .map_err(okbs_platform::PlatformError::from)
            .and_then(|exe| elevation.restart_elevated(&exe, &arguments));
        match result {
            Ok(()) => {
                tracing::info!("restarting with administrator rights");
                self.restart_elevated = true;
            }
            Err(err) => {
                tracing::warn!("cannot restart with administrator rights: {err}");
                self.ask_for_elevation(true);
            }
        }
    }

    fn persist_history(&mut self) {
        if !self.settings.config.advanced.clipboard_history_persist {
            return;
        }
        if let Err(err) = self.history_entries.save() {
            tracing::warn!("cannot save the clipboard history: {err}");
        }
    }

    fn handle_indicator_event(&mut self, event: IndicatorEvent) {
        match event {
            IndicatorEvent::Moved(position) => {
                self.settings
                    .update(|c| c.general.floating_indicator_pos = Some(position));
                // An open settings window must not write the old position back.
                self.notify_window();
            }
            IndicatorEvent::Locked(locked) => {
                self.settings
                    .update(|c| c.general.floating_indicator_locked = locked);
                self.notify_window();
            }
            IndicatorEvent::Hidden => {
                self.settings
                    .update(|c| c.general.floating_indicator = false);
                self.configure_indicator();
                self.notify_window();
            }
            IndicatorEvent::OpenSettings => self.open_settings(Section::General),
        }
    }

    /// Processes pending events. Returns `false` when the program should exit.
    pub fn step(&mut self) -> bool {
        while let Ok(event) = self.engine.events().try_recv() {
            if !self.handle_engine_event(event) {
                return false;
            }
        }
        while let Ok(event) = self.window_events.try_recv() {
            self.handle_window_event(event);
        }
        if let Some(popup) = &mut self.autoreplace_ui {
            while let Some(insertion) = popup.poll() {
                self.engine.send(Command::InsertAutoreplace {
                    item: insertion.item,
                    target: insertion.target,
                });
            }
        }
        let mut indicator_events = Vec::new();
        if let Some(indicator) = &mut self.indicator {
            while let Some(event) = indicator.poll() {
                indicator_events.push(event);
            }
        }
        for event in indicator_events {
            self.handle_indicator_event(event);
        }
        let mut choices = Vec::new();
        if let Some(history) = &self.history {
            while let Some(choice) = history.poll() {
                choices.push(choice);
            }
        }
        for choice in choices {
            match choice {
                HistoryChoice::Insert(index) => {
                    if let Some(text) = self.history_entries.entries().get(index).cloned() {
                        self.engine.send(Command::InsertText {
                            text,
                            target: self.history_target,
                        });
                    }
                }
                HistoryChoice::Clear => {
                    self.history_entries.clear();
                    self.persist_history();
                    self.configure_history();
                }
            }
        }
        if self.restart_elevated {
            return false;
        }
        if let Some((_, results)) = &self.spelling {
            while let Ok(result) = results.try_recv() {
                let spellcheck = &self.settings.config.spellcheck;
                let enabled = if result.job.interactive {
                    spellcheck.check_typed_words
                } else {
                    spellcheck.check_on_command
                };
                let expired = result.job.interactive && result.job.id <= self.spelling_expired;
                if result.job.id != self.spelling_id || !enabled || expired {
                    tracing::debug!(target: "okbs_spelling", job = result.job.id,
                        latest_job = self.spelling_id, enabled, expired,
                        "check result discarded: superseded, disabled or expired");
                    continue;
                }
                if result.job.interactive && result.misspellings.is_empty() {
                    tracing::debug!(target: "okbs_spelling", job = result.job.id,
                        "typed check has no popup");
                    continue;
                }
                if result.job.interactive
                    && result.job.settings.typed_mode == TypedSpellcheckMode::Auto
                {
                    match (
                        result.job.target,
                        automatic_correction(&result.misspellings),
                    ) {
                        (Some(target), Some(corrected)) => {
                            tracing::debug!(target: "okbs_spelling", job = result.job.id, ?target,
                                "automatic replacement requested");
                            self.engine.send(Command::CorrectTypedSpelling {
                                target,
                                original: result.job.text,
                                corrected,
                            });
                        }
                        // Ambiguous: the word stays as typed, only the error is announced.
                        _ => self.play_configured(SoundId::SpellingError),
                    }
                    continue;
                }
                if result.job.interactive {
                    self.play_configured(SoundId::SpellingError);
                }
                if result.job.interactive || result.job.settings.show_result_window {
                    if let Some(window) = &self.window {
                        let view = if result.job.interactive {
                            TextResult::spelling_popup(result.job.text, result.misspellings)
                        } else {
                            TextResult::spelling(result.job.text, result.misspellings)
                        };
                        if result.job.interactive {
                            tracing::debug!(target: "okbs_spelling", job = result.job.id,
                                target = ?result.job.target, "opening suggestion popup");
                            window.show_text_passive(
                                &self.settings.config,
                                view,
                                self.spelling_position.as_ref().map_or_else(
                                    || spelling_popup_position(result.job.target),
                                    |position| position(result.job.target),
                                ),
                                result
                                    .job
                                    .target
                                    .map(|target| (target.window, target.control)),
                            );
                        } else {
                            window.show_text(&self.settings.config, view);
                        }
                    }
                } else {
                    let corrected = okbs_core::spell::apply_first_suggestions(
                        &result.job.text,
                        &result.misspellings,
                    );
                    self.engine.send(Command::CorrectClipboard {
                        original: result.job.text,
                        corrected,
                    });
                }
            }
        }
        if self.alert_until.is_some_and(|t| Instant::now() >= t) {
            self.alert_until = None;
            self.refresh_icon();
        }
        let mut commands = Vec::new();
        if let Some(tray) = &mut self.tray {
            while let Some(command) = tray.poll() {
                commands.push(command);
            }
        }
        for command in commands {
            match command {
                TrayCommand::Exit => return false,
                TrayCommand::OpenSettings => self.open_settings(Section::General),
                TrayCommand::ToggleAutoswitch => {
                    self.engine.send(Command::ToggleAutoswitch);
                }
                TrayCommand::ToggleSounds => {
                    self.engine.send(Command::SetSounds(!self.state.sounds));
                }
                TrayCommand::SelectLayout(index) => {
                    if let Some(layout) = self.layouts.get(index) {
                        self.engine.send(Command::SelectLayout(layout.id));
                    }
                }
                TrayCommand::ClipboardLayout => {
                    self.engine.send(Command::ClipboardOp(TextOp::Layout));
                }
                TrayCommand::ClipboardTransliterate => {
                    self.engine
                        .send(Command::ClipboardOp(TextOp::Transliterate));
                }
                TrayCommand::ClipboardSpellcheck => {
                    self.engine.send(Command::SpellcheckClipboard);
                }
                TrayCommand::ClipboardHistory => self.show_history(None),
                TrayCommand::KeyboardSettings => {
                    if let Some(system) = &self.system_settings
                        && let Err(err) = system.open_keyboard_settings()
                    {
                        tracing::warn!("cannot open the keyboard settings: {err}");
                    }
                }
                TrayCommand::About => self.open_settings(Section::About),
                TrayCommand::AutoreplaceMenu => {
                    self.engine.send(Command::AutoreplaceList { toggle: false });
                }
                TrayCommand::AutoreplaceList => {
                    self.engine.send(Command::AutoreplaceList { toggle: true });
                }
                TrayCommand::InsertAutoreplace(index) => {
                    if let Some(item) = self.settings.config.autoreplace.items.get(index) {
                        self.engine.send(Command::InsertAutoreplace {
                            item: item.clone(),
                            target: None,
                        });
                    }
                }
            }
        }
        if let Some(tray) = self.tray.as_mut()
            && let Err(err) = tray.update(self.state)
        {
            tracing::warn!("{err}");
        }
        true
    }

    /// Closes the window and the tray, then stops the engine.
    pub fn shutdown(mut self) {
        self.autoreplace_ui = None;
        self.indicator = None;
        self.history = None;
        self.window = None;
        self.tray = None;
        self.engine.shutdown();
    }
}

pub fn autoreplace_labels(lang: okbs_core::Lang) -> AutoreplaceLabels {
    AutoreplaceLabels {
        title: tr(Text::MenuAutoreplaceList, lang).into(),
        insert: tr(Text::BtnInsert, lang).into(),
        close: tr(Text::BtnClose, lang).into(),
        empty: tr(Text::AutoreplaceEmpty, lang).into(),
        disabled: tr(Text::AutoreplaceDisabled, lang).into(),
        hint_help: tr(Text::AutoreplaceHintHelp, lang).into(),
        menu_help: tr(Text::AutoreplaceMenuHelp, lang).into(),
        list_help: tr(Text::AutoreplaceListHelp, lang).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_worker_checks_the_snapshot_with_requested_languages() {
        let root = tempfile::tempdir().unwrap();
        let (requests, results) = spelling_worker(root.path().to_path_buf()).unwrap();
        requests
            .send(SpellingJob {
                id: 17,
                text: "Превет, wrold!".into(),
                interactive: false,
                target: None,
                settings: okbs_core::config::Spellcheck {
                    languages: vec![okbs_core::Lang::En],
                    max_suggestions: 3,
                    ..Default::default()
                },
            })
            .unwrap();
        let result = results.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(result.job.id, 17);
        assert_eq!(result.job.text, "Превет, wrold!");
        assert_eq!(result.misspellings.len(), 1);
        assert_eq!(result.misspellings[0].word, "wrold");
        assert!(result.misspellings[0].suggestions.len() <= 3);
        assert!(
            result.misspellings[0]
                .suggestions
                .iter()
                .any(|s| s == "world")
        );
    }

    #[test]
    fn automatic_correction_is_not_the_first_of_several_suggestions() {
        let root = tempfile::tempdir().unwrap();
        let (requests, results) = spelling_worker(root.path().to_path_buf()).unwrap();
        requests
            .send(SpellingJob {
                id: 1,
                text: "поши".into(),
                interactive: true,
                target: None,
                settings: okbs_core::config::Spellcheck {
                    typed_mode: TypedSpellcheckMode::Auto,
                    max_suggestions: 1,
                    ..Default::default()
                },
            })
            .unwrap();
        let result = results.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(
            result.misspellings[0].suggestions.len() > 1,
            "{:?}",
            result.misspellings
        );
        assert_eq!(automatic_correction(&result.misspellings), None);

        let misspelling = |suggestions: &[&str]| okbs_core::spell::Misspelling {
            range: 0..5,
            word: "wrold".into(),
            lang: okbs_core::Lang::En,
            suggestions: suggestions.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(
            automatic_correction(&[misspelling(&["world", "wold"])]).as_deref(),
            Some("world")
        );
        assert_eq!(automatic_correction(&[misspelling(&[])]), None);
        let two = [misspelling(&["world"]), misspelling(&["world"])];
        assert_eq!(automatic_correction(&two), None);
    }

    /// Answers every dialog with the same path, or cancels.
    #[test]
    fn backend_rejection_precedes_autostart_persistence_and_engine_changes() {
        use okbs_platform::{Injector, KeyStroke, LayoutId, LayoutManager};
        struct QuietInjector;
        impl Injector for QuietInjector {
            fn send(&mut self, _: &[KeyStroke]) -> okbs_platform::Result<()> {
                Ok(())
            }
        }
        struct QuietLayouts;
        impl LayoutManager for QuietLayouts {
            fn layouts(&self) -> okbs_platform::Result<Vec<LayoutInfo>> {
                Ok(Vec::new())
            }
            fn current(&self) -> okbs_platform::Result<LayoutId> {
                Ok(LayoutId(0))
            }
            fn set(&mut self, _: LayoutId) -> okbs_platform::Result<()> {
                Ok(())
            }
            fn keymap(&self, _: LayoutId) -> okbs_platform::Result<okbs_core::KeyMap> {
                Err(okbs_platform::PlatformError::Unsupported(
                    "test has no keyboard",
                ))
            }
        }
        struct Login(Arc<std::sync::atomic::AtomicUsize>);
        impl Autostart for Login {
            fn is_enabled(&self) -> okbs_platform::Result<bool> {
                Ok(true)
            }
            fn set_enabled(&self, _: bool, _: &Path, _: bool) -> okbs_platform::Result<()> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let config = Config::default();
        okbs_core::config::save(&path, &config).unwrap();
        let before = std::fs::read(&path).unwrap();
        let (_sender, input) = unbounded();
        let processor = okbs_engine::Processor::new(
            config.clone(),
            okbs_engine::Backends {
                injector: Box::new(QuietInjector),
                layouts: Box::new(QuietLayouts),
                clipboard: None,
                sound: None,
                focus: None,
            },
        );
        let engine = EngineHandle::spawn(
            processor,
            okbs_engine::Inputs {
                input,
                layout: None,
                focus: None,
                clipboard: None,
            },
        )
        .unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut controller = Controller::new(
            Settings {
                config: config.clone(),
                path: path.clone(),
                read_only: false,
            },
            engine,
            PlatformHooks {
                autostart: Some(Box::new(Login(calls.clone()))),
                on_apply: Some(Box::new(|_| {
                    Err(okbs_platform::PlatformError::Unsupported(
                        "rejected fixture",
                    ))
                })),
                ..PlatformHooks::default()
            },
            None,
            false,
        );
        let mut attempted = config.clone();
        attempted.general.autostart = false;
        attempted.general.autoswitch = false;
        attempted.linux.layout_backend = okbs_core::config::LayoutBackend::Kde;
        controller.handle_window_event(SettingsEvent::Apply(Box::new(attempted)));
        assert_eq!(controller.settings.config, config);
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(
            !controller
                .engine
                .events()
                .try_iter()
                .any(|event| matches!(event, Event::ConfigApplied))
        );
        controller.shutdown();
    }

    /// Answers every dialog with the same path, or cancels.
    struct FixedDialog(Option<PathBuf>);

    impl FileDialogs for FixedDialog {
        fn open(&self, _: &FileRequest) -> okbs_platform::Result<Option<PathBuf>> {
            Ok(self.0.clone())
        }
        fn save(&self, _: &FileRequest) -> okbs_platform::Result<Option<PathBuf>> {
            Ok(self.0.clone())
        }
    }

    fn request() -> FileRequest {
        FileRequest {
            title: "Export".into(),
            kind: "Rules".into(),
            extension: "toml".into(),
            file_name: RULES_FILE_NAME.into(),
            owner: "Settings".into(),
        }
    }

    #[test]
    fn exported_rules_come_back_from_the_chosen_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RULES_FILE_NAME);
        let dialog = FixedDialog(Some(path.clone()));
        let rules = vec![Rule {
            pattern: "ghb".into(),
            ..Rule::default()
        }];
        assert_eq!(
            export_rules(&dialog, &request(), &rules),
            RulesTransfer::Exported {
                count: 1,
                path: path.display().to_string(),
            }
        );
        let RulesTransfer::Imported(imported) = import_rules(&dialog, &request()) else {
            panic!("rules must be read back");
        };
        assert_eq!(imported.rules, rules);

        std::fs::write(&path, "not = [valid").unwrap();
        assert!(matches!(
            import_rules(&dialog, &request()),
            RulesTransfer::Failed(message) if message.contains(RULES_FILE_NAME)
        ));
        let cancelled = FixedDialog(None);
        assert_eq!(
            import_rules(&cancelled, &request()),
            RulesTransfer::Cancelled
        );
        assert_eq!(
            export_rules(&cancelled, &request(), &rules),
            RulesTransfer::Cancelled
        );
    }
}
