//! Country flags for the tray icon («Сделать значок в виде флагов стран»).
//!
//! Flags are drawn in code as simplified 30×20 pictures, so no image files
//! with separate licenses are needed.

use okbs_core::Lang;

/// A flag the tray icon can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Flag {
    /// Russia.
    Ru,
    /// United States.
    Us,
    /// United Kingdom.
    Gb,
    /// Ukraine.
    Ua,
    /// Belarus.
    By,
    /// Kazakhstan.
    Kz,
    /// Germany.
    De,
    /// France.
    Fr,
    /// Italy.
    It,
    /// Spain.
    Es,
    /// Poland.
    Pl,
}

impl Flag {
    /// All flags in the order of the settings list.
    pub const ALL: [Flag; 11] = [
        Flag::Ru,
        Flag::Us,
        Flag::Gb,
        Flag::Ua,
        Flag::By,
        Flag::Kz,
        Flag::De,
        Flag::Fr,
        Flag::It,
        Flag::Es,
        Flag::Pl,
    ];

    /// ISO 3166-1 alpha-2 code in lowercase, as stored in the configuration.
    pub const fn code(self) -> &'static str {
        match self {
            Flag::Ru => "ru",
            Flag::Us => "us",
            Flag::Gb => "gb",
            Flag::Ua => "ua",
            Flag::By => "by",
            Flag::Kz => "kz",
            Flag::De => "de",
            Flag::Fr => "fr",
            Flag::It => "it",
            Flag::Es => "es",
            Flag::Pl => "pl",
        }
    }

    /// Parses a code (`gb`, `GB`; `uk` is accepted for the United Kingdom).
    pub fn from_code(code: &str) -> Option<Flag> {
        let code = code.trim().to_ascii_lowercase();
        if code == "uk" {
            return Some(Flag::Gb);
        }
        Flag::ALL.into_iter().find(|f| f.code() == code)
    }

    /// Country name in the interface language.
    pub const fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Flag::Ru, Lang::Ru) => "Россия",
            (Flag::Ru, Lang::En) => "Russia",
            (Flag::Us, Lang::Ru) => "США",
            (Flag::Us, Lang::En) => "United States",
            (Flag::Gb, Lang::Ru) => "Великобритания",
            (Flag::Gb, Lang::En) => "United Kingdom",
            (Flag::Ua, Lang::Ru) => "Украина",
            (Flag::Ua, Lang::En) => "Ukraine",
            (Flag::By, Lang::Ru) => "Беларусь",
            (Flag::By, Lang::En) => "Belarus",
            (Flag::Kz, Lang::Ru) => "Казахстан",
            (Flag::Kz, Lang::En) => "Kazakhstan",
            (Flag::De, Lang::Ru) => "Германия",
            (Flag::De, Lang::En) => "Germany",
            (Flag::Fr, Lang::Ru) => "Франция",
            (Flag::Fr, Lang::En) => "France",
            (Flag::It, Lang::Ru) => "Италия",
            (Flag::It, Lang::En) => "Italy",
            (Flag::Es, Lang::Ru) => "Испания",
            (Flag::Es, Lang::En) => "Spain",
            (Flag::Pl, Lang::Ru) => "Польша",
            (Flag::Pl, Lang::En) => "Poland",
        }
    }

    /// Flag of a locale's region, or of its language when there is no region:
    /// `en-GB` → United Kingdom, `en` → United States, `uk-UA` → Ukraine.
    pub fn for_locale(locale: &str) -> Option<Flag> {
        let mut parts = locale.split(['-', '_']);
        let language = parts.next().unwrap_or_default().to_ascii_lowercase();
        let region = parts.find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()));
        if let Some(flag) = region.and_then(Flag::from_code) {
            return Some(flag);
        }
        match language.as_str() {
            "ru" => Some(Flag::Ru),
            "en" => Some(Flag::Us),
            "uk" => Some(Flag::Ua),
            "be" => Some(Flag::By),
            "kk" => Some(Flag::Kz),
            "de" => Some(Flag::De),
            "fr" => Some(Flag::Fr),
            "it" => Some(Flag::It),
            "es" => Some(Flag::Es),
            "pl" => Some(Flag::Pl),
            _ => None,
        }
    }
}

/// Flag chosen in the settings for a layout, or the flag of its region.
pub fn flag_for_layout(
    layout_flags: &std::collections::BTreeMap<String, String>,
    locale: &str,
) -> Option<Flag> {
    layout_flags
        .get(locale)
        .and_then(|code| Flag::from_code(code))
        .or_else(|| Flag::for_locale(locale))
}

/// Side of the icon canvas.
const SIZE: usize = 32;
/// Flag rectangle inside the canvas: x in `X0..X1`, y in `Y0..Y1` (30×20).
const X0: usize = 1;
const X1: usize = 31;
const Y0: usize = 6;
const Y1: usize = 26;

type Rgb = [u8; 3];

/// An RGBA canvas of `SIZE × SIZE` pixels.
pub(crate) struct Canvas {
    pub(crate) pixels: Vec<u8>,
}

impl Canvas {
    pub(crate) fn new() -> Self {
        Self {
            pixels: vec![0; SIZE * SIZE * 4],
        }
    }

    pub(crate) fn set(&mut self, x: usize, y: usize, rgb: Rgb) {
        if x < SIZE && y < SIZE {
            let i = (y * SIZE + x) * 4;
            self.pixels[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 0xFF]);
        }
    }

    pub(crate) fn rect(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, rgb: Rgb) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.set(x, y, rgb);
            }
        }
    }

    /// Paints flag pixels whose centre is within `half_width` of the line through `a` and `b`.
    fn band(&mut self, a: (f32, f32), b: (f32, f32), half_width: f32, rgb: Rgb) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        for y in Y0..Y1 {
            for x in X0..X1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let distance = ((px - a.0) * dy - (py - a.1) * dx).abs() / len;
                if distance <= half_width {
                    self.set(x, y, rgb);
                }
            }
        }
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, rgb: Rgb) {
        for y in Y0..Y1 {
            for x in X0..X1 {
                let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if px * px + py * py <= r * r {
                    self.set(x, y, rgb);
                }
            }
        }
    }
}

fn horizontal(c: &mut Canvas, stripes: &[(Rgb, usize)]) {
    let total: usize = stripes.iter().map(|s| s.1).sum();
    let mut y = Y0;
    for (i, &(rgb, weight)) in stripes.iter().enumerate() {
        let end = if i + 1 == stripes.len() {
            Y1
        } else {
            y + (Y1 - Y0) * weight / total
        };
        c.rect(X0, y, X1, end, rgb);
        y = end;
    }
}

fn vertical(c: &mut Canvas, colors: [Rgb; 3]) {
    let w = (X1 - X0) / 3;
    for (i, rgb) in colors.into_iter().enumerate() {
        let x1 = if i == 2 { X1 } else { X0 + w * (i + 1) };
        c.rect(X0 + w * i, Y0, x1, Y1, rgb);
    }
}

const WHITE: Rgb = [0xFF, 0xFF, 0xFF];

/// Draws `flag` into the flag rectangle of the canvas.
pub(crate) fn draw(c: &mut Canvas, flag: Flag) {
    match flag {
        Flag::Ru => horizontal(
            c,
            &[(WHITE, 1), ([0x00, 0x39, 0xA6], 1), ([0xD5, 0x2B, 0x1E], 1)],
        ),
        Flag::Ua => horizontal(c, &[([0x00, 0x57, 0xB7], 1), ([0xFF, 0xD7, 0x00], 1)]),
        Flag::Pl => horizontal(c, &[(WHITE, 1), ([0xDC, 0x14, 0x3C], 1)]),
        Flag::De => horizontal(
            c,
            &[
                ([0x00, 0x00, 0x00], 1),
                ([0xDD, 0x00, 0x00], 1),
                ([0xFF, 0xCE, 0x00], 1),
            ],
        ),
        Flag::Es => horizontal(
            c,
            &[
                ([0xAA, 0x15, 0x1B], 1),
                ([0xF1, 0xBF, 0x00], 2),
                ([0xAA, 0x15, 0x1B], 1),
            ],
        ),
        Flag::Fr => vertical(c, [[0x00, 0x23, 0x95], WHITE, [0xED, 0x29, 0x39]]),
        Flag::It => vertical(c, [[0x00, 0x92, 0x46], WHITE, [0xCE, 0x2B, 0x37]]),
        Flag::By => {
            horizontal(c, &[([0xC8, 0x31, 0x3E], 2), ([0x4A, 0xA6, 0x57], 1)]);
            c.rect(X0, Y0, X0 + 5, Y1, WHITE);
            for y in (Y0..Y1).step_by(3) {
                c.rect(
                    X0 + 1 + (y / 3) % 2 * 2,
                    y,
                    X0 + 2 + (y / 3) % 2 * 2,
                    y + 2,
                    [0xC8, 0x31, 0x3E],
                );
            }
        }
        Flag::Kz => {
            c.rect(X0, Y0, X1, Y1, [0x00, 0xAF, 0xCA]);
            c.circle(17.0, 15.0, 4.5, [0xFE, 0xC5, 0x0C]);
            for y in (Y0 + 1..Y1 - 1).step_by(2) {
                c.set(X0 + 2, y, [0xFE, 0xC5, 0x0C]);
                c.set(X0 + 3, y + 1, [0xFE, 0xC5, 0x0C]);
            }
        }
        Flag::Us => {
            let height = Y1 - Y0;
            for y in Y0..Y1 {
                let stripe = (y - Y0) * 13 / height;
                let rgb = if stripe.is_multiple_of(2) {
                    [0xB2, 0x22, 0x34]
                } else {
                    WHITE
                };
                c.rect(X0, y, X1, y + 1, rgb);
            }
            c.rect(X0, Y0, X0 + 13, Y0 + 11, [0x3C, 0x3B, 0x6E]);
            for y in (Y0 + 1..Y0 + 10).step_by(2) {
                for x in (X0 + 1 + (y - Y0) / 2 % 2..X0 + 12).step_by(2) {
                    c.set(x, y, WHITE);
                }
            }
        }
        Flag::Gb => {
            c.rect(X0, Y0, X1, Y1, [0x01, 0x21, 0x69]);
            let (l, r, t, b) = (X0 as f32, X1 as f32, Y0 as f32, Y1 as f32);
            c.band((l, t), (r, b), 2.6, WHITE);
            c.band((l, b), (r, t), 2.6, WHITE);
            c.band((l, t), (r, b), 1.0, [0xC8, 0x10, 0x2E]);
            c.band((l, b), (r, t), 1.0, [0xC8, 0x10, 0x2E]);
            let (cx, cy) = ((X0 + X1) / 2, (Y0 + Y1) / 2);
            c.rect(cx - 3, Y0, cx + 3, Y1, WHITE);
            c.rect(X0, cy - 3, X1, cy + 3, WHITE);
            c.rect(cx - 2, Y0, cx + 2, Y1, [0xC8, 0x10, 0x2E]);
            c.rect(X0, cy - 2, X1, cy + 2, [0xC8, 0x10, 0x2E]);
        }
    }
}

/// Renders a flag icon: the flag with a thin frame, dimmed when `dim`, with a
/// red frame when `alert`.
pub fn render(flag: Flag, dim: bool, alert: bool) -> Vec<u8> {
    let mut c = Canvas::new();
    let frame = if alert {
        [0xE8, 0x8A, 0x00]
    } else {
        [0x50, 0x50, 0x50]
    };
    let pad = if alert { 2 } else { 1 };
    c.rect(X0 - 1, Y0 - pad, X1 + 1, Y1 + pad, frame);
    if alert {
        c.rect(0, Y0 - 2, SIZE, Y1 + 2, frame);
    }
    draw(&mut c, flag);
    if dim {
        for px in c.pixels.as_chunks_mut::<4>().0 {
            if px[3] == 0 {
                continue;
            }
            let luma =
                (0.30 * f32::from(px[0]) + 0.59 * f32::from(px[1]) + 0.11 * f32::from(px[2])) as u8;
            for channel in &mut px[..3] {
                *channel = ((u16::from(*channel) * 3 + u16::from(luma) * 7) / 10) as u8;
            }
            px[3] = 0xA0;
        }
    }
    c.pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * SIZE + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn locales_and_codes() {
        assert_eq!(Flag::for_locale("ru-RU"), Some(Flag::Ru));
        assert_eq!(Flag::for_locale("en-US"), Some(Flag::Us));
        assert_eq!(Flag::for_locale("en-GB"), Some(Flag::Gb));
        assert_eq!(Flag::for_locale("en"), Some(Flag::Us));
        assert_eq!(Flag::for_locale("uk-UA"), Some(Flag::Ua));
        assert_eq!(Flag::for_locale("sr-Cyrl-RS"), None);
        assert_eq!(Flag::for_locale("ja-JP"), None);
        assert_eq!(Flag::from_code("UK"), Some(Flag::Gb));
        for flag in Flag::ALL {
            assert_eq!(Flag::from_code(flag.code()), Some(flag));
            assert!(!flag.name(Lang::Ru).is_empty() && !flag.name(Lang::En).is_empty());
        }
        let mut chosen = std::collections::BTreeMap::new();
        assert_eq!(flag_for_layout(&chosen, "en-US"), Some(Flag::Us));
        chosen.insert("en-US".to_string(), "gb".to_string());
        assert_eq!(flag_for_layout(&chosen, "en-US"), Some(Flag::Gb));
        chosen.insert("ru-RU".to_string(), "bogus".to_string());
        assert_eq!(flag_for_layout(&chosen, "ru-RU"), Some(Flag::Ru));
    }

    #[test]
    fn flags_have_their_colors() {
        let ru = render(Flag::Ru, false, false);
        assert_eq!(pixel(&ru, 16, Y0 + 1), [0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(pixel(&ru, 16, 16), [0x00, 0x39, 0xA6, 0xFF]);
        assert_eq!(pixel(&ru, 16, Y1 - 1), [0xD5, 0x2B, 0x1E, 0xFF]);
        assert_eq!(pixel(&ru, 16, 1)[3], 0, "transparent above the flag");

        let gb = render(Flag::Gb, false, false);
        assert_eq!(
            pixel(&gb, 16, 16),
            [0xC8, 0x10, 0x2E, 0xFF],
            "red cross in the centre"
        );
        assert_eq!(
            pixel(&gb, 11, Y0 + 1)[..3],
            [0x01, 0x21, 0x69],
            "blue field"
        );

        let us = render(Flag::Us, false, false);
        assert_eq!(pixel(&us, X0, Y0)[..3], [0x3C, 0x3B, 0x6E], "blue canton");
        assert_eq!(
            pixel(&us, X1 - 1, Y0)[..3],
            [0xB2, 0x22, 0x34],
            "red top stripe"
        );

        for flag in Flag::ALL {
            let rgba = render(flag, false, false);
            assert_ne!(pixel(&rgba, 16, 16)[3], 0, "{flag:?}");
        }
        let distinct: std::collections::HashSet<Vec<u8>> =
            Flag::ALL.iter().map(|&f| render(f, false, false)).collect();
        assert_eq!(distinct.len(), Flag::ALL.len());
    }

    #[test]
    fn dim_and_alert() {
        let normal = render(Flag::Ua, false, false);
        let dim = render(Flag::Ua, true, false);
        let n = pixel(&normal, 16, Y1 - 2);
        let d = pixel(&dim, 16, Y1 - 2);
        assert!(d[3] < n[3]);
        assert!(d[0].abs_diff(d[2]) < n[0].abs_diff(n[2]), "less saturated");
        let alert = render(Flag::Ua, false, true);
        assert_eq!(pixel(&alert, 0, 16), [0xE8, 0x8A, 0x00, 0xFF]);
    }
}
