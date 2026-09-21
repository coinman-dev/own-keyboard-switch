//! «Показывать плавающий индикатор»: a small always-on-top window with the
//! same picture as the tray icon.
//!
//! The window is layered, so the rounded icon keeps its transparency, and
//! carries `WS_EX_NOACTIVATE`, so neither a drag nor a click ever takes the
//! input focus away from the editor the user types in. It lives on the thread
//! that pumps messages, that is the main thread of the application.
#![allow(unsafe_code)]

use crate::hook::os_error;
use crossbeam_channel::{Receiver, Sender, unbounded};
use okbs_platform::{FloatingIndicator, IndicatorEvent, IndicatorLabels, IndicatorState, Result};
use std::cell::RefCell;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
    GetMonitorInfoW, HBITMAP, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, w};

const CLASS: PCWSTR = w!("OwnKeyboardSwitch.Indicator");
/// Side of the indicator at 100% scaling, in logical points.
const SIDE: i32 = 32;
/// Distance kept from the edges of the working area when no position is saved.
const MARGIN: i32 = 48;
const AUTOHIDE_TIMER: usize = 1;
const MENU_LOCK: usize = 201;
const MENU_SETTINGS: usize = 202;
const MENU_HIDE: usize = 203;

/// Why the visibility is being decided again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    /// The layout changed: with auto-hide the indicator appears for a moment.
    LayoutChanged,
    /// Only the picture changed; an auto-hide countdown keeps running.
    Redraw,
    /// New settings: an auto-hide indicator goes back to being hidden.
    Configured,
}

/// A square RGBA picture, `source` pixels on a side.
#[derive(Debug, Default, Clone)]
struct Picture {
    rgba: Vec<u8>,
    source: u32,
}

struct IndicatorWindow {
    window: HWND,
    state: IndicatorState,
    labels: IndicatorLabels,
    /// Last picture, kept to repaint after a move or a DPI change.
    picture: Picture,
    events: Sender<IndicatorEvent>,
}

thread_local! {
    static INDICATOR: RefCell<Option<IndicatorWindow>> = const { RefCell::new(None) };
}

/// Physical side of the indicator for the monitor the window is on.
fn side_for(window: HWND) -> i32 {
    // SAFETY: a live window handle; the call falls back to 96 on failure.
    let dpi = unsafe { GetDpiForWindow(window) };
    let dpi = if dpi == 0 { 96 } else { dpi };
    (SIDE * i32::try_from(dpi).unwrap_or(96) / 96).max(16)
}

/// Keeps `[x, y, side, side]` inside the working area of its monitor.
fn clamp(position: [i32; 2], side: i32) -> [i32; 2] {
    let [x, y] = position;
    let point = POINT { x, y };
    // SAFETY: `point` is a plain value; `info` is sized before the call.
    let work = unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return position;
        }
        info.rcWork
    };
    if work.right <= work.left || work.bottom <= work.top {
        return position;
    }
    [
        x.clamp(work.left, (work.right - side).max(work.left)),
        y.clamp(work.top, (work.bottom - side).max(work.top)),
    ]
}

/// Top right of the primary working area, where the indicator is first shown.
fn default_position(side: i32) -> [i32; 2] {
    let point = POINT { x: 0, y: 0 };
    // SAFETY: `info` is sized before the call; a failure falls back to a corner.
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return [MARGIN, MARGIN];
        }
        let work = info.rcWork;
        clamp([work.right - side - MARGIN, work.top + MARGIN], side)
    }
}

/// Nearest-neighbour scaling of a square RGBA picture into premultiplied BGRA,
/// the format `UpdateLayeredWindow` expects, bottom-up as a DIB is stored.
fn premultiplied_bgra(rgba: &[u8], source: u32, side: i32) -> Vec<u8> {
    let side = usize::try_from(side).unwrap_or(0);
    let source = usize::try_from(source).unwrap_or(0);
    let mut out = vec![0_u8; side * side * 4];
    if source == 0 || rgba.len() < source * source * 4 {
        return out;
    }
    for y in 0..side {
        // A DIB with a positive height keeps the first row at the bottom.
        let src_y = (side - 1 - y) * source / side;
        for x in 0..side {
            let src = (src_y * source + x * source / side) * 4;
            let alpha = u32::from(rgba[src + 3]);
            let scale = |channel: u8| (u32::from(channel) * alpha / 255) as u8;
            let dst = (y * side + x) * 4;
            out[dst] = scale(rgba[src + 2]);
            out[dst + 1] = scale(rgba[src + 1]);
            out[dst + 2] = scale(rgba[src]);
            out[dst + 3] = rgba[src + 3];
        }
    }
    out
}

/// Draws the current picture at `position` and resizes the window to `side`.
fn repaint(window: HWND, picture: &Picture, position: [i32; 2], side: i32) -> Result<()> {
    let pixels = premultiplied_bgra(&picture.rgba, picture.source, side);
    if pixels.is_empty() {
        return Ok(());
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: side,
            biHeight: side,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: every handle below is released on all paths; `bits` points into
    // a DIB of exactly `side * side * 4` bytes, the size of `pixels`.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap: HBITMAP =
            match CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) if !bits.is_null() => bitmap,
                result => {
                    let _ = DeleteDC(memory);
                    ReleaseDC(None, screen);
                    let _ = result.map(|bitmap| DeleteObject(HGDIOBJ(bitmap.0)));
                    return Err(os_error(
                        "CreateDIBSection(indicator)",
                        &windows::core::Error::from_thread(),
                    ));
                }
            };
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let top_left = POINT {
            x: position[0],
            y: position[1],
        };
        let size = windows::Win32::Foundation::SIZE { cx: side, cy: side };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let result = UpdateLayeredWindow(
            window,
            Some(screen),
            Some(&top_left),
            Some(&size),
            Some(memory),
            Some(&origin),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result.map_err(|err| os_error("UpdateLayeredWindow(indicator)", &err))
    }
}

/// Current outer position of the window, in physical pixels.
fn window_position(window: HWND) -> Option<[i32; 2]> {
    let mut rect = RECT::default();
    // SAFETY: a live window handle and a valid out pointer.
    unsafe { GetWindowRect(window, &mut rect) }.ok()?;
    Some([rect.left, rect.top])
}

/// Shows the context menu. `TrackPopupMenu` runs its own message loop and
/// sends `WM_COMMAND` back to this window from inside it, so the caller must
/// not hold a borrow of [`INDICATOR`] while this runs: the command handler
/// needs it mutably.
fn show_menu(window: HWND, labels: &IndicatorLabels, locked: bool) {
    // SAFETY: the menu is created and destroyed here; the window handle is live.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let lock = HSTRING::from(labels.lock.as_str());
        let settings = HSTRING::from(labels.settings.as_str());
        let hide = HSTRING::from(labels.hide.as_str());
        let checked = if locked { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | checked, MENU_LOCK, PCWSTR(lock.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS, PCWSTR(settings.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, MENU_HIDE, PCWSTR(hide.as_ptr()));
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        // A popup of a non-activating window only closes on an outside click
        // when its owner is in the foreground first; the click is the user's
        // own deliberate action, and WM_NULL lets the menu dismiss normally.
        let _ = SetForegroundWindow(window);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            None,
            window,
            None,
        );
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

fn send(event: IndicatorEvent) {
    INDICATOR.with_borrow(|state| {
        if let Some(state) = state {
            let _ = state.events.send(event);
        }
    });
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // Never take the focus: the user keeps typing in the other window.
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN => {
            let locked = INDICATOR.with_borrow(|s| s.as_ref().is_some_and(|s| s.state.locked));
            if !locked {
                // SAFETY: hands the drag to the system move loop of this window.
                unsafe {
                    let _ = ReleaseCapture();
                    SendMessageW(
                        hwnd,
                        WM_NCLBUTTONDOWN,
                        Some(WPARAM(HTCAPTION as usize)),
                        Some(LPARAM(0)),
                    );
                }
            }
            return LRESULT(0);
        }
        WM_LBUTTONDBLCLK => {
            send(IndicatorEvent::OpenSettings);
            return LRESULT(0);
        }
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            // Copy what the menu needs and drop the borrow before tracking it.
            let menu = INDICATOR.with_borrow(|state| {
                state
                    .as_ref()
                    .map(|state| (state.window, state.labels.clone(), state.state.locked))
            });
            if let Some((window, labels, locked)) = menu {
                show_menu(window, &labels, locked);
            }
            return LRESULT(0);
        }
        WM_EXITSIZEMOVE => {
            if let Some(position) = window_position(hwnd) {
                INDICATOR.with_borrow_mut(|state| {
                    if let Some(state) = state {
                        state.state.position = Some(position);
                    }
                });
                send(IndicatorEvent::Moved(position));
            }
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == AUTOHIDE_TIMER => {
            // SAFETY: the timer was set for this window by `set_icon`.
            unsafe {
                let _ = KillTimer(Some(hwnd), AUTOHIDE_TIMER);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            return LRESULT(0);
        }
        WM_COMMAND => {
            match wparam.0 & 0xffff {
                MENU_LOCK => {
                    let locked = INDICATOR.with_borrow_mut(|state| {
                        state.as_mut().map(|state| {
                            state.state.locked = !state.state.locked;
                            state.state.locked
                        })
                    });
                    if let Some(locked) = locked {
                        send(IndicatorEvent::Locked(locked));
                    }
                }
                MENU_SETTINGS => send(IndicatorEvent::OpenSettings),
                MENU_HIDE => {
                    // SAFETY: hiding the window that received this command.
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    send(IndicatorEvent::Hidden);
                }
                _ => {}
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            // SAFETY: hiding the window that received this message.
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: the default handler for messages this window does not implement.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// The floating layout indicator.
pub struct WinIndicator {
    window: HWND,
    events: Receiver<IndicatorEvent>,
}

impl std::fmt::Debug for WinIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinIndicator").finish_non_exhaustive()
    }
}

impl WinIndicator {
    /// Creates the window. Call it on the thread that pumps messages.
    pub fn new(state: &IndicatorState, labels: IndicatorLabels) -> Result<Self> {
        // SAFETY: the class is registered once per process; a second
        // registration fails harmlessly and CreateWindowExW reports it.
        let window = unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&class);
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                CLASS,
                w!("Own Keyboard Switch"),
                WS_POPUP,
                0,
                0,
                SIDE,
                SIDE,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| os_error("create floating indicator", &e))?
        };
        let (events_tx, events) = unbounded();
        INDICATOR.with_borrow_mut(|slot| {
            *slot = Some(IndicatorWindow {
                window,
                state: state.clone(),
                labels,
                picture: Picture::default(),
                events: events_tx,
            });
        });
        Ok(Self { window, events })
    }

    /// Shows or hides the window according to the current settings.
    fn apply_visibility(&mut self, reason: Reason) {
        let show = INDICATOR.with_borrow(|state| {
            let state = state.as_ref()?;
            Some((
                state.state.visible,
                state.state.autohide,
                state.state.autohide_ms,
            ))
        });
        let Some((visible, autohide, autohide_ms)) = show else {
            return;
        };
        // SAFETY: a live window handle owned by this thread.
        unsafe {
            if !visible {
                let _ = KillTimer(Some(self.window), AUTOHIDE_TIMER);
                let _ = ShowWindow(self.window, SW_HIDE);
                return;
            }
            if !autohide {
                let _ = KillTimer(Some(self.window), AUTOHIDE_TIMER);
                let _ = ShowWindow(self.window, SW_SHOWNOACTIVATE);
                return;
            }
            // With auto-hide the indicator only appears for a moment after the
            // layout changed, and stays hidden the rest of the time. A redraw
            // in between must not cut a running countdown short.
            match reason {
                Reason::LayoutChanged => {
                    let _ = ShowWindow(self.window, SW_SHOWNOACTIVATE);
                    SetTimer(
                        Some(self.window),
                        AUTOHIDE_TIMER,
                        autohide_ms.max(200),
                        None,
                    );
                }
                Reason::Configured => {
                    let _ = KillTimer(Some(self.window), AUTOHIDE_TIMER);
                    let _ = ShowWindow(self.window, SW_HIDE);
                }
                Reason::Redraw => {}
            }
        }
    }
}

impl Drop for WinIndicator {
    fn drop(&mut self) {
        INDICATOR.with_borrow_mut(|slot| *slot = None);
        // SAFETY: the window was created by this thread and is destroyed once.
        unsafe {
            let _ = KillTimer(Some(self.window), AUTOHIDE_TIMER);
            let _ = DestroyWindow(self.window);
        }
    }
}

impl FloatingIndicator for WinIndicator {
    fn configure(&mut self, state: &IndicatorState, labels: IndicatorLabels) -> Result<()> {
        INDICATOR.with_borrow_mut(|slot| {
            if let Some(slot) = slot {
                slot.state = state.clone();
                slot.labels = labels;
            }
        });
        self.set_icon_inner(false)?;
        self.apply_visibility(Reason::Configured);
        Ok(())
    }

    fn set_icon(&mut self, rgba: &[u8], size: u32, layout_changed: bool) -> Result<()> {
        INDICATOR.with_borrow_mut(|slot| {
            if let Some(slot) = slot {
                slot.picture = Picture {
                    rgba: rgba.to_vec(),
                    source: size,
                };
            }
        });
        // Show first, so the picture lands on a window of the final size.
        self.apply_visibility(if layout_changed {
            Reason::LayoutChanged
        } else {
            Reason::Redraw
        });
        self.set_icon_inner(true)
    }

    fn poll(&mut self) -> Option<IndicatorEvent> {
        self.events.try_recv().ok()
    }
}

impl WinIndicator {
    fn set_icon_inner(&mut self, require_picture: bool) -> Result<()> {
        let side = side_for(self.window);
        // Take everything the drawing needs out of the cell first: Win32
        // calls must never run while a borrow of INDICATOR is alive.
        let work = INDICATOR.with_borrow_mut(|slot| {
            let slot = slot.as_mut()?;
            if slot.picture.rgba.is_empty() && require_picture {
                return None;
            }
            let position = slot
                .state
                .position
                .map_or_else(|| default_position(side), |p| clamp(p, side));
            slot.state.position = Some(position);
            Some((position, slot.picture.clone()))
        });
        let Some((position, picture)) = work else {
            return Ok(());
        };
        if picture.rgba.is_empty() {
            return Ok(());
        }
        repaint(self.window, &picture, position, side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_preserves_corners_and_premultiplies_alpha() {
        // A 2×2 picture: opaque red, transparent, half-transparent white, opaque black.
        let rgba = [
            0xFF, 0x00, 0x00, 0xFF, // top left
            0x00, 0xFF, 0x00, 0x00, // top right, fully transparent
            0xFF, 0xFF, 0xFF, 0x80, // bottom left
            0x00, 0x00, 0x00, 0xFF, // bottom right
        ];
        let out = premultiplied_bgra(&rgba, 2, 2);
        assert_eq!(out.len(), 16);
        // The first DIB row is the bottom one of the picture.
        assert_eq!(&out[0..4], &[0x80, 0x80, 0x80, 0x80]);
        assert_eq!(&out[4..8], &[0x00, 0x00, 0x00, 0xFF]);
        assert_eq!(&out[8..12], &[0x00, 0x00, 0xFF, 0xFF]);
        // A transparent source pixel stays fully transparent black.
        assert_eq!(&out[12..16], &[0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn scaling_up_keeps_the_picture_and_rejects_short_buffers() {
        let rgba = [0xFF, 0x00, 0x00, 0xFF];
        let out = premultiplied_bgra(&rgba, 1, 4);
        assert_eq!(out.len(), 64);
        assert!(
            out.as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0x00, 0x00, 0xFF, 0xFF])
        );
        assert!(
            premultiplied_bgra(&[0, 0, 0, 0], 4, 4)
                .iter()
                .all(|&b| b == 0)
        );
        assert!(premultiplied_bgra(&[], 0, 4).iter().all(|&b| b == 0));
    }
}
