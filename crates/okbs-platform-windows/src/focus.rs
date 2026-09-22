//! Foreground window information and focus change notifications.
#![allow(unsafe_code)]

use crossbeam_channel::Sender;
use okbs_platform::{
    FocusEvent, FocusInfo, InputTarget, PlatformError, Result, StopGuard, WindowInfo,
};
use std::cell::RefCell;
use std::path::PathBuf;
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, HWINEVENTHOOK, IUIAutomation, IUIAutomation2, SetWinEventHook, UnhookWinEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MENUEND, GUITHREADINFO, GWL_STYLE, GetClassNameW,
    GetForegroundWindow, GetGUIThreadInfo, GetMenu, GetMenuItemCount, GetMenuStringW,
    GetPhysicalCursorPos, GetWindowLongPtrW, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsWindow, MF_BYPOSITION, SetForegroundWindow, WINEVENT_OUTOFCONTEXT,
};
use windows::core::Interface;
use windows::core::PWSTR;

const ES_PASSWORD: isize = 0x0020;

/// The physical pointer position. The UI placement
/// guard uses physical pixels too; never multiply these coordinates by DPI.
pub fn cursor_position() -> [f32; 2] {
    let mut point = windows::Win32::Foundation::POINT::default();
    // SAFETY: `point` is a valid writable structure.
    if unsafe { GetPhysicalCursorPos(&mut point) }.is_ok() {
        [point.x as f32, point.y as f32]
    } else {
        [24.0, 24.0]
    }
}

/// A placement point on the monitor containing the foreground editor.
/// The mouse may be on another monitor while text is being typed.
pub fn foreground_window_position() -> Option<[f32; 2]> {
    // SAFETY: query-only call with a valid output buffer.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect).ok()?;
        Some([
            rect.left.saturating_add(24) as f32,
            rect.top.saturating_add(72) as f32,
        ])
    }
}

/// Placement point for an input target captured before a background task.
pub fn input_target_position(target: InputTarget) -> Option<[f32; 2]> {
    // SAFETY: the opaque window handle is only queried and `rect` is valid.
    unsafe {
        let window = HWND(target.window as usize as *mut core::ffi::c_void);
        if !IsWindow(Some(window)).as_bool() {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect).ok()?;
        Some([
            rect.left.saturating_add(24) as f32,
            rect.top.saturating_add(72) as f32,
        ])
    }
}

fn wide_to_string(buf: &[u16], len: i32) -> Option<String> {
    let len = usize::try_from(len).ok().filter(|&l| l > 0)?;
    Some(String::from_utf16_lossy(&buf[..len.min(buf.len())]))
}

fn process_path(pid: u32) -> Option<PathBuf> {
    // SAFETY: OpenProcess with limited query rights; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: buffer and length describe valid memory.
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: the handle was opened above.
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(
        &buf[..len as usize],
    )))
}

/// Information about a window.
pub fn window_info(hwnd: HWND) -> WindowInfo {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let mut title = [0u16; 512];
    let mut class = [0u16; 256];
    // SAFETY: buffers are valid for their lengths.
    let (title_len, class_len) = unsafe {
        (
            GetWindowTextW(hwnd, &mut title),
            GetClassNameW(hwnd, &mut class),
        )
    };
    WindowInfo {
        pid: (pid != 0).then_some(pid),
        executable: (pid != 0).then(|| process_path(pid)).flatten(),
        title: wide_to_string(&title, title_len),
        app_id: wide_to_string(&class, class_len),
    }
}

/// Focus information of the foreground window.
#[derive(Debug, Default)]
pub struct WinFocus;

/// Fast identity check for the hook. No UI Automation or process path queries.
pub fn input_target() -> Option<InputTarget> {
    // SAFETY: all buffers are sized correctly; no foreign memory is dereferenced.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let mut class = [0u16; 128];
        let len = GetClassNameW(window, &mut class);
        let class = wide_to_string(&class, len).unwrap_or_default();
        if matches!(
            class.as_str(),
            "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "NotifyIconOverflowWindow"
                | "Progman"
                | "WorkerW"
        ) {
            return None;
        }
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let thread = crate::layouts::input_thread(window)?;
        let has_focus = GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndFocus.is_invalid();
        if !has_focus && class != "ConsoleWindowClass" {
            return None;
        }
        Some(InputTarget {
            window: window.0 as usize as u64,
            control: if has_focus { info.hwndFocus } else { window }.0 as usize as u64,
        })
    }
}

/// Language of the letters marked with `&` in the menu bar of `window`.
/// Owner-drawn and modern menus report no text and give `None`.
fn menu_language(window: HWND) -> Option<okbs_core::Lang> {
    // SAFETY: an opaque window handle; every buffer is sized for its call.
    unsafe {
        let menu = GetMenu(window);
        if menu.is_invalid() {
            return None;
        }
        let count = GetMenuItemCount(Some(menu));
        let (mut cyrillic, mut latin) = (0_u32, 0_u32);
        for index in 0..count.max(0) {
            let mut text = [0_u16; 128];
            let length = GetMenuStringW(menu, index as u32, Some(&mut text), MF_BYPOSITION);
            let label = String::from_utf16_lossy(
                &text[..usize::try_from(length).unwrap_or(0).min(text.len())],
            );
            let mut chars = label.chars().peekable();
            while let Some(c) = chars.next() {
                if c != '&' {
                    continue;
                }
                match chars.peek() {
                    // «&&» is a literal ampersand, not an access key.
                    Some('&') => {
                        chars.next();
                    }
                    Some(letter) if letter.is_alphabetic() => {
                        if letter.is_ascii_alphabetic() {
                            latin += 1;
                        } else {
                            cyrillic += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        match (latin, cyrillic) {
            (0, 0) => None,
            (l, c) if l >= c => Some(okbs_core::Lang::En),
            _ => Some(okbs_core::Lang::Ru),
        }
    }
}

impl FocusInfo for WinFocus {
    fn menu_access_language(&self) -> Result<Option<okbs_core::Lang>> {
        // SAFETY: no arguments.
        let window = unsafe { GetForegroundWindow() };
        Ok((!window.is_invalid())
            .then(|| menu_language(window))
            .flatten())
    }

    fn is_terminal(&self) -> Result<bool> {
        // SAFETY: queries only; no console contents are read.
        let window = unsafe { GetForegroundWindow() };
        Ok(matches!(
            crate::layouts::window_class(window).as_str(),
            "ConsoleWindowClass" | "CASCADIA_HOSTING_WINDOW_CLASS"
        ))
    }

    fn input_target(&self) -> Result<Option<InputTarget>> {
        Ok(input_target())
    }

    fn activate_target(&self, target: InputTarget) -> Result<()> {
        let window = HWND(target.window as usize as *mut core::ffi::c_void);
        // SAFETY: opaque handles are checked before requesting activation.
        if !unsafe { IsWindow(Some(window)) }.as_bool() {
            return Err(PlatformError::Other("insertion window was closed".into()));
        }
        // SAFETY: activation is requested only by a user's insert command.
        let _ = unsafe { SetForegroundWindow(window) };
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
        loop {
            if input_target() == Some(target) {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Err(PlatformError::Other(
            "cannot restore the insertion control".into(),
        ))
    }
    fn active_window(&self) -> Result<Option<WindowInfo>> {
        // SAFETY: no arguments.
        let hwnd = unsafe { GetForegroundWindow() };
        Ok((!hwnd.is_invalid()).then(|| window_info(hwnd)))
    }

    fn is_password_field(&self) -> Result<Option<bool>> {
        // SAFETY: no arguments.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return Ok(None);
        }
        // SAFETY: valid window handle.
        let Some(thread) = crate::layouts::input_thread(hwnd) else {
            return Ok(None);
        };
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is a properly sized GUITHREADINFO.
        if unsafe { GetGUIThreadInfo(thread, &mut info) }.is_err() || info.hwndFocus.is_invalid() {
            return Ok(None);
        }
        let mut class = [0u16; 64];
        // SAFETY: valid buffer.
        let len = unsafe { GetClassNameW(info.hwndFocus, &mut class) };
        let class = wide_to_string(&class, len).unwrap_or_default();
        if class.eq_ignore_ascii_case("Edit") {
            // SAFETY: valid window handle.
            let style = unsafe { GetWindowLongPtrW(info.hwndFocus, GWL_STYLE) };
            return Ok(Some(style & ES_PASSWORD != 0));
        }
        Ok(uia_is_password())
    }

    fn subscribe(&mut self, sink: Sender<FocusEvent>) -> Result<Box<dyn StopGuard>> {
        crate::message_thread::spawn("okbs-focus", move || {
            SINK.with_borrow_mut(|s| *s = Some(sink));
            // SAFETY: out-of-context hook with a valid callback; removed in the cleanup closure.
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_MENUEND,
                    None,
                    Some(foreground_changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            Ok(move || {
                if !hook.is_invalid() {
                    // SAFETY: the hook was installed by this thread.
                    let _ = unsafe { UnhookWinEvent(hook) };
                }
                SINK.with_borrow_mut(|s| *s = None);
            })
        })
    }
}

thread_local! {
    static SINK: RefCell<Option<Sender<FocusEvent>>> = const { RefCell::new(None) };
    static AUTOMATION: RefCell<Option<Option<IUIAutomation>>> = const { RefCell::new(None) };
}

/// UI Automation client of the calling thread, created on first use.
fn automation() -> Option<IUIAutomation> {
    AUTOMATION.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| {
            // SAFETY: COM is initialized for this thread before creating the client;
            // an already initialized apartment returns a harmless error code.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let client: Option<IUIAutomation> =
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok();
                if let Some(client2) = client
                    .as_ref()
                    .and_then(|c| c.cast::<IUIAutomation2>().ok())
                {
                    // Do not stall typing when the focused application hangs.
                    let _ = client2.SetConnectionTimeout(300);
                    let _ = client2.SetTransactionTimeout(300);
                }
                client
            }
        })
        .clone()
    })
}

/// Whether the focused element is a password field (browsers, WPF, UWP).
fn uia_is_password() -> Option<bool> {
    let client = automation()?;
    // SAFETY: plain COM calls on a valid client; results are reference counted.
    unsafe {
        let element = client.GetFocusedElement().ok()?;
        element.CurrentIsPassword().ok().map(|b| b.as_bool())
    }
}

/// The hook covers the small range that holds both the foreground change and
/// the end of a menu bar loop; other events in between are ignored.
unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let message = if event == EVENT_SYSTEM_MENUEND {
        FocusEvent::MenuClosed
    } else if event == EVENT_SYSTEM_FOREGROUND {
        FocusEvent::WindowChanged((!hwnd.is_invalid()).then(|| window_info(hwnd)))
    } else {
        return;
    };
    SINK.with_borrow(|sink| {
        if let Some(sink) = sink {
            let _ = sink.send(message);
        }
    });
}
