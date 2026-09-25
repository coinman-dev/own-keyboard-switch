//! Clipboard conversion and spelling results; no text is persisted or logged.

use crate::i18n::{Text, tr};
use egui::{RichText, Ui};
use okbs_core::Lang;
use okbs_core::spell::{Misspelling, apply_first_suggestions};

/// A clipboard result ready to display.
#[derive(Clone)]
pub struct TextResult {
    original: String,
    spelling: Option<Vec<Misspelling>>,
    choices: Vec<Option<usize>>,
    copied: bool,
    compact: bool,
    replacement_failed: bool,
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
        }
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
    pub fn ui(&mut self, ui: &mut Ui, lang: Lang, can_replace: bool) -> Option<String> {
        crate::appearance::panel(ui, |ui| self.content_ui(ui, lang, can_replace)).inner
    }

    fn content_ui(&mut self, ui: &mut Ui, lang: Lang, can_replace: bool) -> Option<String> {
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

    fn compact_ui(&mut self, ui: &mut Ui, lang: Lang, can_replace: bool) -> Option<String> {
        ui.heading(tr(Text::SpellcheckWordTitle, lang));
        let Some(misspellings) = &self.spelling else {
            return None;
        };
        if let Some((misspelling, choice)) = misspellings.iter().zip(&mut self.choices).next() {
            let label_width = ui.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(
                        misspelling.word.clone(),
                        egui::TextStyle::Body.resolve(ui.style()),
                        ui.visuals().text_color(),
                    )
                    .size()
                    .x
            });
            let combo_width = ui.spacing().combo_width;
            let row_width =
                (label_width + ui.spacing().item_spacing.x + combo_width).min(ui.available_width());
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - row_width) / 2.0).max(0.0));
                ui.allocate_ui_with_layout(
                    egui::vec2(row_width, crate::appearance::CONTROL_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(&misspelling.word);
                        let label = choice
                            .and_then(|index| misspelling.suggestions.get(index))
                            .map_or(tr(Text::SpellingKeep, lang), String::as_str);
                        egui::ComboBox::from_id_salt("spelling_popup_choice")
                            .width(combo_width)
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(choice, None, tr(Text::SpellingKeep, lang));
                                for (index, suggestion) in
                                    misspelling.suggestions.iter().enumerate()
                                {
                                    ui.selectable_value(choice, Some(index), suggestion);
                                }
                            });
                    },
                );
            });
        }
        let mut replace = false;
        ui.vertical_centered(|ui| {
            replace = ui
                .add_enabled(
                    can_replace,
                    crate::appearance::action_button(tr(Text::SpellcheckReplaceWord, lang)),
                )
                .clicked();
        });
        if replace {
            return Some(self.text());
        }
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
        None
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

    #[test]
    fn compact_replace_button_is_centered() {
        let mut view = TextResult::spelling_popup(
            "wrold".into(),
            vec![Misspelling {
                range: 0..5,
                word: "wrold".into(),
                lang: Lang::En,
                suggestions: vec!["world".into()],
            }],
        );
        let ctx = egui::Context::default();
        let width = 390.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 160.0),
                )),
                ..Default::default()
            },
            |ui| {
                let _ = view.ui(ui, Lang::En, true);
            },
        );
        output.textures_delta.clear();
        let heading_left = output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == tr(Text::SpellcheckWordTitle, Lang::En)
            {
                return Some(text.galley.rect.translate(text.pos.to_vec2()).left());
            }
            None
        });
        let text_center = output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == tr(Text::SpellcheckReplaceWord, Lang::En)
            {
                return Some(text.galley.rect.translate(text.pos.to_vec2()).center().x);
            }
            None
        });
        let word_rect = output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == "wrold"
            {
                return Some(text.galley.rect.translate(text.pos.to_vec2()));
            }
            None
        });
        let text_center = text_center.expect("replace button");
        assert!(
            (text_center - width / 2.0).abs() < 1.0,
            "replace button center {text_center} != window center {}",
            width / 2.0
        );
        assert!(heading_left.expect("heading") > 0.0);
        let word_rect = word_rect.expect("misspelled word");
        let expected_left = (width - (word_rect.width() + 10.0 + 200.0)) / 2.0;
        assert!(
            (word_rect.left() - expected_left).abs() < 1.0,
            "replacement row left {} != expected {expected_left}",
            word_rect.left()
        );
    }
}
