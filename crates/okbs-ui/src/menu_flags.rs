//! Country flags beside the keyboard layouts in the tray menu (Windows).
//!
//! A native menu shows an item picture only in its left column, where
//! Windows 11 also draws the check mark, so the flag is set as the item's
//! bitmap and the active layout is shown in bold instead.
#![allow(unsafe_code)]

use crate::flags::{self, Flag};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, HMENU, MENUITEMINFOW, MIIM_BITMAP, SM_CYMENUCHECK, SetMenuDefaultItem,
    SetMenuItemInfoW,
};

/// Shows the item at `position` of `menu` in bold, or none. A picture takes
/// the place of the check mark, so this is how the active layout stands out.
pub(crate) fn set_bold(menu: isize, position: Option<u32>) {
    // SAFETY: `menu` is the live popup menu of the tray icon.
    let _ = unsafe { SetMenuDefaultItem(HMENU(menu as *mut _), position.unwrap_or(u32::MAX), 1) };
}

/// Rows of the framed flag inside the 32 × 32 picture of [`flags::render`].
const TOP: usize = 5;
const ROWS: usize = 22;
const SIDE: usize = 32;

/// Pictures used by a menu; they are deleted after the menu is gone.
#[derive(Debug, Default)]
pub(crate) struct MenuBitmaps(Vec<HBITMAP>);

impl Drop for MenuBitmaps {
    fn drop(&mut self) {
        for bitmap in self.0.drain(..) {
            // SAFETY: created by this module; the menu showing it no longer exists.
            let _ = unsafe { DeleteObject(bitmap.into()) };
        }
    }
}

impl MenuBitmaps {
    /// Shows `flag` before the text of the item at `position` of `menu`.
    pub(crate) fn set(&mut self, menu: isize, position: u32, flag: Flag) {
        // SAFETY: GetSystemMetrics has no pointer arguments.
        let check = unsafe { GetSystemMetrics(SM_CYMENUCHECK) };
        let Some(bitmap) = flag_bitmap(flag, (check * 4 / 5).max(12) as usize) else {
            return;
        };
        let info = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_BITMAP,
            hbmpItem: bitmap,
            ..Default::default()
        };
        // SAFETY: `menu` is the live popup menu of the tray icon; `info` is valid.
        if unsafe { SetMenuItemInfoW(HMENU(menu as *mut _), position, true, &info) }.is_ok() {
            self.0.push(bitmap);
        } else {
            // SAFETY: the bitmap is not used by any menu.
            let _ = unsafe { DeleteObject(bitmap.into()) };
        }
    }
}

/// The framed flag scaled to `height` pixels, as the premultiplied 32-bit
/// picture a menu blends with its background.
fn flag_bitmap(flag: Flag, height: usize) -> Option<HBITMAP> {
    let width = (height * SIDE + ROWS / 2) / ROWS;
    let pixels = scale(&flags::render(flag, false, false), width, height);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // Negative: rows go from top to bottom.
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    // SAFETY: `info` describes a top-down 32-bit bitmap; `bits` receives its memory.
    let bitmap =
        unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    if bits.is_null() {
        // SAFETY: the bitmap was just created and is not used anywhere.
        let _ = unsafe { DeleteObject(bitmap.into()) };
        return None;
    }
    // SAFETY: the DIB section holds exactly `width * height` 4-byte pixels.
    unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), pixels.len()) }
        .copy_from_slice(&pixels);
    Some(bitmap)
}

/// Averages 4 × 4 samples per pixel of the framed flag; returns premultiplied BGRA.
fn scale(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    const SAMPLES: usize = 4;
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let fx = (x * SAMPLES + sx) * SIDE / (width * SAMPLES);
                    let fy = TOP + (y * SAMPLES + sy) * ROWS / (height * SAMPLES);
                    let pixel = &rgba[(fy * SIDE + fx) * 4..][..4];
                    let alpha = u32::from(pixel[3]);
                    for (total, channel) in sum.iter_mut().zip([pixel[2], pixel[1], pixel[0]]) {
                        *total += u32::from(channel) * alpha / 255;
                    }
                    sum[3] += alpha;
                }
            }
            out.extend(sum.map(|total| (total / (SAMPLES * SAMPLES) as u32) as u8));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_flag_keeps_its_colours_and_proportions() {
        let pixels = scale(&flags::render(Flag::Ru, false, false), 23, 16);
        assert_eq!(pixels.len(), 23 * 16 * 4);
        // Opaque inside, white-over-blue-over-red rows of the Russian flag.
        let at = |x: usize, y: usize| &pixels[(y * 23 + x) * 4..][..4];
        assert_eq!(at(11, 3)[3], 0xFF);
        assert!(
            at(11, 3)[..3].iter().all(|&c| c > 0xE0),
            "white: {:?}",
            at(11, 3)
        );
        assert!(at(11, 8)[0] > at(11, 8)[2], "blue: {:?}", at(11, 8));
        assert!(at(11, 13)[2] > at(11, 13)[0], "red: {:?}", at(11, 13));
    }
}
