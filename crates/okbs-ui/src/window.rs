//! Settings, text results, the insertion list and the clipboard history share
//! one GUI event loop. Closing a window hides its viewport; only Shutdown
//! exits the UI thread.

use crate::autoreplace_list::{AutoreplaceListWindow, ListRequest, ListView};
use crate::clipboard_history::{ClipboardHistoryWindow, HistoryRequest, HistoryView};
use crate::i18n::{Text, tr};
use crate::settings::{
    LayoutEntry, Section, SettingsEvent, SettingsInput, SettingsView, WINDOW_SIZE, WindowAction,
    configure_context,
};
use crate::text_result::TextResult;
use crossbeam_channel::{Receiver, Sender, unbounded};
use okbs_core::Lang;
use okbs_core::config::{Config, Rule, Theme, UiLanguage};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub(crate) enum Request {
    Open(
        Box<Config>,
        Section,
        Option<SettingsInput>,
        Vec<LayoutEntry>,
    ),
    OpenText(Box<Config>, TextResult, bool),
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
    open: AtomicBool,
    shutdown: AtomicBool,
}

impl Shared {
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
    root: Root,
    list: ListView,
    history: HistoryView,
}

pub(crate) fn apply_theme(ctx: &egui::Context, theme: Theme) {
    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
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

    fn show_settings_passive(&self, ctx: &egui::Context) {
        self.shared.open.store(true, Ordering::SeqCst);
        for command in [
            egui::ViewportCommand::Visible(true),
            egui::ViewportCommand::Minimized(false),
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
            apply_theme(ctx, theme);
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
                Request::OpenText(config, result, focus) => {
                    self.text_result = Some(result);
                    self.set_appearance(ctx, config.general.ui_language, config.general.theme);
                    if focus {
                        self.show_settings(ctx);
                    } else {
                        self.show_settings_passive(ctx);
                    }
                }
                Request::Input(input) => {
                    if let SettingsInput::ConfigChanged(config) = &input {
                        self.set_appearance(ctx, config.general.ui_language, config.general.theme);
                    }
                    self.view.handle(input);
                }
                Request::List(request) => match request {
                    ListRequest::Configure(config) => {
                        apply_theme(ctx, config.theme);
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
                        apply_theme(ctx, self.list.config.theme);
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
                        apply_theme(ctx, config.theme);
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
                        apply_theme(ctx, self.history.config.theme);
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
                    .show(ui.ctx(), |ui| result.ui(ui, self.lang));
                if !open {
                    self.text_result = None;
                }
            } else {
                result.ui(ui, self.lang);
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
        if self.is_open() {
            let _ = self.requests.send(Request::Input(input));
            self.shared.wake();
        }
    }

    /// Opens a clipboard result, preserving any settings draft already open.
    pub fn show_text(&self, config: &Config, result: TextResult) {
        let _ = self
            .requests
            .send(Request::OpenText(Box::new(config.clone()), result, true));
        self.shared.wake();
    }

    /// Shows a completed-word spelling result without taking focus away from
    /// the application where the user is typing.
    pub fn show_text_passive(&self, config: &Config, result: TextResult) {
        let _ = self
            .requests
            .send(Request::OpenText(Box::new(config.clone()), result, false));
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

fn native_options(title: &str) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: settings_builder(title),
        centered: true,
        persist_window: false,
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(windows)]
            {
                use winit::platform::windows::EventLoopBuilderExtWindows;
                builder.with_any_thread(true);
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
        let mut initial_list = None;
        let mut initial_history = None;
        let (config, section, input, layouts, text_result, settings_visible) = match request {
            Request::Open(config, section, input, layouts) => {
                (config, section, input, layouts, None, true)
            }
            Request::OpenText(config, result, _) => (
                config,
                Section::General,
                None,
                Vec::new(),
                Some(result),
                false,
            ),
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
            .map_or(tr(Text::SettingsWindowTitle, lang), |r| r.title(lang));
        let title = match root {
            Root::List => list.config.labels.title.clone(),
            Root::History => history.config.labels.title.clone(),
            Root::Settings => settings_title.into(),
        };
        let mut options = native_options(&title);
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
        let result = eframe::run_native(
            &title,
            options,
            Box::new(move |cc| {
                configure_context(&cc.egui_ctx);
                apply_theme(&cc.egui_ctx, theme);
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
