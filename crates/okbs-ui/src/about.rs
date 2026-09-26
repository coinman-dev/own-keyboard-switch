//! «О программе»: version, copyright, license and the notices of third-party
//! components.

use crate::appearance::action_button;
use crate::i18n::{Text, tr};
use egui::{RichText, Ui};
use okbs_core::Lang;
use std::sync::OnceLock;

const NOTICE: &str = include_str!("../../../NOTICE");

/// Everything `okbswitch --licenses` prints and «Лицензии...» shows: the
/// project notice and license, then the terms of the dictionaries, fonts and
/// libraries built into the program.
pub fn licenses_text() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        [
            NOTICE,
            include_str!("../../../LICENSE"),
            include_str!("../../../data/LICENSES.md"),
            include_str!("../../../data/hunspell/README_en_US.txt"),
            include_str!("../../../THIRD-PARTY-NOTICES.txt"),
        ]
        .join("\n")
    })
}

/// «Copyright (c) 2026 ...» from the notice the license requires to pass on.
fn copyright() -> &'static str {
    NOTICE
        .lines()
        .find_map(|line| line.strip_prefix("Required Notice: "))
        .unwrap_or_default()
}

fn project_page() -> &'static str {
    NOTICE
        .lines()
        .find(|line| line.starts_with("https://"))
        .unwrap_or_default()
}

/// Contents of the «О программе» section. Returns `true` when «Лицензии...»
/// was clicked.
pub(crate) fn section(ui: &mut Ui, lang: Lang) -> bool {
    ui.label(
        RichText::new(format!(
            "{}, {} {}",
            tr(Text::AppName, lang),
            tr(Text::AboutVersion, lang),
            okbs_core::VERSION
        ))
        .strong(),
    );
    ui.add_space(6.0);
    for text in [Text::AboutDescription, Text::AboutPrivacy] {
        ui.add(egui::Label::new(tr(text, lang)).wrap());
        ui.add_space(4.0);
    }
    ui.separator();
    ui.label(copyright());
    ui.add(egui::Label::new(project_page()).wrap());
    ui.add_space(4.0);
    for text in [Text::AboutLicense, Text::AboutThirdParty] {
        ui.add(egui::Label::new(tr(text, lang)).wrap());
        ui.add_space(4.0);
    }
    ui.add(action_button(tr(Text::AboutLicenses, lang)))
        .clicked()
}

/// The full notices in a scrollable box; `open` turns false on «Закрыть».
pub(crate) fn licenses_dialog(ctx: &egui::Context, lang: Lang, open: &mut bool) {
    static LINES: OnceLock<Vec<&'static str>> = OnceLock::new();
    let lines = LINES.get_or_init(|| licenses_text().lines().collect());
    egui::Modal::new(egui::Id::new("licenses_dialog")).show(ctx, |ui| {
        crate::appearance::content(ui);
        ui.set_width(700.0);
        ui.heading(tr(Text::AboutLicensesTitle, lang));
        ui.add_space(4.0);
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        // Only the visible lines are laid out: the notices are long.
        egui::ScrollArea::both()
            .max_height(400.0)
            .auto_shrink([false, false])
            .show_rows(ui, row_height, lines.len(), |ui, rows| {
                for line in &lines[rows] {
                    ui.add(egui::Label::new(RichText::new(*line).monospace()).extend());
                }
            });
        ui.add_space(8.0);
        if ui.add(action_button(tr(Text::BtnClose, lang))).clicked() {
            *open = false;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_provides_the_copyright_and_the_project_page() {
        assert!(copyright().starts_with("Copyright (c) "));
        assert!(project_page().starts_with("https://github.com/"));
        let text = licenses_text();
        assert!(text.starts_with(NOTICE));
        assert!(text.contains("PolyForm Noncommercial License 1.0.0"));
    }
}
