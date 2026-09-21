//! Autoreplace list on the shared UI thread and event loop.

use crate::window::{Request, Shared};
use crossbeam_channel::{Receiver, Sender, unbounded};
use okbs_core::config::{AutoReplace, Theme};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const WINDOW_SIZE: [f32; 2] = [480.0, 400.0];

#[derive(Debug, Clone, Default)]
pub struct AutoreplaceListLabels {
    pub title: String,
    pub insert: String,
    pub close: String,
    pub empty: String,
    pub disabled: String,
    pub menu_help: String,
    pub list_help: String,
}

#[derive(Debug, Clone, Default)]
pub struct AutoreplaceListConfig {
    pub settings: AutoReplace,
    pub labels: AutoreplaceListLabels,
    pub theme: Theme,
}

pub(crate) enum ListRequest {
    Configure(Box<AutoreplaceListConfig>),
    Show(Box<ListView>),
    Hide,
}

/// A handle to a viewport of SettingsWindow, never a second event loop.
pub struct AutoreplaceListWindow {
    requests: Sender<Request>,
    selected: Sender<usize>,
    results: Receiver<usize>,
    shared: Arc<Shared>,
    visible: Arc<AtomicBool>,
}

impl std::fmt::Debug for AutoreplaceListWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoreplaceListWindow")
            .finish_non_exhaustive()
    }
}

impl AutoreplaceListWindow {
    pub(crate) fn new(requests: Sender<Request>, shared: Arc<Shared>) -> Self {
        let (selected, results) = unbounded();
        Self {
            requests,
            selected,
            results,
            shared,
            visible: Arc::default(),
        }
    }

    fn send(&self, request: ListRequest) {
        let _ = self.requests.send(Request::List(request));
        self.shared.wake();
    }

    pub fn configure(&self, config: AutoreplaceListConfig) {
        self.send(ListRequest::Configure(Box::new(config)));
    }

    pub fn show(&self, config: AutoreplaceListConfig, persistent: bool, position: [f32; 2]) {
        self.send(ListRequest::Show(Box::new(ListView {
            selected_index: (!config.settings.items.is_empty()).then_some(0),
            config,
            persistent,
            position,
            place_on_open: true,
            selected: self.selected.clone(),
            visible: self.visible.clone(),
        })));
    }

    pub fn hide(&self) {
        self.send(ListRequest::Hide);
    }
    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
    pub fn poll(&self) -> Option<usize> {
        self.results.try_recv().ok()
    }
}

impl Drop for AutoreplaceListWindow {
    fn drop(&mut self) {
        self.hide();
    }
}

pub(crate) struct ListView {
    pub config: AutoreplaceListConfig,
    pub persistent: bool,
    pub position: [f32; 2],
    place_on_open: bool,
    selected_index: Option<usize>,
    selected: Sender<usize>,
    visible: Arc<AtomicBool>,
}

impl Default for ListView {
    fn default() -> Self {
        Self {
            config: AutoreplaceListConfig::default(),
            persistent: false,
            position: [24.0, 24.0],
            place_on_open: false,
            selected_index: None,
            selected: unbounded().0,
            visible: Arc::default(),
        }
    }
}

impl ListView {
    pub fn keep_on_screen(&mut self, ctx: &egui::Context) {
        if !self.visible() {
            return;
        }
        #[cfg(windows)]
        if crate::window_position::keep_visible(
            &self.config.labels.title,
            self.place_on_open.then_some(self.position),
        ) {
            self.place_on_open = false;
        }
        #[cfg(not(windows))]
        if self.place_on_open {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.position.into()));
            self.place_on_open = false;
        }
        // Also recover an open popup after display/DPI/taskbar changes, even
        // while its parent viewport is hidden and no user input arrives.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    pub fn visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::SeqCst);
    }

    pub fn configure(&mut self, config: AutoreplaceListConfig) {
        self.config = config;
        if self
            .selected_index
            .is_none_or(|i| i >= self.config.settings.items.len())
        {
            self.selected_index = (!self.config.settings.items.is_empty()).then_some(0);
        }
    }

    fn choose(&mut self, ctx: &egui::Context) {
        if self.config.settings.enabled
            && let Some(index) = self.selected_index
            && self.config.settings.items.get(index).is_some()
        {
            let _ = self.selected.send(index);
            if !self.persistent {
                self.close(ctx);
            }
        }
    }

    pub fn close(&self, ctx: &egui::Context) {
        self.set_visible(false);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            self.close(ui.ctx());
        }
        if !self.visible() {
            return;
        }
        self.keep_on_screen(ui.ctx());
        if !self.persistent {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.close(ui.ctx());
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.choose(ui.ctx());
            }
        }
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            crate::settings::content_style(ui);
            let footer_height =
                ui.spacing().interact_size.y + ui.text_style_height(&egui::TextStyle::Small) + 24.0;
            let list_height = (ui.available_height() - footer_height).max(0.0);
            egui::ScrollArea::vertical()
                .max_height(list_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if !self.config.settings.enabled {
                        ui.weak(&self.config.labels.disabled);
                    } else if self.config.settings.items.is_empty() {
                        ui.weak(&self.config.labels.empty);
                    } else {
                        for index in 0..self.config.settings.items.len() {
                            let item = &self.config.settings.items[index];
                            let replacement: String = item
                                .to
                                .chars()
                                .take(65)
                                .map(|c| if c.is_control() { ' ' } else { c })
                                .collect();
                            let response = ui.selectable_label(
                                self.selected_index == Some(index),
                                format!("{}  —  {replacement}", item.from),
                            );
                            if response.clicked() {
                                self.selected_index = Some(index);
                            }
                            if response.double_clicked() {
                                self.selected_index = Some(index);
                                self.choose(ui.ctx());
                            }
                        }
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.config.settings.enabled && self.selected_index.is_some(),
                        crate::settings::action_button(&self.config.labels.insert),
                    )
                    .clicked()
                {
                    self.choose(ui.ctx());
                }
                if ui
                    .add(crate::settings::action_button(&self.config.labels.close))
                    .clicked()
                {
                    self.close(ui.ctx());
                }
            });
            ui.label(
                egui::RichText::new(if self.persistent {
                    &self.config.labels.list_help
                } else {
                    &self.config.labels.menu_help
                })
                .weak()
                .small(),
            );
        });
    }

    pub fn builder(&self) -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_title(&self.config.labels.title)
            .with_icon(crate::branding::icon())
            .with_inner_size(WINDOW_SIZE)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_always_on_top()
            .with_active(!self.persistent)
            .with_visible(self.visible())
    }
}
