//! Native autoreplace feedback and insertion list. A persistent list and hints
//! use WS_EX_NOACTIVATE, so clicks do not steal the editor's input focus.
#![allow(unsafe_code)]

use crate::hook::os_error;
use crossbeam_channel::{Receiver, Sender, unbounded};
use okbs_core::PhysKey;
use okbs_core::config::AutoReplace;
use okbs_platform::autoreplace_gate::AutoReplaceGate;
use okbs_platform::{
    AutoreplaceInsertion, AutoreplaceLabels, AutoreplaceUi, InputTarget, PlatformError, Result,
};
use std::cell::RefCell;
use std::sync::Arc;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, ClientToScreen, CreateFontW,
    DEFAULT_CHARSET, DeleteObject, GetMonitorInfoW, GetSysColorBrush, HFONT, HGDIOBJ,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::SS_NOPREFIX;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, w};

const INSERT: usize = 101;
const CLOSE: usize = 102;
const LIST: usize = 103;
const CLASS: PCWSTR = w!("OwnKeyboardSwitch.Autoreplace");

struct PanelState {
    gate: Arc<AutoReplaceGate>,
    window: HWND,
    list: HWND,
    hint: HWND,
    settings: AutoReplace,
    target: Option<InputTarget>,
    persistent: bool,
    events: Sender<AutoreplaceInsertion>,
}

thread_local! { static PANEL: RefCell<Option<PanelState>> = const { RefCell::new(None) }; }

fn select_entry() {
    let action = PANEL.with_borrow(|state| {
        let state = state.as_ref()?;
        if !state.settings.enabled {
            return None;
        }
        // SAFETY: list is a live child HWND owned by this UI thread.
        let index =
            usize::try_from(unsafe { SendMessageW(state.list, LB_GETCURSEL, None, None) }.0)
                .ok()?;
        let item = state.settings.items.get(index)?.clone();
        let target = if state.persistent {
            crate::focus::input_target().or(state.target)
        } else {
            state.target
        };
        Some((
            state.events.clone(),
            AutoreplaceInsertion { item, target },
            state.window,
            state.persistent,
        ))
    });
    if let Some((events, insertion, window, persistent)) = action {
        if !persistent {
            // SAFETY: hiding a live window owned by this thread.
            unsafe {
                let _ = ShowWindow(window, SW_HIDE);
            }
        }
        let _ = events.send(insertion);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => {
            let no_activate =
                PANEL.with_borrow(|s| s.as_ref().is_some_and(|s| hwnd == s.hint || s.persistent));
            if no_activate {
                return LRESULT(MA_NOACTIVATE as isize);
            }
        }
        WM_CLOSE => {
            // SAFETY: hwnd is the window receiving this callback.
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            return LRESULT(0);
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let notification = (wparam.0 >> 16) & 0xffff;
            if (id == INSERT && notification == BN_CLICKED as usize)
                || (id == LIST && notification == LBN_DBLCLK as usize)
            {
                select_entry();
            }
            if id == CLOSE && notification == BN_CLICKED as usize {
                // SAFETY: hwnd is a live parent window.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
            return LRESULT(0);
        }
        WM_VKEYTOITEM => {
            let key = wparam.0 as u16;
            if key == VK_RETURN.0 {
                PANEL.with_borrow(|s| {
                    if let Some(s) = s {
                        s.gate.suppress_until_release(PhysKey::Enter);
                    }
                });
                select_entry();
                return LRESULT(-2);
            }
            if key == VK_ESCAPE.0 {
                PANEL.with_borrow(|s| {
                    if let Some(s) = s {
                        s.gate.suppress_until_release(PhysKey::Escape);
                    }
                });
                // SAFETY: hwnd is a live parent window.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                return LRESULT(-2);
            }
            return LRESULT(-1);
        }
        WM_ACTIVATE if wparam.0 & 0xffff == WA_INACTIVE as usize => {
            let temporary = PANEL.with_borrow(|s| {
                s.as_ref()
                    .is_some_and(|s| hwnd == s.window && !s.persistent)
            });
            if temporary {
                // SAFETY: dismiss only our temporary picker when its user leaves it.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
        _ => {}
    }
    // SAFETY: unchanged parameters are forwarded to the default procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn position(window: HWND, width: i32, height: i32) -> Result<()> {
    let mut point = POINT::default();
    // SAFETY: all output structures and HWNDs are valid for these calls.
    unsafe {
        let _ = GetCursorPos(&mut point);
        let active = GetForegroundWindow();
        let thread = GetWindowThreadProcessId(active, None);
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndCaret.is_invalid() {
            let mut caret = POINT {
                x: info.rcCaret.left,
                y: info.rcCaret.bottom,
            };
            if ClientToScreen(info.hwndCaret, &mut caret).as_bool() {
                point = caret;
            }
        }
        point.x += 12;
        point.y += 12;
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            point.x = point.x.clamp(
                info.rcWork.left,
                (info.rcWork.right - width).max(info.rcWork.left),
            );
            point.y = point.y.clamp(
                info.rcWork.top,
                (info.rcWork.bottom - height).max(info.rcWork.top),
            );
        }
        SetWindowPos(
            window,
            Some(HWND_TOPMOST),
            point.x,
            point.y,
            width,
            height,
            SWP_NOACTIVATE,
        )
        .map_err(|e| os_error("position autoreplace window", &e))
    }
}

/// Owned by the application's main message thread. No secondary process.
pub struct WinAutoreplaceUi {
    window: HWND,
    list: HWND,
    insert: HWND,
    close: HWND,
    help: HWND,
    hint_window: HWND,
    hint_label: HWND,
    font: HFONT,
    settings: AutoReplace,
    labels: AutoreplaceLabels,
    events: Receiver<AutoreplaceInsertion>,
}

impl std::fmt::Debug for WinAutoreplaceUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinAutoreplaceUi").finish_non_exhaustive()
    }
}

impl WinAutoreplaceUi {
    pub fn new(
        settings: &AutoReplace,
        labels: AutoreplaceLabels,
        gate: Arc<AutoReplaceGate>,
    ) -> Result<Self> {
        // SAFETY: class callbacks, constant class names and all buffers remain valid.
        unsafe {
            let instance = HINSTANCE(
                GetModuleHandleW(None)
                    .map_err(|e| os_error("module handle", &e))?
                    .0,
            );
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                ..Default::default()
            };
            // An existing registration can be reused after the previous controller closed.
            let _ = RegisterClassW(&class);
            let mut ui = Self {
                window: HWND::default(),
                list: HWND::default(),
                insert: HWND::default(),
                close: HWND::default(),
                help: HWND::default(),
                hint_window: HWND::default(),
                hint_label: HWND::default(),
                font: HFONT::default(),
                settings: settings.clone(),
                labels,
                events: unbounded().1,
            };
            ui.window = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_NOACTIVATE,
                CLASS,
                w!(""),
                WS_POPUP | WS_CAPTION | WS_SYSMENU,
                0,
                0,
                480,
                400,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|e| os_error("create autoreplace list", &e))?;
            ui.hint_window = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                CLASS,
                w!(""),
                WS_POPUP | WS_BORDER,
                0,
                0,
                440,
                90,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|e| os_error("create autoreplace hint", &e))?;
            let child =
                |class: PCWSTR, parent: HWND, id: usize, style: WINDOW_STYLE| -> Result<HWND> {
                    CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        class,
                        w!(""),
                        WS_CHILD | WS_VISIBLE | style,
                        0,
                        0,
                        10,
                        10,
                        Some(parent),
                        Some(HMENU(id as *mut core::ffi::c_void)),
                        Some(instance),
                        None,
                    )
                    .map_err(|e| os_error("create autoreplace control", &e))
                };
            ui.list = child(
                w!("LISTBOX"),
                ui.window,
                LIST,
                WS_VSCROLL
                    | WS_BORDER
                    | WINDOW_STYLE(
                        (LBS_NOTIFY | LBS_WANTKEYBOARDINPUT | LBS_NOINTEGRALHEIGHT) as u32,
                    ),
            )?;
            ui.insert = child(
                w!("BUTTON"),
                ui.window,
                INSERT,
                WINDOW_STYLE(BS_PUSHBUTTON as u32),
            )?;
            ui.close = child(
                w!("BUTTON"),
                ui.window,
                CLOSE,
                WINDOW_STYLE(BS_PUSHBUTTON as u32),
            )?;
            ui.help = child(w!("STATIC"), ui.window, 104, WINDOW_STYLE(SS_NOPREFIX.0))?;
            ui.hint_label = child(
                w!("STATIC"),
                ui.hint_window,
                105,
                WINDOW_STYLE(SS_NOPREFIX.0),
            )?;
            let dpi = GetDpiForWindow(ui.window).max(96);
            ui.font = CreateFontW(
                -(14 * dpi as i32 / 96),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            );
            for child in [ui.list, ui.insert, ui.close, ui.help, ui.hint_label] {
                SendMessageW(
                    child,
                    WM_SETFONT,
                    Some(WPARAM(ui.font.0 as usize)),
                    Some(LPARAM(1)),
                );
            }
            let (events, rx) = unbounded();
            ui.events = rx;
            PANEL.with_borrow_mut(|state| {
                *state = Some(PanelState {
                    gate,
                    window: ui.window,
                    list: ui.list,
                    hint: ui.hint_window,
                    settings: settings.clone(),
                    target: None,
                    persistent: false,
                    events,
                })
            });
            ui.populate()?;
            Ok(ui)
        }
    }

    fn populate(&self) -> Result<()> {
        // SAFETY: handles are owned by self on the creating UI thread.
        unsafe {
            SetWindowTextW(self.window, &HSTRING::from(&self.labels.title))
                .map_err(|e| os_error("list title", &e))?;
            SetWindowTextW(self.insert, &HSTRING::from(&self.labels.insert))
                .map_err(|e| os_error("insert label", &e))?;
            SetWindowTextW(self.close, &HSTRING::from(&self.labels.close))
                .map_err(|e| os_error("close label", &e))?;
            SendMessageW(self.list, LB_RESETCONTENT, None, None);
            let lines: Vec<String> = if self.settings.items.is_empty() {
                vec![self.labels.empty.clone()]
            } else {
                self.settings
                    .items
                    .iter()
                    .map(|i| {
                        format!(
                            "{}  —  {}",
                            i.from,
                            i.to.chars()
                                .take(65)
                                .map(|c| if c.is_control() { ' ' } else { c })
                                .collect::<String>()
                        )
                    })
                    .collect()
            };
            for line in lines {
                let line = HSTRING::from(line);
                SendMessageW(
                    self.list,
                    LB_ADDSTRING,
                    None,
                    Some(LPARAM(line.as_ptr() as isize)),
                );
            }
            SendMessageW(self.list, LB_SETCURSEL, Some(WPARAM(0)), None);
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                self.insert,
                self.settings.enabled && !self.settings.items.is_empty(),
            );
            let persistent = PANEL.with_borrow(|s| s.as_ref().is_some_and(|s| s.persistent));
            let help = if !self.settings.enabled {
                &self.labels.disabled
            } else if persistent {
                &self.labels.list_help
            } else {
                &self.labels.menu_help
            };
            SetWindowTextW(self.help, &HSTRING::from(help))
                .map_err(|e| os_error("list help", &e))?;
            SetLayeredWindowAttributes(
                self.window,
                COLORREF(0),
                (self.settings.list_opacity.clamp(0.1, 1.0) * 255.0).round() as u8,
                LWA_ALPHA,
            )
            .map_err(|e| os_error("list opacity", &e))?;
        }
        Ok(())
    }

    fn layout(&self) -> Result<()> {
        // SAFETY: output rectangle and controls are owned by this thread.
        unsafe {
            let mut client = RECT::default();
            GetClientRect(self.window, &mut client)
                .map_err(|e| os_error("list client area", &e))?;
            let scale = GetDpiForWindow(self.window).max(96) as i32;
            let px = |v| v * scale / 96;
            let w = client.right;
            let h = client.bottom;
            for (child, x, y, width, height) in [
                (self.list, px(10), px(10), w - px(20), h - px(96)),
                (self.insert, px(10), h - px(76), px(150), px(30)),
                (self.close, w - px(110), h - px(76), px(100), px(30)),
                (self.help, px(10), h - px(38), w - px(20), px(32)),
            ] {
                MoveWindow(child, x, y, width, height, true)
                    .map_err(|e| os_error("layout list controls", &e))?;
            }
        }
        Ok(())
    }
}

impl AutoreplaceUi for WinAutoreplaceUi {
    fn configure(
        &mut self,
        settings: &AutoReplace,
        labels: AutoreplaceLabels,
        _theme: okbs_core::config::Theme,
    ) -> Result<()> {
        self.settings = settings.clone();
        self.labels = labels;
        PANEL.with_borrow_mut(|s| {
            if let Some(s) = s {
                s.settings = settings.clone();
            }
        });
        self.hint(None)?;
        self.populate()
    }

    fn hint(&mut self, index: Option<usize>) -> Result<()> {
        // SAFETY: controls are live and used only on their creating thread.
        unsafe {
            let Some(item) = index
                .and_then(|i| self.settings.items.get(i))
                .filter(|_| self.settings.enabled)
            else {
                let _ = ShowWindow(self.hint_window, SW_HIDE);
                return Ok(());
            };
            let text = format!(
                "{} → {}\r\n{}",
                item.from,
                item.to
                    .chars()
                    .take(70)
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect::<String>(),
                self.labels.hint_help
            );
            SetWindowTextW(self.hint_label, &HSTRING::from(text))
                .map_err(|e| os_error("hint text", &e))?;
            let scale = GetDpiForWindow(self.hint_window).max(96) as i32;
            let (w, h) = (440 * scale / 96, 92 * scale / 96);
            position(self.hint_window, w, h)?;
            MoveWindow(self.hint_label, 10, 8, w - 22, h - 18, true)
                .map_err(|e| os_error("hint bounds", &e))?;
            let _ = ShowWindow(self.hint_window, SW_SHOWNOACTIVATE);
        }
        Ok(())
    }

    fn show_list(&mut self, toggle: bool, target: Option<InputTarget>) -> Result<()> {
        self.hint(None)?;
        // SAFETY: style changes and visibility operations target our own HWND.
        unsafe {
            if toggle && IsWindowVisible(self.window).as_bool() {
                let _ = ShowWindow(self.window, SW_HIDE);
                return Ok(());
            }
            PANEL.with_borrow_mut(|s| {
                if let Some(s) = s {
                    s.target = target;
                    s.persistent = toggle;
                }
            });
            let ex = WS_EX_TOOLWINDOW
                | WS_EX_TOPMOST
                | WS_EX_LAYERED
                | if toggle {
                    WS_EX_NOACTIVATE
                } else {
                    WINDOW_EX_STYLE(0)
                };
            SetWindowLongPtrW(self.window, GWL_EXSTYLE, ex.0 as isize);
            self.populate()?;
            let scale = GetDpiForWindow(self.window).max(96) as i32;
            position(self.window, 480 * scale / 96, 400 * scale / 96)?;
            self.layout()?;
            let _ = ShowWindow(
                self.window,
                if toggle { SW_SHOWNOACTIVATE } else { SW_SHOW },
            );
            if !toggle {
                let _ = SetForegroundWindow(self.window);
                let _ = SetFocus(Some(self.list));
                if GetForegroundWindow() != self.window || GetFocus() != self.list {
                    let _ = ShowWindow(self.window, SW_HIDE);
                    return Err(PlatformError::Other(
                        "cannot focus the autoreplace picker".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn poll(&mut self) -> Option<AutoreplaceInsertion> {
        self.events.try_recv().ok()
    }
}

impl Drop for WinAutoreplaceUi {
    fn drop(&mut self) {
        PANEL.with_borrow_mut(|s| *s = None);
        // SAFETY: child windows die with their parent; the font is released last.
        unsafe {
            if !self.window.is_invalid() {
                let _ = DestroyWindow(self.window);
            }
            if !self.hint_window.is_invalid() {
                let _ = DestroyWindow(self.hint_window);
            }
            if !self.font.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.font.0));
            }
        }
    }
}
