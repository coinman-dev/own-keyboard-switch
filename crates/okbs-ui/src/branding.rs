//! Embedded application logo shared by the settings view and native windows.

use std::sync::{Arc, OnceLock};

pub(crate) fn icon() -> Arc<egui::IconData> {
    static ICON: OnceLock<Arc<egui::IconData>> = OnceLock::new();
    ICON.get_or_init(|| {
        let image = image::load_from_memory(include_bytes!("../../../images/logo-256.png"))
            .expect("embedded application logo is a valid PNG")
            .into_rgba8();
        Arc::new(egui::IconData {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        })
    })
    .clone()
}

pub(crate) fn show(ui: &mut egui::Ui) {
    // Reduce the current presentation by a further 5%.
    let size = ui.available_width().min(174.0 * 0.95 * 0.95);
    let id = egui::Id::new("okbswitch_logo_texture");
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let pixels = image::load_from_memory(include_bytes!("../../../images/logo-settings.png"))
            .expect("embedded settings logo is a valid PNG")
            .into_rgba8();
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [pixels.width() as usize, pixels.height() as usize],
            pixels.as_raw(),
        );
        let texture = ui
            .ctx()
            .load_texture("okbswitch_logo", image, egui::TextureOptions::LINEAR);
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    ui.add(
        egui::Image::new((texture.id(), egui::vec2(size, size)))
            .tint(egui::Color32::WHITE.gamma_multiply(0.95 * 0.95)),
    );
}
