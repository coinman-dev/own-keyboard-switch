//! «Показать историю буфера обмена»: the remembered clipboard texts on the
//! shared UI thread and event loop.
//!
//! The window never takes the focus away from the application the entry is
//! inserted into; the controller restores that application before pasting.

use crate::window::{Request, Shared};
use crossbeam_channel::{Receiver, Sender, unbounded};
use okbs_core::config::Theme;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const WINDOW_SIZE: [f32; 2] = [520.0, 420.0];
/// Characters of an entry shown in the list; the rest is elided.
const PREVIEW: usize = 90;

#[derive(Debug, Clone, Default)]
pub struct ClipboardHistoryLabels {
    pub title: String,
    pub insert: String,
    pub close: String,
    pub clear: String,
    pub empty: String,
    pub help: String,
}

#[derive(Debug, Clone, Default)]
pub struct ClipboardHistoryConfig {
    /// Newest entry first. Never written to the log.
    pub entries: Vec<String>,
    pub labels: ClipboardHistoryLabels,
    pub theme: Theme,
}

/// What the user chose in the history window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryChoice {
    /// Insert the entry with this index into the remembered application.
    Insert(usize),
    /// «Очистить»: forget every remembered text.
    Clear,
}

pub(crate) enum HistoryRequest {
    Configure(Box<ClipboardHistoryConfig>),
    Show(Box<HistoryView>),
    Hide,
}

/// A handle to a viewport of SettingsWindow, never a second event loop.
pub struct ClipboardHistoryWindow {
    requests: Sender<Request>,
    chosen: Sender<HistoryChoice>,
    results: Receiver<HistoryChoice>,
    shared: Arc<Shared>,
    visible: Arc<AtomicBool>,
}

impl std::fmt::Debug for ClipboardHistoryWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardHistoryWindow")
            .finish_non_exhaustive()
    }
}

impl ClipboardHistoryWindow {
    pub(crate) fn new(requests: Sender<Request>, shared: Arc<Shared>) -> Self {
        let (chosen, results) = unbounded();
        Self {
            requests,
            chosen,
            results,
            shared,
            visible: Arc::default(),
        }
    }

    fn send(&self, request: HistoryRequest) {
        let _ = self.requests.send(Request::History(request));
        self.shared.wake();
    }

    /// Updates the entries of an open window without showing a hidden one.
    pub fn configure(&self, config: ClipboardHistoryConfig) {
        self.send(HistoryRequest::Configure(Box::new(config)));
    }

    pub fn show(&self, config: ClipboardHistoryConfig, position: [f32; 2]) {
        self.send(HistoryRequest::Show(Box::new(HistoryView {
            selected_index: (!config.entries.is_empty()).then_some(0),
            config,
            position,
            place_on_open: true,
            chosen: self.chosen.clone(),
            visible: self.visible.clone(),
        })));
    }

    pub fn hide(&self) {
        self.send(HistoryRequest::Hide);
    }

    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    pub fn poll(&self) -> Option<HistoryChoice> {
        self.results.try_recv().ok()
    }
}

impl Drop for ClipboardHistoryWindow {
    fn drop(&mut self) {
        self.hide();
    }
}

pub(crate) struct HistoryView {
    pub config: ClipboardHistoryConfig,
    pub position: [f32; 2],
    place_on_open: bool,
    selected_index: Option<usize>,
    chosen: Sender<HistoryChoice>,
    visible: Arc<AtomicBool>,
}

impl Default for HistoryView {
    fn default() -> Self {
        Self {
            config: ClipboardHistoryConfig::default(),
            position: [24.0, 24.0],
            place_on_open: false,
            selected_index: None,
            chosen: unbounded().0,
            visible: Arc::default(),
        }
    }
}

/// One line of an entry, with control characters replaced and the rest elided.
fn preview(text: &str) -> String {
    let mut out: String = text
        .chars()
        .take(PREVIEW)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if text.chars().nth(PREVIEW).is_some() {
        out.push('…');
    }
    out.trim().to_string()
}

impl HistoryView {
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
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    pub fn visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::SeqCst);
    }

    pub fn configure(&mut self, config: ClipboardHistoryConfig) {
        self.config = config;
        if self
            .selected_index
            .is_none_or(|i| i >= self.config.entries.len())
        {
            self.selected_index = (!self.config.entries.is_empty()).then_some(0);
        }
    }

    fn choose(&mut self, ctx: &egui::Context) {
        if let Some(index) = self.selected_index
            && self.config.entries.get(index).is_some()
        {
            let _ = self.chosen.send(HistoryChoice::Insert(index));
            self.close(ctx);
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
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close(ui.ctx());
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.choose(ui.ctx());
        }
        crate::appearance::panel(ui, |ui| {
            let footer_height =
                ui.spacing().interact_size.y + ui.text_style_height(&egui::TextStyle::Small) + 24.0;
            let list_height = (ui.available_height() - footer_height).max(0.0);
            egui::ScrollArea::vertical()
                .max_height(list_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.config.entries.is_empty() {
                        ui.weak(&self.config.labels.empty);
                    } else {
                        for index in 0..self.config.entries.len() {
                            let response = ui.selectable_label(
                                self.selected_index == Some(index),
                                preview(&self.config.entries[index]),
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
                        self.selected_index.is_some(),
                        crate::appearance::action_button(&self.config.labels.insert),
                    )
                    .clicked()
                {
                    self.choose(ui.ctx());
                }
                if ui
                    .add_enabled(
                        !self.config.entries.is_empty(),
                        crate::appearance::action_button(&self.config.labels.clear),
                    )
                    .clicked()
                {
                    let _ = self.chosen.send(HistoryChoice::Clear);
                    self.config.entries.clear();
                    self.selected_index = None;
                }
                if ui
                    .add(crate::appearance::action_button(&self.config.labels.close))
                    .clicked()
                {
                    self.close(ui.ctx());
                }
            });
            ui.label(egui::RichText::new(&self.config.labels.help).weak().small());
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
            .with_visible(self.visible())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_one_trimmed_line_and_elides_long_texts() {
        assert_eq!(preview(" hello\tworld \n"), "hello world");
        let long = "a".repeat(PREVIEW + 5);
        let shown = preview(&long);
        assert_eq!(shown.chars().count(), PREVIEW + 1);
        assert!(shown.ends_with('…'));
        assert_eq!(preview(&"b".repeat(PREVIEW)), "b".repeat(PREVIEW));
    }

    #[test]
    fn configure_keeps_a_valid_selection() {
        let mut view = HistoryView::default();
        view.configure(ClipboardHistoryConfig {
            entries: vec!["one".into(), "two".into()],
            ..Default::default()
        });
        assert_eq!(view.selected_index, Some(0));
        view.selected_index = Some(1);
        view.configure(ClipboardHistoryConfig {
            entries: vec!["only".into()],
            ..Default::default()
        });
        assert_eq!(view.selected_index, Some(0));
        view.configure(ClipboardHistoryConfig::default());
        assert_eq!(view.selected_index, None);
    }
}
