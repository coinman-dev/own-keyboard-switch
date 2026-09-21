//! Tray icon images: the layout code on a coloured background, drawn with a
//! built-in 5×7 pixel font (no font files needed).

use crate::flags::Flag;
use okbs_core::Lang;

/// Uppercase Latin letters, 5 columns × 7 rows, most significant bit on the left.
const FONT: [[u8; 7]; 26] = [
    [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
    [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
    [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
    [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
    [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
    [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
    [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
    [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
    [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
    [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
    [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
    [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
    [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
    [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
    [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
    [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
    [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
    [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
    [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
    [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
    [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
    [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
    [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
    [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
    [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04],
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
];

/// Side of the square icon in pixels.
pub const ICON_SIZE: u32 = 32;

/// What the icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconState {
    /// Two-letter layout code shown in text mode (`RU`, `EN`, `UK`).
    pub label: [u8; 2],
    /// Language of the active layout, for the text icon colour.
    pub lang: Option<Lang>,
    /// Flag shown instead of the text («Сделать значок в виде флагов стран»).
    pub flag: Option<Flag>,
    /// Automatic switching is on (the icon is grey or dimmed when off).
    pub autoswitch: bool,
    /// Flags stay bright when automatic switching is off.
    pub full_brightness: bool,
    /// A possible typo was just detected (red).
    pub alert: bool,
}

impl IconState {
    /// Icon for the active layout described by `locale` and `lang`.
    pub fn for_layout(
        general: &okbs_core::config::General,
        locale: Option<&str>,
        lang: Option<Lang>,
        autoswitch: bool,
        alert: bool,
    ) -> Self {
        let flag = if general.tray_flags {
            locale.and_then(|l| crate::flags::flag_for_layout(&general.layout_flags, l))
        } else {
            None
        };
        Self {
            label: label_for(locale, lang),
            lang,
            flag,
            autoswitch,
            full_brightness: general.tray_flags_full_brightness,
            alert,
        }
    }

    /// The label as text.
    pub fn label_text(&self) -> &str {
        std::str::from_utf8(&self.label).unwrap_or("--")
    }
}

/// Two-letter code: `RU`/`EN` for the supported languages, otherwise the
/// language part of the locale (`uk-UA` → `UK`), or `--`.
pub fn label_for(locale: Option<&str>, lang: Option<Lang>) -> [u8; 2] {
    match lang {
        Some(Lang::Ru) => *b"RU",
        Some(Lang::En) => *b"EN",
        None => {
            let letters: Vec<u8> = locale
                .unwrap_or_default()
                .bytes()
                .take_while(u8::is_ascii_alphabetic)
                .map(|b| b.to_ascii_uppercase())
                .collect();
            match letters.as_slice() {
                [a, b, ..] => [*a, *b],
                _ => *b"--",
            }
        }
    }
}

/// Amber: distinguishable from the red EN icon.
const ALERT: [u8; 3] = [0xE8, 0x8A, 0x00];

fn background(state: IconState) -> [u8; 3] {
    if state.alert {
        ALERT
    } else if !state.autoswitch {
        [0x80, 0x80, 0x80]
    } else {
        match state.lang {
            Some(Lang::Ru) => [0x1E, 0x5A, 0xC8],
            Some(Lang::En) => [0xB4, 0x2A, 0x2A],
            None => [0x40, 0x40, 0x40],
        }
    }
}

/// Renders the icon as RGBA pixels, `ICON_SIZE × ICON_SIZE`.
pub fn render(state: IconState) -> Vec<u8> {
    if let Some(flag) = state.flag {
        let dim = !state.autoswitch && !state.full_brightness;
        return crate::flags::render(flag, dim, state.alert);
    }
    let size = ICON_SIZE as usize;
    let mut rgba = vec![0u8; size * size * 4];
    let bg = background(state);
    let radius = 5.0f32;
    for y in 0..size {
        for x in 0..size {
            let cx = (x as f32 + 0.5).clamp(radius, size as f32 - radius);
            let cy = (y as f32 + 0.5).clamp(radius, size as f32 - radius);
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let alpha = (radius + 0.5 - d).clamp(0.0, 1.0);
            let i = (y * size + x) * 4;
            rgba[i..i + 3].copy_from_slice(&bg);
            rgba[i + 3] = (alpha * 255.0) as u8;
        }
    }
    let text = state.label;
    let scale = 2usize;
    let glyph_w = 5 * scale;
    let gap = 2;
    let width = text.len() * glyph_w + (text.len() - 1) * gap;
    let x0 = (size - width) / 2;
    let y0 = (size - 7 * scale) / 2;
    for (n, ch) in text.iter().copied().enumerate() {
        let glyph = match ch {
            b'A'..=b'Z' => FONT[usize::from(ch - b'A')],
            b'-' => [0, 0, 0, 0x1F, 0, 0, 0],
            _ => [0; 7],
        };
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = x0 + n * (glyph_w + gap) + col * scale + dx;
                        let y = y0 + row * scale + dy;
                        let i = (y * size + x) * 4;
                        rgba[i..i + 4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
                    }
                }
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * ICON_SIZE as usize + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    fn state(lang: Option<Lang>) -> IconState {
        IconState {
            label: label_for(None, lang),
            lang,
            flag: None,
            autoswitch: true,
            full_brightness: false,
            alert: false,
        }
    }

    #[test]
    fn renders_background_text_and_corners() {
        let on = state(Some(Lang::Ru));
        let rgba = render(on);
        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert_eq!(pixel(&rgba, 0, 0)[3], 0, "rounded corner is transparent");
        assert_eq!(pixel(&rgba, 16, 2), [0x1E, 0x5A, 0xC8, 0xFF]);
        let white = rgba
            .chunks(4)
            .filter(|p| p == &[0xFF, 0xFF, 0xFF, 0xFF])
            .count();
        assert!(white > 60, "{white} text pixels");

        let off = render(IconState {
            autoswitch: false,
            ..on
        });
        assert_eq!(pixel(&off, 16, 2), [0x80, 0x80, 0x80, 0xFF]);
        let alert = render(IconState { alert: true, ..on });
        assert_eq!(pixel(&alert, 16, 2), [0xE8, 0x8A, 0x00, 0xFF]);
        assert_ne!(render(state(Some(Lang::En))), rgba);
    }

    #[test]
    fn labels_and_flags_from_settings() {
        assert_eq!(&label_for(Some("uk-UA"), None), b"UK");
        assert_eq!(&label_for(Some("ru-RU"), Some(Lang::Ru)), b"RU");
        assert_eq!(&label_for(None, None), b"--");

        let mut general = okbs_core::config::General::default();
        let text = IconState::for_layout(&general, Some("en-GB"), Some(Lang::En), true, false);
        assert_eq!((text.flag, text.label_text()), (None, "EN"));

        general.tray_flags = true;
        let gb = IconState::for_layout(&general, Some("en-GB"), Some(Lang::En), true, false);
        assert_eq!(gb.flag, Some(Flag::Gb));
        general.layout_flags.insert("en-GB".into(), "us".into());
        let us = IconState::for_layout(&general, Some("en-GB"), Some(Lang::En), false, false);
        assert_eq!(us.flag, Some(Flag::Us));
        assert_eq!(
            render(us),
            crate::flags::render(Flag::Us, true, false),
            "dimmed when auto switch is off"
        );
        general.tray_flags_full_brightness = true;
        let bright = IconState::for_layout(&general, Some("en-GB"), Some(Lang::En), false, false);
        assert_eq!(render(bright), crate::flags::render(Flag::Us, false, false));
        let unknown = IconState::for_layout(&general, Some("ja-JP"), None, true, false);
        assert_eq!((unknown.flag, unknown.label_text()), (None, "JA"));
    }
}
