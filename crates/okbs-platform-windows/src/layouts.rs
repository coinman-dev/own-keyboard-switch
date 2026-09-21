//! Keyboard layouts: enumeration, current layout, switching and key maps.
#![allow(unsafe_code)]

use crossbeam_channel::Sender;
use okbs_core::{KeyMap, Lang, PhysKey};
use okbs_platform::{LayoutId, LayoutInfo, LayoutManager, PlatformError, Result, StopGuard};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME,
};
use windows::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, HKL, MAPVK_VSC_TO_VK_EX, MapVirtualKeyExW,
    ToUnicodeEx, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    PostMessageW, WM_INPUTLANGCHANGEREQUEST,
};

const LOCALE_NAME_MAX_LENGTH: usize = 85;

pub(crate) fn window_class(window: HWND) -> String {
    let mut class = [0_u16; 128];
    // SAFETY: writable fixed-size UTF-16 buffer and an opaque window handle.
    let length = unsafe { GetClassNameW(window, &mut class) };
    String::from_utf16_lossy(&class[..usize::try_from(length).unwrap_or(0).min(class.len())])
}

/// A console HWND reports the client thread (e.g. PowerShell), whose HKL can be
/// stale. Its default IME window belongs to the actual conhost input thread.
/// Querying that thread requires neither attaching to the console nor reading
/// console text. Other application windows keep their normal thread mapping.
pub(crate) fn input_thread(window: HWND) -> Option<u32> {
    let owner = if window_class(window) == "ConsoleWindowClass" {
        // SAFETY: this only queries the input-method window associated with HWND.
        unsafe { ImmGetDefaultIMEWnd(window) }
    } else {
        window
    };
    if owner.is_invalid() {
        return None;
    }
    // SAFETY: querying an opaque HWND does not dereference foreign memory.
    let thread = unsafe { GetWindowThreadProcessId(owner, None) };
    (thread != 0).then_some(thread)
}

/// Converts an `HKL` into the platform-neutral id.
pub fn layout_id(hkl: HKL) -> LayoutId {
    LayoutId(hkl.0 as usize as u64)
}

/// Converts the platform-neutral id back into an `HKL`.
pub fn hkl(id: LayoutId) -> HKL {
    HKL(id.0 as usize as *mut core::ffi::c_void)
}

/// Language identifier (`LANGID`) stored in the low word of an `HKL`.
pub fn langid(id: LayoutId) -> u16 {
    (id.0 & 0xFFFF) as u16
}

/// Locale name such as `ru-RU` for a `LANGID`.
pub fn locale_name(langid: u16) -> Option<String> {
    let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: the buffer is valid for LOCALE_NAME_MAX_LENGTH UTF-16 units.
    let len = unsafe { LCIDToLocaleName(u32::from(langid), Some(&mut buf), 0) };
    let len = usize::try_from(len).ok().filter(|&l| l > 1)?;
    Some(String::from_utf16_lossy(&buf[..len - 1]))
}

/// Describes one layout.
pub fn describe(id: LayoutId) -> LayoutInfo {
    let lang_id = langid(id);
    let locale = locale_name(lang_id).unwrap_or_else(|| format!("0x{lang_id:04X}"));
    let mut short: String = locale.chars().take(2).collect();
    if let Some(first) = short.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    LayoutInfo {
        id,
        lang: Lang::from_windows_langid(lang_id),
        name: display_name(&locale).unwrap_or_else(|| locale.clone()),
        locale,
        short,
    }
}

/// Localized display name of a locale, e.g. «Русский (Россия)».
pub fn display_name(locale: &str) -> Option<String> {
    let name = windows::core::HSTRING::from(locale);
    let mut buf = [0u16; 128];
    // SAFETY: `name` is NUL-terminated and the buffer is valid for its length.
    let len = unsafe { GetLocaleInfoEx(&name, LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut buf)) };
    let len = usize::try_from(len).ok().filter(|&l| l > 1)?;
    Some(String::from_utf16_lossy(&buf[..len - 1]))
}

/// Layouts in the order of the Windows language bar.
pub fn installed_layouts() -> Vec<LayoutInfo> {
    // SAFETY: calling with no buffer only returns the number of layouts.
    let count = unsafe { GetKeyboardLayoutList(None) };
    let Ok(count) = usize::try_from(count) else {
        return Vec::new();
    };
    let mut list = vec![HKL::default(); count];
    // SAFETY: the buffer holds `count` HKL values.
    let filled = unsafe { GetKeyboardLayoutList(Some(&mut list)) };
    list.truncate(usize::try_from(filled).unwrap_or(0));
    list.into_iter().map(|h| describe(layout_id(h))).collect()
}

fn foreground_thread() -> Option<(HWND, u32)> {
    // SAFETY: both calls accept any window handle, including a null one.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        input_thread(hwnd).map(|thread| (hwnd, thread))
    }
}

/// Layout of the foreground input thread, including hosted console windows.
pub fn foreground_layout() -> Option<LayoutId> {
    let (_, thread) = foreground_thread()?;
    // SAFETY: GetKeyboardLayout accepts any thread id.
    Some(layout_id(unsafe { GetKeyboardLayout(thread) }))
}

/// Builds the key map of a layout with `ToUnicodeEx`.
pub fn keymap_for(id: LayoutId) -> KeyMap {
    let layout = hkl(id);
    let mut map = KeyMap::new();
    let mut state = [0u8; 256];
    for &key in PhysKey::ALL {
        if !(key.is_text_key() || key == PhysKey::Space) {
            continue;
        }
        let (scan, extended) = key.win_scancode();
        let code = u32::from(scan) | if extended { 0xE000 } else { 0 };
        // SAFETY: plain value conversion for a valid HKL.
        let vk = unsafe { MapVirtualKeyExW(code, MAPVK_VSC_TO_VK_EX, Some(layout)) };
        if vk == 0 {
            continue;
        }
        let mut chars = [None, None];
        for (i, shift) in [false, true].into_iter().enumerate() {
            state[usize::from(VK_SHIFT.0)] = if shift { 0x80 } else { 0 };
            let mut buf = [0u16; 8];
            // SAFETY: valid key state and buffer; flag 0x4 keeps the kernel dead-key state intact.
            let n =
                unsafe { ToUnicodeEx(vk, u32::from(scan), &state, &mut buf, 0x4, Some(layout)) };
            if n == 1 {
                chars[i] = char::from_u32(u32::from(buf[0])).filter(|c| !c.is_control());
            }
        }
        map.set(key, chars[0], chars[1]);
    }
    map
}

/// Layout manager for the foreground window.
#[derive(Debug, Default)]
pub struct WinLayouts;

impl LayoutManager for WinLayouts {
    fn layouts(&self) -> Result<Vec<LayoutInfo>> {
        Ok(installed_layouts())
    }

    fn current(&self) -> Result<LayoutId> {
        foreground_layout().ok_or(PlatformError::Other("no foreground window".into()))
    }

    fn set(&mut self, layout: LayoutId) -> Result<()> {
        let (hwnd, thread) =
            foreground_thread().ok_or(PlatformError::Other("no foreground window".into()))?;
        // SAFETY: thread was obtained from the current foreground input window.
        if layout_id(unsafe { GetKeyboardLayout(thread) }) == layout {
            return Ok(());
        }
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is a properly sized GUITHREADINFO.
        let target = if unsafe { GetGUIThreadInfo(thread, &mut info) }.is_ok()
            && !info.hwndFocus.is_invalid()
        {
            info.hwndFocus
        } else {
            hwnd
        };
        // SAFETY: posting a message with plain values.
        unsafe {
            PostMessageW(
                Some(target),
                WM_INPUTLANGCHANGEREQUEST,
                WPARAM(0),
                LPARAM(hkl(layout).0 as isize),
            )
        }
        .map_err(|e| crate::hook::os_error("PostMessageW(WM_INPUTLANGCHANGEREQUEST)", &e))?;
        // PostMessage only enqueues a request. Do not erase input until the
        // foreground application has actually accepted the new layout.
        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        loop {
            // SAFETY: handle/value queries only.
            if unsafe { GetForegroundWindow() } != hwnd {
                return Err(PlatformError::Other(
                    "focus changed before layout switch".into(),
                ));
            }
            if layout_id(unsafe { GetKeyboardLayout(thread) }) == layout {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(PlatformError::Other(
                    "foreground application did not accept the requested layout".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn keymap(&self, layout: LayoutId) -> Result<KeyMap> {
        Ok(keymap_for(layout))
    }

    fn subscribe(&mut self, sink: Sender<LayoutId>) -> Result<Box<dyn StopGuard>> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("okbs-layout-watch".into())
            .spawn(move || {
                let mut last = None;
                while !flag.load(Ordering::Relaxed) {
                    let now = foreground_layout();
                    if now.is_some() && now != last {
                        last = now;
                        if let Some(id) = now
                            && sink.send(id).is_err()
                        {
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(150));
                }
            })?;
        Ok(Box::new(Poller {
            stop,
            thread: Some(thread),
        }))
    }
}

struct Poller {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StopGuard for Poller {
    fn stop(self: Box<Self>) {}
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
