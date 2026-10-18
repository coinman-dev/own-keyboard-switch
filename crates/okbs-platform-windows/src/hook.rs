//! Global keyboard and mouse capture with low-level hooks.
#![allow(unsafe_code)]

use crate::INJECT_TAG;
use crossbeam_channel::Sender;
use okbs_core::PhysKey;
use okbs_core::config::{Config, Mouse, MouseButton, SwitchKey};
use okbs_platform::autoreplace_gate::AutoReplaceGate;
use okbs_platform::{InputEvent, KeyboardSource, PlatformError, Result, StopGuard};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CANCEL, VK_CAPITAL, VK_PAUSE};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetWindowThreadProcessId, HC_ACTION, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, MSLLHOOKSTRUCT, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDOWN,
    WM_SYSKEYDOWN, WM_XBUTTONDOWN, WM_XBUTTONUP, WindowFromPoint, XBUTTON1, XBUTTON2,
};

struct HookState {
    sink: Sender<InputEvent>,
    down: HashSet<PhysKey>,
    filter: Arc<HookFilter>,
}

/// Keys the hook suppresses, so that applications do not receive them.
/// Updated from the configuration while the hook runs.
#[derive(Debug)]
pub struct HookFilter {
    swallow_capslock: AtomicBool,
    mouse: Mutex<Mouse>,
    pub autoreplace: Arc<AutoReplaceGate>,
}

impl Default for HookFilter {
    fn default() -> Self {
        Self {
            swallow_capslock: AtomicBool::new(false),
            mouse: Mutex::new(Mouse::default()),
            autoreplace: Arc::new(AutoReplaceGate::new(&Config::default())),
        }
    }
}

impl HookFilter {
    /// Applies «Отключить кнопку Caps Lock» and «Переключать по: Caps Lock».
    pub fn configure(&self, config: &Config) {
        let swallow =
            config.advanced.disable_capslock || config.switching.switch_key == SwitchKey::CapsLock;
        self.swallow_capslock.store(swallow, Ordering::Relaxed);
        if let Ok(mut mouse) = self.mouse.lock() {
            *mouse = config.mouse.clone();
        }
        self.autoreplace.configure(config);
    }
}

thread_local! {
    static STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

/// Maps a low-level hook event to a physical key.
pub fn phys_key(vk: u32, scan: u32, extended: bool) -> Option<PhysKey> {
    if vk == u32::from(VK_PAUSE.0) || vk == u32::from(VK_CANCEL.0) {
        return Some(PhysKey::Pause);
    }
    PhysKey::from_win_scancode(u16::try_from(scan).ok()?, extended)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lParam points to a KBDLLHOOKSTRUCT valid during the call.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let pressed = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let extended = info.flags.0 & LLKHF_EXTENDED.0 != 0;
        let injected = info.dwExtraInfo == INJECT_TAG;
        if injected {
            // Own input must not change physical key state (or turn tagged
            // Unicode input into an UnknownKey that clears the engine buffer).
            // SAFETY: forwarding the unchanged hook arguments.
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        let time = Instant::now();
        let mut swallow = false;
        STATE.with_borrow_mut(|state| {
            let Some(state) = state else { return };
            let mut event = match phys_key(info.vkCode, info.scanCode, extended) {
                Some(key) => {
                    let repeat = if pressed {
                        !state.down.insert(key)
                    } else {
                        state.down.remove(&key);
                        false
                    };
                    InputEvent::Key {
                        key,
                        pressed,
                        repeat,
                        injected,
                        time,
                    }
                }
                // Fake shifts around navigation keys (extended 0x2A) and unknown keys.
                None if info.scanCode == 0x2A && extended => return,
                None => InputEvent::UnknownKey {
                    // Preserve Unicode units and extended flags for queued input.
                    code: (info.vkCode & 0xff)
                        | (u32::from(extended) << 8)
                        | ((info.scanCode & 0xffff) << 16),
                    pressed,
                    time,
                },
            };
            if state.filter.autoreplace.discard(event) {
                swallow = true;
                return;
            }
            swallow = !injected
                && matches!(
                    event,
                    InputEvent::Key {
                        key: PhysKey::CapsLock,
                        ..
                    }
                )
                && state.filter.swallow_capslock.load(Ordering::Relaxed);
            let target = crate::focus::input_target();
            let lang = crate::layouts::foreground_layout()
                .and_then(|id| okbs_core::Lang::from_windows_langid(crate::layouts::langid(id)));
            if !swallow && state.filter.autoreplace.capture(event, lang, target) {
                let epoch = state.filter.autoreplace.epoch();
                event = match event {
                    InputEvent::Key {
                        key,
                        pressed,
                        repeat,
                        time,
                        ..
                    } => InputEvent::CapturedKey {
                        epoch,
                        key,
                        pressed,
                        repeat,
                        time,
                        target,
                    },
                    InputEvent::UnknownKey {
                        code,
                        pressed,
                        time,
                    } => InputEvent::CapturedUnknown {
                        epoch,
                        code,
                        pressed,
                        time,
                        target,
                    },
                    other => other,
                };
                swallow = true;
            }
            if state.sink.send(event).is_err() {
                state.filter.autoreplace.fail_open();
                swallow = false;
            }
        });
        if swallow {
            return LRESULT(1);
        }
    }
    // SAFETY: forwarding the unchanged hook arguments.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The «Управление мышью» button of a hook message, if it has one.
fn assignable_button(message: u32, mouse_data: u32) -> Option<MouseButton> {
    match message {
        WM_MBUTTONDOWN | WM_MBUTTONUP => Some(MouseButton::Middle),
        WM_XBUTTONDOWN | WM_XBUTTONUP => match (mouse_data >> 16) as u16 {
            XBUTTON1 => Some(MouseButton::Back),
            XBUTTON2 => Some(MouseButton::Forward),
            _ => None,
        },
        _ => None,
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let message = wparam.0 as u32;
        // SAFETY: HC_ACTION provides a live MSLLHOOKSTRUCT for this callback.
        let mouse_data = unsafe { (*(lparam.0 as *const MSLLHOOKSTRUCT)).mouseData };
        let button = assignable_button(message, mouse_data);
        let (assigned, blocked) = STATE.with_borrow(|state| {
            let Some(state) = state else {
                return (None, false);
            };
            let Some(button) = button else {
                return (None, false);
            };
            let Ok(mouse) = state.filter.mouse.lock() else {
                return (None, false);
            };
            (mouse.action(button).map(|_| button), mouse.blocks(button))
        });
        let down = matches!(message, WM_MBUTTONDOWN | WM_XBUTTONDOWN);
        if matches!(
            message,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        ) {
            // The low-level click arrives before Windows activates the clicked
            // window. Record both owners to diagnose first-click focus races.
            // SAFETY: HC_ACTION provides a live MSLLHOOKSTRUCT for this callback.
            let point = unsafe { (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt };
            let mut clicked_pid = 0;
            let mut foreground_pid = 0;
            // SAFETY: query-only calls and valid output buffers.
            let (clicked, foreground) = unsafe {
                let clicked = WindowFromPoint(point);
                let foreground = GetForegroundWindow();
                GetWindowThreadProcessId(clicked, Some(&mut clicked_pid));
                GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
                (clicked, foreground)
            };
            if clicked_pid == std::process::id() || foreground_pid == std::process::id() {
                tracing::debug!(target: "okbs_input", clicked_pid, foreground_pid,
                    clicked_window = clicked.0 as usize, foreground_window = foreground.0 as usize,
                    "mouse down before activation involving application UI");
            }
            STATE.with_borrow(|state| {
                if let Some(state) = state {
                    let event = InputEvent::MouseButton {
                        time: Instant::now(),
                        in_own_window: clicked_pid == std::process::id(),
                    };
                    state
                        .filter
                        .autoreplace
                        .capture(event, None, crate::focus::input_target());
                    let _ = state.sink.send(event);
                    if let Some(button) = assigned.filter(|_| down) {
                        let _ = state.sink.send(InputEvent::MouseShortcut {
                            button,
                            time: Instant::now(),
                        });
                    }
                }
            });
        }
        if blocked {
            // Applications receive neither the press nor the release.
            return LRESULT(1);
        }
    }
    // SAFETY: forwarding the unchanged hook arguments.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Keyboard source based on low-level hooks.
#[derive(Debug, Default)]
pub struct HookSource {
    /// Keys to suppress.
    pub filter: Arc<HookFilter>,
}

impl KeyboardSource for HookSource {
    fn start(&mut self, sink: Sender<InputEvent>) -> Result<Box<dyn StopGuard>> {
        let filter = self.filter.clone();
        crate::message_thread::spawn("okbs-hook", move || {
            STATE.with_borrow_mut(|state| {
                *state = Some(HookState {
                    sink,
                    down: HashSet::new(),
                    filter,
                })
            });
            // SAFETY: a null module name returns the handle of the current executable.
            let module =
                unsafe { GetModuleHandleW(None) }.map_err(|e| os_error("GetModuleHandleW", &e))?;
            let instance = HINSTANCE(module.0);
            // SAFETY: the hook procedures have the required signature and live for the program.
            let keyboard = unsafe {
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(instance), 0)
            }
            .map_err(|e| os_error("SetWindowsHookExW(WH_KEYBOARD_LL)", &e))?;
            // SAFETY: as above.
            let mouse = match unsafe {
                SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(instance), 0)
            } {
                Ok(hook) => Some(hook),
                Err(err) => {
                    tracing::warn!("mouse hook unavailable: {err}");
                    None
                }
            };
            tracing::info!("keyboard hook installed");
            Ok(move || {
                // SAFETY: the hooks were installed by this thread and are removed once.
                unsafe {
                    let _ = UnhookWindowsHookEx(keyboard);
                    if let Some(mouse) = mouse {
                        let _ = UnhookWindowsHookEx(mouse);
                    }
                }
                STATE.with_borrow_mut(|state| *state = None);
                tracing::info!("keyboard hook removed");
            })
        })
    }

    fn caps_lock_on(&self) -> Option<bool> {
        // SAFETY: GetKeyState has no pointer arguments.
        Some(unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1 != 0)
    }
}

pub(crate) fn os_error(context: &str, err: &windows::core::Error) -> PlatformError {
    PlatformError::Os {
        code: i64::from(err.code().0),
        context: context.to_string(),
    }
}
