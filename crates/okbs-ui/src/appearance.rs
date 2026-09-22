//! Shared appearance for every egui window and viewport.

use okbs_core::config::Theme;

pub(crate) const CONTROL_HEIGHT: f32 = 28.0;
const CONTROL_WIDTH: f32 = 88.0;

/// Installs the application-wide styles when the shared GUI context starts.
pub(crate) fn configure(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        for font in style.text_styles.values_mut() {
            font.size += 1.0;
        }
        style.wrap_mode = Some(egui::TextWrapMode::Wrap);
    });
}

/// Applies the configured color theme to every viewport in the shared context.
pub(crate) fn apply_theme(ctx: &egui::Context, theme: Theme) {
    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
}

/// Applies the common spacing used by window contents and dialogs.
pub(crate) fn content(ui: &mut egui::Ui) {
    let spacing = ui.spacing_mut();
    spacing.interact_size = egui::vec2(CONTROL_WIDTH, CONTROL_HEIGHT);
    spacing.button_padding = egui::vec2(10.0, 5.0);
    spacing.item_spacing = egui::vec2(10.0, 6.0);
    spacing.icon_width = 16.0;
    spacing.icon_width_inner = 9.0;
    spacing.combo_width = 200.0;
    spacing.scroll.dormant_handle_opacity = 0.6;
    spacing.scroll.dormant_background_opacity = 0.15;
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
}

/// Draws a root window body with the shared panel background, outer margins,
/// and content spacing.
pub(crate) fn panel<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::central_panel(ui.style()).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        content(ui);
        add_contents(ui)
    })
}

/// A consistently sized action button used in every application window.
pub(crate) fn action_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text).min_size(egui::vec2(CONTROL_WIDTH, CONTROL_HEIGHT))
}
