//! Settings, text results, the insertion list and the clipboard history share
//! one GUI event loop. Closing a window hides its viewport; only Shutdown
//! exits the UI thread.

use crate::autoreplace_list::{AutoreplaceListWindow, ListRequest, ListView};
use crate::clipboard_history::{ClipboardHistoryWindow, HistoryRequest, HistoryView};
use crate::i18n::{Text, tr};
use crate::settings::{
    LayoutEntry, Section, SettingsEvent, SettingsInput, SettingsView, WINDOW_SIZE, WindowAction,
};
use crate::text_result::TextResult;
use crossbeam_channel::{Receiver, Sender, unbounded};
use okbs_core::Lang;
use okbs_core::config::{Config, Rule, Theme, UiLanguage};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub(crate) enum Request {
    Open(
        Box<Config>,
        Section,
        Option<SettingsInput>,
        Vec<LayoutEntry>,
    ),
    OpenText(
        Box<Config>,
        TextResult,
        bool,
        Option<[f32; 2]>,
        Option<(u64, u64)>,
    ),
    Input(SettingsInput),
    List(ListRequest),
    History(HistoryRequest),
    Shutdown,
}

/// Which window owns the root viewport, that is which one started the event
/// loop. The other two are shown as deferred viewports of the same loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Root {
    Settings,
    List,
    History,
}

#[derive(Default)]
pub(crate) struct Shared {
    context: Mutex<Option<egui::Context>>,
    dictionary_states: Mutex<std::collections::BTreeMap<String, SettingsInput>>,
    open: AtomicBool,
    shutdown: AtomicBool,
}

impl Shared {
    fn restore_dictionary_state(&self, view: &mut SettingsView) {
        if let Ok(states) = self.dictionary_states.lock() {
            for input in states.values().cloned() {
                view.handle(input);
            }
        }
    }
    pub(crate) fn wake(&self) {
        if let Ok(slot) = self.context.lock()
            && let Some(ctx) = slot.as_ref()
        {
            ctx.request_repaint();
        }
    }
}

/// Handle of the settings window thread.
pub struct SettingsWindow {
    requests: Sender<Request>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct SettingsNotifier {
    requests: Sender<Request>,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for SettingsNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsNotifier").finish_non_exhaustive()
    }
}

impl SettingsNotifier {
    pub fn send(&self, input: SettingsInput) {
        if let SettingsInput::DictionaryState { id, .. } = &input
            && let Ok(mut states) = self.shared.dictionary_states.lock()
        {
            states.insert(id.clone(), input.clone());
        }
        let _ = self.requests.send(Request::Input(input));
        self.shared.wake();
    }
}

impl std::fmt::Debug for SettingsWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsWindow")
            .field("open", &self.is_open())
            .finish_non_exhaustive()
    }
}

struct App {
    view: SettingsView,
    requests: Receiver<Request>,
    events: Sender<SettingsEvent>,
    shared: Arc<Shared>,
    theme: Theme,
    lang: Lang,
    system_language: Lang,
    settings_visible: bool,
    text_result: Option<TextResult>,
    text_result_position: Option<[f32; 2]>,
    spellcheck_result: Option<TextResult>,
    spellcheck_position: Option<[f32; 2]>,
    spellcheck_target: Option<(u64, u64)>,
    spellcheck_pending: Option<u64>,
    root: Root,
    list: ListView,
    history: HistoryView,
}

impl App {
    fn id_of(&self, window: Root) -> egui::ViewportId {
        if self.root == window {
            return egui::ViewportId::ROOT;
        }
        match window {
            Root::Settings => egui::ViewportId::from_hash_of("settings"),
            Root::List => egui::ViewportId::from_hash_of("autoreplace_list"),
            Root::History => egui::ViewportId::from_hash_of("clipboard_history"),
        }
    }

    fn settings_id(&self) -> egui::ViewportId {
        self.id_of(Root::Settings)
    }

    fn list_id(&self) -> egui::ViewportId {
        self.id_of(Root::List)
    }

    fn history_id(&self) -> egui::ViewportId {
        self.id_of(Root::History)
    }

    fn spellcheck_id(&self) -> egui::ViewportId {
        egui::ViewportId::from_hash_of("spellcheck-popup")
    }

    fn show_settings(&self, ctx: &egui::Context) {
        self.shared.open.store(true, Ordering::SeqCst);
        for command in [
            egui::ViewportCommand::Visible(true),
            egui::ViewportCommand::Minimized(false),
            egui::ViewportCommand::Focus,
        ] {
            ctx.send_viewport_cmd_to(self.settings_id(), command);
        }
        ctx.request_repaint();
    }

    fn close_settings(&mut self, ctx: &egui::Context) {
        self.shared.open.store(false, Ordering::SeqCst);
        self.settings_visible = false;
        self.text_result = None;
        ctx.send_viewport_cmd_to(self.settings_id(), egui::ViewportCommand::Visible(false));
        ctx.send_viewport_cmd_to(self.settings_id(), egui::ViewportCommand::CancelClose);
        let _ = self.events.send(SettingsEvent::Closed);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    fn set_appearance(&mut self, ctx: &egui::Context, language: UiLanguage, theme: Theme) {
        self.lang = language.resolve(self.system_language);
        if theme != self.theme {
            self.theme = theme;
            crate::appearance::apply_theme(ctx, theme);
        }
        let title = if self.settings_visible {
            tr(Text::SettingsWindowTitle, self.lang)
        } else {
            self.text_result
                .as_ref()
                .map_or(tr(Text::SettingsWindowTitle, self.lang), |result| {
                    result.title(self.lang)
                })
        };
        ctx.send_viewport_cmd_to(
            self.settings_id(),
            egui::ViewportCommand::Title(title.into()),
        );
        ctx.request_repaint();
    }
}

impl App {
    fn process_requests(&mut self, ctx: &egui::Context) {
        if self.shared.shutdown.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        while let Ok(request) = self.requests.try_recv() {
            match request {
                Request::Open(config, section, input, layouts) => {
                    let (language, theme) = (config.general.ui_language, config.general.theme);
                    if !self.settings_visible {
                        self.view = SettingsView::new(*config, section, self.system_language);
                        self.shared.restore_dictionary_state(&mut self.view);
                    }
                    self.settings_visible = true;
                    self.view.open_section(section);
                    self.view.set_layouts(layouts);
                    self.set_appearance(ctx, language, theme);
                    self.show_settings(ctx);
                    if let Some(input) = input {
                        self.view.handle(input);
                    }
                }
                Request::OpenText(config, result, focus, position, target) => {
                    self.set_appearance(ctx, config.general.ui_language, config.general.theme);
                    if focus {
                        self.text_result = Some(result);
                        self.text_result_position = position;
                        self.show_settings(ctx);
                    } else {
                        self.spellcheck_result = Some(result);
                        self.spellcheck_position = position;
                        self.spellcheck_target = target;
                        self.spellcheck_pending = None;
                        #[cfg(windows)]
                        if let Some(result) = &self.spellcheck_result {
                            crate::window_position::show_inactive(result.title(self.lang));
                        }
                        ctx.send_viewport_cmd_to(
                            self.spellcheck_id(),
                            egui::ViewportCommand::Visible(true),
                        );
                        ctx.request_repaint_of(self.spellcheck_id());
                    }
                }
                Request::Input(input) => {
                    if let SettingsInput::SpellingReplacementFinished {
                        request_id,
                        success,
                    } = &input
                    {
                        self.finish_spelling_replacement(ctx, *request_id, *success);
                    }
                    if let SettingsInput::ConfigChanged(config) = &input {
                        self.set_appearance(ctx, config.general.ui_language, config.general.theme);
                    }
                    self.view.handle(input);
                }
                Request::List(request) => match request {
                    ListRequest::Configure(config) => {
                        crate::appearance::apply_theme(ctx, config.theme);
                        self.theme = config.theme;
                        self.list.configure(*config);
                        ctx.send_viewport_cmd_to(
                            self.list_id(),
                            egui::ViewportCommand::Title(self.list.config.labels.title.clone()),
                        );
                        ctx.request_repaint_of(self.list_id());
                    }
                    ListRequest::Show(list) => {
                        self.list = *list;
                        self.list.set_visible(true);
                        crate::appearance::apply_theme(ctx, self.list.config.theme);
                        self.theme = self.list.config.theme;
                        ctx.send_viewport_cmd_to(
                            self.list_id(),
                            egui::ViewportCommand::Title(self.list.config.labels.title.clone()),
                        );
                        ctx.send_viewport_cmd_to(
                            self.list_id(),
                            egui::ViewportCommand::Visible(true),
                        );
                        if !self.list.persistent {
                            ctx.send_viewport_cmd_to(self.list_id(), egui::ViewportCommand::Focus);
                        }
                        ctx.request_repaint_of(self.list_id());
                        ctx.request_repaint();
                    }
                    ListRequest::Hide => {
                        self.list.set_visible(false);
                        ctx.send_viewport_cmd_to(
                            self.list_id(),
                            egui::ViewportCommand::Visible(false),
                        );
                    }
                },
                Request::History(request) => match request {
                    HistoryRequest::Configure(config) => {
                        crate::appearance::apply_theme(ctx, config.theme);
                        self.theme = config.theme;
                        self.history.configure(*config);
                        ctx.send_viewport_cmd_to(
                            self.history_id(),
                            egui::ViewportCommand::Title(self.history.config.labels.title.clone()),
                        );
                        ctx.request_repaint_of(self.history_id());
                    }
                    HistoryRequest::Show(history) => {
                        self.history = *history;
                        self.history.set_visible(true);
                        crate::appearance::apply_theme(ctx, self.history.config.theme);
                        self.theme = self.history.config.theme;
                        for command in [
                            egui::ViewportCommand::Title(self.history.config.labels.title.clone()),
                            egui::ViewportCommand::Visible(true),
                            egui::ViewportCommand::Focus,
                        ] {
                            ctx.send_viewport_cmd_to(self.history_id(), command);
                        }
                        ctx.request_repaint_of(self.history_id());
                        ctx.request_repaint();
                    }
                    HistoryRequest::Hide => {
                        self.history.set_visible(false);
                        ctx.send_viewport_cmd_to(
                            self.history_id(),
                            egui::ViewportCommand::Visible(false),
                        );
                    }
                },
                Request::Shutdown => {
                    self.shared.shutdown.store(true, Ordering::SeqCst);
                    self.list.set_visible(false);
                    self.history.set_visible(false);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            match self.root {
                Root::Settings => self.close_settings(ctx),
                Root::List => self.list.close(ctx),
                Root::History => self.history.close(ctx),
            }
        }
        self.list.keep_on_screen(ctx);
        self.history.keep_on_screen(ctx);
    }

    fn spellcheck_ui(&mut self, ui: &mut egui::Ui) {
        let Some(result) = &mut self.spellcheck_result else {
            return;
        };
        let corrected = ui
            .add_enabled_ui(self.spellcheck_pending.is_none(), |ui| {
                result.ui(ui, self.lang, self.spellcheck_target.is_some())
            })
            .inner;
        if let Some(corrected) = corrected {
            self.submit_spelling_replacement(corrected);
            return;
        }
        #[cfg(windows)]
        let _ =
            crate::window_position::keep_visible(result.title(self.lang), self.spellcheck_position);
        if ui.ctx().input(|input| input.viewport().close_requested()) {
            tracing::debug!(target: "okbs_spelling", "suggestion popup dismissed");
            self.spellcheck_result = None;
            self.spellcheck_pending = None;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    fn submit_spelling_replacement(&mut self, corrected: String) {
        if self.spellcheck_pending.is_some() {
            return;
        }
        let (Some(result), Some(target)) = (&mut self.spellcheck_result, self.spellcheck_target)
        else {
            return;
        };
        static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);
        let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(target: "okbs_spelling", request_id, ?target,
            changed = corrected != result.original(), "replace button clicked; keeping popup until acknowledgement");
        result.replacement_failed(false);
        self.spellcheck_pending = Some(request_id);
        if self
            .events
            .send(SettingsEvent::ReplaceSpelling {
                request_id,
                target,
                original: result.original().to_string(),
                corrected,
            })
            .is_err()
        {
            self.spellcheck_pending = None;
            result.replacement_failed(true);
            tracing::error!(target: "okbs_spelling", request_id, "popup replacement channel disconnected");
        }
    }

    fn finish_spelling_replacement(&mut self, ctx: &egui::Context, request_id: u64, success: bool) {
        if self.spellcheck_pending != Some(request_id) {
            return;
        }
        self.spellcheck_pending = None;
        if success {
            self.spellcheck_result = None;
            ctx.send_viewport_cmd_to(self.spellcheck_id(), egui::ViewportCommand::Visible(false));
        } else if let Some(result) = &mut self.spellcheck_result {
            result.replacement_failed(true);
        }
        ctx.request_repaint_of(self.spellcheck_id());
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            self.close_settings(ui.ctx());
        }
        if !self.shared.open.load(Ordering::SeqCst) {
            return;
        }
        let mut events = Vec::new();
        let action = if self.settings_visible {
            self.view.ui(ui, &mut events)
        } else {
            WindowAction::Stay
        };
        if let Some(result) = &mut self.text_result {
            if self.settings_visible {
                let mut open = true;
                egui::Window::new(result.title(self.lang))
                    .id(egui::Id::new("clipboard_result"))
                    .default_width(560.0)
                    .open(&mut open)
                    .show(ui.ctx(), |ui| {
                        let _ = result.ui(ui, self.lang, false);
                    });
                if !open {
                    self.text_result = None;
                }
            } else {
                let _ = result.ui(ui, self.lang, false);
                #[cfg(windows)]
                if result.is_compact() {
                    let _ = crate::window_position::keep_visible(
                        result.title(self.lang),
                        self.text_result_position,
                    );
                }
            }
        }
        for event in events {
            if let SettingsEvent::Apply(config) = &event {
                self.set_appearance(ui.ctx(), config.general.ui_language, config.general.theme);
            }
            let _ = self.events.send(event);
        }
        if action == WindowAction::Close {
            self.close_settings(ui.ctx());
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_requests(ctx);
        #[cfg(windows)]
        if let Some(captured) = crate::key_capture::take() {
            self.view.handle(match captured {
                crate::key_capture::Captured::Hotkey(hotkey) => {
                    SettingsInput::HotkeyCaptured(hotkey)
                }
                crate::key_capture::Captured::Cancelled => SettingsInput::CaptureCancelled,
            });
            // The engine may still be waiting when its hook was not called.
            let _ = self.events.send(SettingsEvent::CancelCapture);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.shared.shutdown.load(Ordering::SeqCst) {
            return;
        }
        let ctx = ui.ctx().clone();
        // All three native windows belong to this one winit event loop. Keep
        // the hidden viewports registered so logic can reopen them while every
        // window is hidden.
        match self.root {
            Root::Settings => self.settings_ui(ui),
            Root::List => self.list.ui(ui),
            Root::History => self.history.ui(ui),
        }
        if self.root != Root::Settings {
            let builder = settings_builder(tr(Text::SettingsWindowTitle, self.lang))
                .with_visible(self.shared.open.load(Ordering::SeqCst));
            ctx.show_viewport_immediate(self.settings_id(), builder, |ui, _| self.settings_ui(ui));
        }
        if self.root != Root::List {
            ctx.show_viewport_immediate(self.list_id(), self.list.builder(), |ui, _| {
                self.list.ui(ui)
            });
        }
        if self.root != Root::History {
            ctx.show_viewport_immediate(self.history_id(), self.history.builder(), |ui, _| {
                self.history.ui(ui)
            });
        }
        let builder = egui::ViewportBuilder::default()
            .with_title(tr(Text::SpellcheckWordTitle, self.lang))
            .with_icon(crate::branding::icon())
            .with_inner_size([390.0, 130.0])
            .with_min_inner_size([300.0, 115.0])
            .with_max_inner_size([520.0, 170.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_always_on_top()
            .with_active(false)
            .with_visible(self.spellcheck_result.is_some())
            .with_position(self.spellcheck_position.unwrap_or([24.0, 24.0]));
        ctx.show_viewport_immediate(self.spellcheck_id(), builder, |ui, _| {
            self.spellcheck_ui(ui)
        });
        #[cfg(windows)]
        crate::key_capture::set_active(
            self.shared.open.load(Ordering::SeqCst) && self.view.is_capturing(),
        );
    }
}

impl SettingsWindow {
    pub fn autoreplace_list(&self) -> AutoreplaceListWindow {
        AutoreplaceListWindow::new(self.requests.clone(), self.shared.clone())
    }

    /// Handle of the clipboard history window on the same event loop.
    pub fn clipboard_history(&self) -> ClipboardHistoryWindow {
        ClipboardHistoryWindow::new(self.requests.clone(), self.shared.clone())
    }
    /// Starts the window thread. Events of the window go to `events`.
    pub fn spawn(events: Sender<SettingsEvent>, system_language: Lang) -> std::io::Result<Self> {
        let (requests, rx) = unbounded();
        let shared = Arc::new(Shared::default());
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("okbs-settings".into())
            .spawn(move || window_thread(&rx, &events, &thread_shared, system_language))?;
        Ok(Self {
            requests,
            shared,
            thread: Some(thread),
        })
    }

    /// Whether the window is currently shown.
    pub fn is_open(&self) -> bool {
        self.shared.open.load(Ordering::SeqCst)
    }

    /// Opens the window (or brings it to the front) on `section`.
    pub fn open(
        &self,
        config: &Config,
        section: Section,
        suggested_rule: Option<Rule>,
        layouts: Vec<LayoutEntry>,
    ) {
        self.open_with_input(
            config,
            section,
            suggested_rule.map(SettingsInput::SuggestRule),
            layouts,
        );
    }

    /// Asks whether to restart with administrator rights. The question has to
    /// reach the user even when «ОК» closed the window in the same frame, so
    /// the window is opened again if needed.
    pub fn ask_elevation(&self, config: &Config, failed: bool, layouts: Vec<LayoutEntry>) {
        self.open_with_input(
            config,
            Section::General,
            Some(SettingsInput::ElevationRequired { failed }),
            layouts,
        );
    }

    /// Opens an editor with selected text, including when the window is closed.
    pub fn add_autoreplace(&self, config: &Config, text: String, layouts: Vec<LayoutEntry>) {
        self.open_with_input(
            config,
            Section::Autoreplace,
            Some(SettingsInput::AddAutoreplace(text)),
            layouts,
        );
    }

    fn open_with_input(
        &self,
        config: &Config,
        section: Section,
        input: Option<SettingsInput>,
        layouts: Vec<LayoutEntry>,
    ) {
        let _ = self.requests.send(Request::Open(
            Box::new(config.clone()),
            section,
            input,
            layouts,
        ));
        self.shared.wake();
    }

    /// Forwards a message to the open window.
    pub fn send(&self, input: SettingsInput) {
        self.notifier().send(input);
    }

    pub fn notifier(&self) -> SettingsNotifier {
        SettingsNotifier {
            requests: self.requests.clone(),
            shared: self.shared.clone(),
        }
    }

    /// Opens a clipboard result, preserving any settings draft already open.
    pub fn show_text(&self, config: &Config, result: TextResult) {
        let _ = self.requests.send(Request::OpenText(
            Box::new(config.clone()),
            result,
            true,
            None,
            None,
        ));
        self.shared.wake();
    }

    /// Shows an interactive completed-word spelling result. The engine returns
    /// focus to the original input control before applying the chosen fix.
    pub fn show_text_passive(
        &self,
        config: &Config,
        result: TextResult,
        position: Option<[f32; 2]>,
        target: Option<(u64, u64)>,
    ) {
        let _ = self.requests.send(Request::OpenText(
            Box::new(config.clone()),
            result,
            false,
            position,
            target,
        ));
        self.shared.wake();
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Shutdown);
        self.shared.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn settings_builder(title: &str) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_icon(crate::branding::icon())
        .with_inner_size(WINDOW_SIZE)
        .with_min_inner_size(WINDOW_SIZE)
        .with_max_inner_size(WINDOW_SIZE)
        .with_resizable(false)
        .with_maximize_button(false)
}

fn native_options(title: &str, shared: &Arc<Shared>) -> eframe::NativeOptions {
    #[cfg(windows)]
    let shared = shared.clone();
    #[cfg(not(windows))]
    let _ = shared;
    eframe::NativeOptions {
        viewport: settings_builder(title),
        centered: true,
        persist_window: false,
        // Called once: later windows of this thread reuse the same event loop.
        event_loop_builder: Some(Box::new(move |builder| {
            #[cfg(windows)]
            {
                use winit::platform::windows::EventLoopBuilderExtWindows;
                builder.with_any_thread(true);
                builder.with_msg_hook(move |msg| {
                    crate::key_capture::observe(msg, || shared.wake());
                    false
                });
            }
            #[cfg(target_os = "linux")]
            {
                winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(builder, true);
            }
        })),
        ..Default::default()
    }
}

fn window_thread(
    requests: &Receiver<Request>,
    events: &Sender<SettingsEvent>,
    shared: &Arc<Shared>,
    system_language: Lang,
) {
    while let Ok(request) = requests.recv() {
        let passive = matches!(&request, Request::OpenText(_, _, false, _, _));
        let mut initial_list = None;
        let mut initial_history = None;
        let mut initial_spellcheck = None;
        let mut initial_spellcheck_position = None;
        let mut initial_spellcheck_target = None;
        let (config, section, input, layouts, text_result, settings_visible) = match request {
            Request::Open(config, section, input, layouts) => {
                (config, section, input, layouts, None, true)
            }
            Request::OpenText(config, result, _, position, target) => {
                if passive {
                    initial_spellcheck = Some(result);
                    initial_spellcheck_position = position;
                    initial_spellcheck_target = target;
                    (config, Section::General, None, Vec::new(), None, false)
                } else {
                    (
                        config,
                        Section::General,
                        None,
                        Vec::new(),
                        Some((result, position)),
                        false,
                    )
                }
            }
            Request::Input(_) => continue,
            Request::List(ListRequest::Show(list)) => {
                let mut config = Config::default();
                config.general.theme = list.config.theme;
                initial_list = Some(*list);
                (
                    Box::new(config),
                    Section::General,
                    None,
                    Vec::new(),
                    None,
                    false,
                )
            }
            Request::List(_) => continue,
            Request::History(HistoryRequest::Show(history)) => {
                let mut config = Config::default();
                config.general.theme = history.config.theme;
                initial_history = Some(*history);
                (
                    Box::new(config),
                    Section::General,
                    None,
                    Vec::new(),
                    None,
                    false,
                )
            }
            Request::History(_) => continue,
            Request::Shutdown => return,
        };
        let lang = config.general.ui_language.resolve(system_language);
        let theme = config.general.theme;
        let mut view = SettingsView::new(*config, section, system_language);
        shared.restore_dictionary_state(&mut view);
        view.set_layouts(layouts);
        if let Some(input) = input {
            view.handle(input);
        }
        let root = match (initial_list.is_some(), initial_history.is_some()) {
            (true, _) => Root::List,
            (_, true) => Root::History,
            _ => Root::Settings,
        };
        shared.open.store(root == Root::Settings, Ordering::SeqCst);
        let list = initial_list.unwrap_or_default();
        list.set_visible(root == Root::List);
        let history = initial_history.unwrap_or_default();
        history.set_visible(root == Root::History);
        let app_requests = requests.clone();
        let app_events = events.clone();
        let app_shared = shared.clone();
        let settings_title = text_result
            .as_ref()
            .map_or(tr(Text::SettingsWindowTitle, lang), |(r, _)| r.title(lang));
        let title = match root {
            Root::List => list.config.labels.title.clone(),
            Root::History => history.config.labels.title.clone(),
            Root::Settings => settings_title.into(),
        };
        let mut options = native_options(&title, shared);
        if let Some((result, position)) = &text_result
            && result.is_compact()
        {
            options.viewport = egui::ViewportBuilder::default()
                .with_title(result.title(lang))
                .with_icon(crate::branding::icon())
                .with_inner_size([390.0, 130.0])
                .with_min_inner_size([300.0, 115.0])
                .with_max_inner_size([520.0, 170.0])
                .with_resizable(false)
                .with_maximize_button(false)
                .with_active(false)
                .with_position(position.unwrap_or([24.0, 24.0]));
        }
        if passive {
            options.viewport = options.viewport.with_active(false);
            options.viewport = options.viewport.with_visible(false);
        }
        match root {
            Root::List => {
                options.viewport = list.builder().with_visible(true);
                options.centered = false;
            }
            Root::History => {
                options.viewport = history.builder().with_visible(true);
                options.centered = false;
            }
            Root::Settings => {}
        }
        let (text_result, text_result_position) = match text_result {
            Some((result, position)) => (Some(result), position),
            None => (None, None),
        };
        let result = eframe::run_native(
            &title,
            options,
            Box::new(move |cc| {
                crate::appearance::configure(&cc.egui_ctx);
                crate::appearance::apply_theme(&cc.egui_ctx, theme);
                cc.egui_ctx.request_repaint();
                if let Ok(mut slot) = app_shared.context.lock() {
                    *slot = Some(cc.egui_ctx.clone());
                }
                Ok(Box::new(App {
                    view,
                    requests: app_requests,
                    events: app_events,
                    shared: app_shared,
                    theme,
                    lang,
                    system_language,
                    settings_visible,
                    text_result,
                    text_result_position,
                    spellcheck_result: initial_spellcheck,
                    spellcheck_position: initial_spellcheck_position,
                    spellcheck_target: initial_spellcheck_target,
                    spellcheck_pending: None,
                    root,
                    list,
                    history,
                }))
            }),
        );
        if let Err(err) = result {
            tracing::error!("cannot open the settings window: {err}");
        }
        if let Ok(mut slot) = shared.context.lock() {
            *slot = None;
        }
        shared.open.store(false, Ordering::SeqCst);
        let _ = events.send(SettingsEvent::Closed);
        if shared.shutdown.load(Ordering::SeqCst) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_popup_waits_for_matching_success_before_hiding() {
        let ctx = egui::Context::default();
        let (tx, requests) = unbounded();
        let (events, received) = unbounded();
        let mut app = App {
            view: SettingsView::new(Config::default(), Section::General, Lang::En),
            requests,
            events,
            shared: Arc::default(),
            theme: Theme::System,
            lang: Lang::En,
            system_language: Lang::En,
            settings_visible: false,
            text_result: None,
            text_result_position: None,
            spellcheck_result: Some(TextResult::spelling_popup("wrold".into(), Vec::new())),
            spellcheck_position: None,
            spellcheck_target: Some((10, 11)),
            spellcheck_pending: None,
            root: Root::Settings,
            list: ListView::default(),
            history: HistoryView::default(),
        };
        app.submit_spelling_replacement("world".into());
        let SettingsEvent::ReplaceSpelling {
            request_id: first, ..
        } = received.try_recv().unwrap()
        else {
            panic!("replacement request")
        };
        assert!(app.spellcheck_result.is_some());
        assert_eq!(app.spellcheck_pending, Some(first));
        app.submit_spelling_replacement("world".into());
        assert!(
            received.try_recv().is_err(),
            "double click must not send twice"
        );
        tx.send(Request::Input(SettingsInput::SpellingReplacementFinished {
            request_id: first,
            success: false,
        }))
        .unwrap();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.process_requests(ui.ctx())
        });
        output.textures_delta.clear();
        assert!(
            app.spellcheck_result.is_some(),
            "failed replacement must remain visible"
        );
        assert_eq!(app.spellcheck_pending, None);
        app.submit_spelling_replacement("world".into());
        let SettingsEvent::ReplaceSpelling {
            request_id: second, ..
        } = received.try_recv().unwrap()
        else {
            panic!("replacement retry")
        };
        tx.send(Request::Input(SettingsInput::SpellingReplacementFinished {
            request_id: first,
            success: true,
        }))
        .unwrap();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.process_requests(ui.ctx())
        });
        output.textures_delta.clear();
        assert!(
            app.spellcheck_result.is_some(),
            "old acknowledgement must not close a newer request"
        );
        assert_eq!(app.spellcheck_pending, Some(second));
        tx.send(Request::Input(SettingsInput::SpellingReplacementFinished {
            request_id: second,
            success: true,
        }))
        .unwrap();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.process_requests(ui.ctx())
        });
        output.textures_delta.clear();
        assert!(app.spellcheck_result.is_none());
        assert_eq!(app.spellcheck_pending, None);
    }

    #[test]
    fn shutdown_is_not_canceled_by_the_normal_hide_on_close_handler() {
        for root in [Root::Settings, Root::List, Root::History] {
            let ctx = egui::Context::default();
            let (tx, requests) = unbounded();
            let (events, _) = unbounded();
            let mut app = App {
                view: SettingsView::new(Config::default(), Section::General, Lang::En),
                requests,
                events,
                shared: Arc::default(),
                theme: Theme::Dark,
                lang: Lang::En,
                system_language: Lang::En,
                settings_visible: true,
                text_result: None,
                text_result_position: None,
                spellcheck_result: None,
                spellcheck_position: None,
                spellcheck_target: None,
                spellcheck_pending: None,
                root,
                list: ListView::default(),
                history: HistoryView::default(),
            };
            app.list.set_visible(true);
            app.history.set_visible(true);
            tx.send(Request::Shutdown).expect("shutdown request");
            for _ in 0..2 {
                let mut input = egui::RawInput::default();
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .expect("root viewport")
                    .events
                    .push(egui::ViewportEvent::Close);
                let mut output = ctx.run_ui(input, |ui| app.process_requests(ui.ctx()));
                output.textures_delta.clear();
                let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
                assert!(commands.contains(&egui::ViewportCommand::Close));
                assert!(!commands.contains(&egui::ViewportCommand::CancelClose));
                assert!(!app.list.visible());
                assert!(!app.history.visible());
            }
        }
    }

    #[test]
    fn applying_language_updates_the_native_title_and_text_windows() {
        let ctx = egui::Context::default();
        let (_, requests) = unbounded();
        let (events, _) = unbounded();
        let mut app = App {
            view: SettingsView::new(Config::default(), Section::General, Lang::Ru),
            requests,
            events,
            shared: Arc::new(Shared::default()),
            theme: Theme::System,
            lang: Lang::Ru,
            system_language: Lang::Ru,
            settings_visible: true,
            text_result: Some(TextResult::conversion("test".into())),
            text_result_position: None,
            spellcheck_result: None,
            spellcheck_position: None,
            spellcheck_target: None,
            spellcheck_pending: None,
            root: Root::Settings,
            list: ListView::default(),
            history: HistoryView::default(),
        };
        for (visible, preference, expected) in [
            (true, UiLanguage::En, "Own Keyboard Switch Settings"),
            (true, UiLanguage::System, "Настройки Own Keyboard Switch"),
            (false, UiLanguage::En, "Clipboard conversion result"),
            (
                false,
                UiLanguage::System,
                "Результат конвертации буфера обмена",
            ),
        ] {
            app.settings_visible = visible;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                app.set_appearance(ui.ctx(), preference, Theme::System);
            });
            output.textures_delta.clear();
            assert_eq!(app.lang, preference.resolve(Lang::Ru));
            assert!(output.viewport_output[&egui::ViewportId::ROOT].commands.iter().any(|command|
                matches!(command, egui::ViewportCommand::Title(title) if title == expected)
            ));
        }
    }
}
