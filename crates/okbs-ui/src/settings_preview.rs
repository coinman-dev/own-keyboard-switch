//! Manual visual check without opening a desktop window:
//! `cargo test -p okbs-ui render_previews -- --ignored`
//! Images go to `target/settings-preview/`. The PNG encoder is test-only.

use super::*;
use egui::{ColorImage, ImageData, TextureId};
use std::collections::HashMap;

fn texture_updates(textures: &mut HashMap<TextureId, ColorImage>, output: &mut egui::FullOutput) {
    for (id, deltas) in &output.textures_delta.set {
        for delta in deltas {
            let ImageData::Color(image) = &delta.image;
            if let Some([x, y]) = delta.pos {
                let texture = textures.get_mut(id).expect("existing texture");
                for row in 0..image.size[1] {
                    let start = (y + row) * texture.size[0] + x;
                    texture.pixels[start..start + image.size[0]].copy_from_slice(
                        &image.pixels[row * image.size[0]..(row + 1) * image.size[0]],
                    );
                }
            } else {
                textures.insert(*id, (**image).clone());
            }
        }
    }
    output.textures_delta.clear();
}

fn rasterize(
    ctx: &egui::Context,
    output: egui::FullOutput,
    textures: &HashMap<TextureId, ColorImage>,
) -> Vec<u8> {
    let width = WINDOW_SIZE[0] as usize;
    let height = WINDOW_SIZE[1] as usize;
    let mut pixels = vec![0u8; width * height * 3];
    for primitive in ctx.tessellate(output.shapes, 1.0) {
        let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else {
            continue;
        };
        let texture = &textures[&mesh.texture_id];
        for indices in mesh.indices.as_chunks::<3>().0 {
            let vertices = [
                mesh.vertices[indices[0] as usize],
                mesh.vertices[indices[1] as usize],
                mesh.vertices[indices[2] as usize],
            ];
            let [a, b, c] = vertices.map(|v| v.pos);
            let cross = |u: egui::Vec2, v: egui::Vec2| u.x * v.y - u.y * v.x;
            let area = cross(b - a, c - a);
            if area.abs() < 0.0001 {
                continue;
            }
            let bounds = egui::Rect::from_points(&[a, b, c]).intersect(primitive.clip_rect);
            for y in (bounds.top().floor().max(0.0) as usize)
                ..(bounds.bottom().ceil().min(height as f32) as usize)
            {
                for x in (bounds.left().floor().max(0.0) as usize)
                    ..(bounds.right().ceil().min(width as f32) as usize)
                {
                    let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
                    if !primitive.clip_rect.contains(p) {
                        continue;
                    }
                    let weights = [
                        cross(b - p, c - p) / area,
                        cross(c - p, a - p) / area,
                        cross(a - p, b - p) / area,
                    ];
                    if weights.iter().any(|w| *w < -0.00001) {
                        continue;
                    }
                    let mut uv = egui::Vec2::ZERO;
                    let mut color = [0.0f32; 4];
                    for (vertex, weight) in vertices.iter().zip(weights) {
                        uv += vertex.uv.to_vec2() * weight;
                        for (channel, value) in color.iter_mut().zip(vertex.color.to_array()) {
                            *channel += f32::from(value) * weight / 255.0;
                        }
                    }
                    let tx = ((uv.x * texture.size[0] as f32) as usize).min(texture.size[0] - 1);
                    let ty = ((uv.y * texture.size[1] as f32) as usize).min(texture.size[1] - 1);
                    let texel = texture.pixels[ty * texture.size[0] + tx].to_array();
                    let alpha = color[3] * f32::from(texel[3]) / 255.0;
                    let base = (y * width + x) * 3;
                    for channel in 0..3 {
                        pixels[base + channel] = (color[channel] * f32::from(texel[channel])
                            + f32::from(pixels[base + channel]) * (1.0 - alpha))
                            .clamp(0.0, 255.0)
                            as u8;
                    }
                }
            }
        }
    }
    pixels
}

#[test]
#[ignore = "generates local preview artifacts for manual inspection"]
fn render_previews() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/settings-preview");
    std::fs::create_dir_all(&directory).expect("preview directory");
    for lang in Lang::ALL {
        for section in Section::ALL {
            let ctx = egui::Context::default();
            configure_context(&ctx);
            ctx.set_theme(egui::ThemePreference::Dark);
            let mut config = Config::default();
            config.general.tray_flags = true;
            let mut view = SettingsView::new(config, section, lang);
            view.set_layouts(vec![
                LayoutEntry {
                    locale: "en-US".into(),
                    name: match lang {
                        Lang::Ru => "Английский (США)",
                        Lang::En => "English (United States)",
                    }
                    .into(),
                },
                LayoutEntry {
                    locale: "ru-RU".into(),
                    name: match lang {
                        Lang::Ru => "Русский (Россия)",
                        Lang::En => "Russian (Russia)",
                    }
                    .into(),
                },
            ]);
            let mut textures = HashMap::new();
            for frame in 0..4 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            WINDOW_SIZE.into(),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        view.ui(ui, &mut Vec::new());
                    },
                );
                texture_updates(&mut textures, &mut output);
                if frame == 3 {
                    let pixels = rasterize(&ctx, output, &textures);
                    let filename = match lang {
                        Lang::Ru => format!("{section:?}.png"),
                        Lang::En => format!("{section:?}-en.png"),
                    };
                    let file =
                        std::fs::File::create(directory.join(filename)).expect("preview file");
                    let mut encoder =
                        png::Encoder::new(file, WINDOW_SIZE[0] as u32, WINDOW_SIZE[1] as u32);
                    encoder.set_color(png::ColorType::Rgb);
                    encoder.set_depth(png::BitDepth::Eight);
                    encoder
                        .write_header()
                        .expect("PNG header")
                        .write_image_data(&pixels)
                        .expect("PNG pixels");
                }
            }
        }
    }
}
