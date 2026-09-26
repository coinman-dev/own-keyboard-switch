//! Clipboard conversion and spelling results; no text is persisted or logged.

use crate::i18n::{Text, tr};
use egui::{RichText, Ui};
use okbs_core::Lang;
use okbs_core::spell::{Misspelling, apply_first_suggestions};

/// What the user chose in the compact spelling popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellingAction {
    /// Replace the word; the text with the correction applied.
    Replace(String),
    /// Keep the word as typed and stop offering corrections for it.
    Skip,
    /// Keep the word and accept it from now on («Мои слова»).
    AddWord,
}

/// A clipboard result ready to display.
#[derive(Clone)]
pub struct TextResult {
    original: String,
    spelling: Option<Vec<Misspelling>>,
    choices: Vec<Option<usize>>,
    copied: bool,
    compact: bool,
    replacement_failed: bool,
    /// Height the compact contents took in the last frame.
    compact_height: Option<f32>,
}

impl std::fmt::Debug for TextResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextResult")
            .field("characters", &self.original.chars().count())
            .field("spelling", &self.spelling.is_some())
            .finish_non_exhaustive()
    }
}

impl TextResult {
    /// Result of a layout conversion or transliteration.
    pub fn conversion(text: String) -> Self {
        Self {
            original: text,
            spelling: None,
            choices: Vec::new(),
            copied: false,
            compact: false,
            replacement_failed: false,
            compact_height: None,
        }
    }

    /// Suggestions are applied only when chosen by the user. Even a single
    /// word keeps the full view: its result can only be copied, not replaced.
    pub fn spelling(text: String, misspellings: Vec<Misspelling>) -> Self {
        Self {
            original: text,
            choices: vec![None; misspellings.len()],
            spelling: Some(misspellings),
            copied: false,
            compact: false,
            replacement_failed: false,
            compact_height: None,
        }
    }

    pub fn spelling_popup(text: String, misspellings: Vec<Misspelling>) -> Self {
        Self {
            original: text,
            choices: vec![None; misspellings.len()],
            spelling: Some(misspellings),
            copied: false,
            compact: true,
            replacement_failed: false,
            compact_height: None,
        }
    }

    /// Height of the compact contents, known after they were drawn once.
    pub(crate) fn compact_height(&self) -> Option<f32> {
        self.compact_height
    }

    pub fn is_compact(&self) -> bool {
        self.compact
    }

    pub fn original(&self) -> &str {
        &self.original
    }

    pub(crate) fn replacement_failed(&mut self, failed: bool) {
        self.replacement_failed = failed;
    }

    /// Localized window title.
    pub fn title(&self, lang: Lang) -> &'static str {
        tr(
            if self.compact {
                Text::SpellcheckWordTitle
            } else if self.spelling.is_some() {
                Text::SpellcheckTitle
            } else {
                Text::ClipboardResultTitle
            },
            lang,
        )
    }

    /// Text with the chosen corrections.
    pub fn text(&self) -> String {
        let Some(misspellings) = &self.spelling else {
            return self.original.clone();
        };
        let chosen: Vec<_> = misspellings
            .iter()
            .zip(&self.choices)
            .filter_map(|(m, choice)| {
                let suggestion = m.suggestions.get((*choice)?)?;
                Some(Misspelling {
                    suggestions: vec![suggestion.clone()],
                    ..m.clone()
                })
            })
            .collect();
        apply_first_suggestions(&self.original, &chosen)
    }

    /// Shows suggestions and a selectable result preview. Copying is explicit
    /// and goes through eframe's clipboard output on the window thread.
    pub fn ui(&mut self, ui: &mut Ui, lang: Lang, can_replace: bool) -> Option<SpellingAction> {
        crate::appearance::panel(ui, |ui| self.content_ui(ui, lang, can_replace)).inner
    }

    fn content_ui(&mut self, ui: &mut Ui, lang: Lang, can_replace: bool) -> Option<SpellingAction> {
        if self.compact {
            return self.compact_ui(ui, lang, can_replace);
        }
        ui.heading(self.title(lang));
        if let Some(misspellings) = &self.spelling {
            if misspellings.is_empty() {
                ui.label(tr(Text::SpellingNoErrors, lang));
            } else {
                if ui.button(tr(Text::SpellingApplyAll, lang)).clicked() {
                    for (choice, m) in self.choices.iter_mut().zip(misspellings) {
                        *choice = (!m.suggestions.is_empty()).then_some(0);
                    }
                    self.copied = false;
                }
                egui::ScrollArea::vertical()
                    .id_salt("spelling_words")
                    .max_height((ui.available_height() * 0.35).clamp(60.0, 180.0))
                    .show(ui, |ui| {
                        for (index, (m, choice)) in
                            misspellings.iter().zip(&mut self.choices).enumerate()
                        {
                            ui.horizontal(|ui| {
                                ui.label(&m.word);
                                let label = choice
                                    .and_then(|i| m.suggestions.get(i))
                                    .map_or(tr(Text::SpellingKeep, lang), String::as_str);
                                egui::ComboBox::from_id_salt(("spelling_choice", index))
                                    .selected_text(label)
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_value(
                                                choice,
                                                None,
                                                tr(Text::SpellingKeep, lang),
                                            )
                                            .changed()
                                        {
                                            self.copied = false;
                                        }
                                        for (i, suggestion) in m.suggestions.iter().enumerate() {
                                            if ui
                                                .selectable_value(choice, Some(i), suggestion)
                                                .changed()
                                            {
                                                self.copied = false;
                                            }
                                        }
                                    });
                            });
                        }
                    });
            }
        }
        ui.separator();
        ui.label(tr(Text::SpellingPreview, lang));
        let result = self.text();
        egui::ScrollArea::vertical()
            .id_salt("clipboard_preview")
            .max_height((ui.available_height() - 70.0).clamp(40.0, 220.0))
            .show(ui, |ui| {
                let mut selectable = result.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut selectable)
                        .desired_width(f32::INFINITY)
                        .desired_rows(6),
                );
            });
        if ui.button(tr(Text::CopyResult, lang)).clicked() {
            ui.ctx().copy_text(result);
            self.copied = true;
        }
        if self.copied {
            ui.label(tr(Text::ResultCopied, lang));
        }
        None
    }

    fn compact_ui(&mut self, ui: &mut Ui, lang: Lang, can_replace: bool) -> Option<SpellingAction> {
        let top = ui.cursor().top();
        let misspelling = self.spelling.as_ref()?.first()?;
        let mut action = None;
        ui.label(RichText::new(&misspelling.word).strong());
        ui.add_space(4.0);
        if misspelling.suggestions.is_empty() {
            ui.label(RichText::new(tr(Text::SpellingNoSuggestions, lang)).weak());
        }
        // One click replaces the word: the variants are the buttons.
        ui.horizontal_wrapped(|ui| {
            for suggestion in &misspelling.suggestions {
                if ui
                    .add_enabled(can_replace, crate::appearance::action_button(suggestion))
                    .clicked()
                {
                    let mut chosen = misspelling.clone();
                    chosen.suggestions = vec![suggestion.clone()];
                    action = Some(SpellingAction::Replace(apply_first_suggestions(
                        &self.original,
                        &[chosen],
                    )));
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add(crate::appearance::action_button(tr(
                    Text::SpellingSkip,
                    lang,
                )))
                .clicked()
            {
                action = Some(SpellingAction::Skip);
            }
            if ui
                .add(crate::appearance::action_button(tr(
                    Text::SpellingAddWord,
                    lang,
                )))
                .clicked()
            {
                action = Some(SpellingAction::AddWord);
            }
        });
        if !can_replace {
            ui.label(
                RichText::new(tr(Text::SpellcheckTargetChanged, lang))
                    .weak()
                    .small(),
            );
        }
        if self.replacement_failed {
            ui.label(tr(Text::SpellcheckReplacementFailed, lang));
        }
        self.compact_height = Some(ui.cursor().top() - top);
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrections_are_opt_in_and_handle_utf8_offsets() {
        let mut view = TextResult::spelling(
            "Превет, wrold!".into(),
            vec![
                Misspelling {
                    range: 0..12,
                    word: "Превет".into(),
                    lang: Lang::Ru,
                    suggestions: vec!["Привет".into()],
                },
                Misspelling {
                    range: 14..19,
                    word: "wrold".into(),
                    lang: Lang::En,
                    suggestions: vec!["world".into()],
                },
            ],
        );
        assert_eq!(view.text(), "Превет, wrold!");
        let single = TextResult::spelling(
            "wrold".into(),
            vec![Misspelling {
                range: 0..5,
                word: "wrold".into(),
                lang: Lang::En,
                suggestions: vec!["world".into()],
            }],
        );
        assert!(!single.is_compact(), "a checked word can be copied");
        view.choices[1] = Some(0);
        assert_eq!(view.text(), "Превет, world!");
        view.choices[0] = Some(0);
        assert_eq!(view.text(), "Привет, world!");
    }

    #[test]
    fn results_render_in_both_languages_without_logging_the_text() {
        for lang in Lang::ALL {
            let mut view = TextResult::conversion("private clipboard text".into());
            assert!(!format!("{view:?}").contains("private"));
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let _ = view.ui(ui, lang, false);
            });
            output.textures_delta.clear();
            let mut view = TextResult::spelling(String::new(), Vec::new());
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let _ = view.ui(ui, lang, false);
            });
            output.textures_delta.clear();
        }
    }

    #[test]
    fn copy_button_is_visible_with_many_suggestions_in_a_small_window() {
        for lang in Lang::ALL {
            let original = "wrold ".repeat(30);
            let misspellings = (0..30)
                .map(|i| Misspelling {
                    range: i * 6..i * 6 + 5,
                    word: "wrold".into(),
                    lang: Lang::En,
                    suggestions: vec!["world".into()],
                })
                .collect();
            let mut view = TextResult::spelling(original, misspellings);
            let ctx = egui::Context::default();
            for _ in 0..3 {
                let mut copy_rect = None;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(620.0, 440.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        let _ = view.ui(ui, lang, false);
                    },
                );
                output.textures_delta.clear();
                for shape in &output.shapes {
                    if let egui::Shape::Text(t) = &shape.shape
                        && t.galley.text() == tr(Text::CopyResult, lang)
                    {
                        copy_rect = Some(t.galley.rect.translate(t.pos.to_vec2()));
                    }
                }
                let rect = copy_rect.expect("copy button must be rendered");
                assert!(
                    rect.bottom() < 440.0,
                    "copy button is outside the window: {rect:?}"
                );
            }
        }
    }

    /// Draws the compact popup, optionally clicking the text `click` first.
    fn popup_frame(view: &mut TextResult, click: Option<&str>) -> Option<SpellingAction> {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 200.0));
        let frame = |events: Vec<egui::Event>, view: &mut TextResult| {
            let mut action = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| action = view.ui(ui, Lang::Ru, true),
            );
            output.textures_delta.clear();
            (action, output.shapes)
        };
        let (action, shapes) = frame(Vec::new(), view);
        let Some(label) = click else {
            return action;
        };
        let target = shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()).center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no «{label}» in the popup"));
        assert!(screen.contains(target), "«{label}» is outside the popup");
        let button = |pressed| egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(target)], view);
        frame(vec![button(true)], view);
        frame(vec![button(false)], view).0
    }

    #[test]
    fn compact_popup_offers_variants_skip_and_personal_words() {
        let popup = || {
            TextResult::spelling_popup(
                "wrold".into(),
                vec![Misspelling {
                    range: 0..5,
                    word: "wrold".into(),
                    lang: Lang::En,
                    suggestions: vec!["world".into(), "would".into()],
                }],
            )
        };
        let mut view = popup();
        assert_eq!(popup_frame(&mut view, None), None);
        let height = view.compact_height().expect("measured");
        assert!(height > 0.0 && height < 200.0, "{height}");
        assert_eq!(
            popup_frame(&mut popup(), Some("would")),
            Some(SpellingAction::Replace("would".into()))
        );
        assert_eq!(
            popup_frame(&mut popup(), Some(tr(Text::SpellingSkip, Lang::Ru))),
            Some(SpellingAction::Skip)
        );
        assert_eq!(
            popup_frame(&mut popup(), Some(tr(Text::SpellingAddWord, Lang::Ru))),
            Some(SpellingAction::AddWord)
        );
    }
}
