//! Non-blocking input handoff for replacements and word boundaries. The hook
//! only tracks keys; detection, password and application checks stay on the engine.

use crate::{InputEvent, InputTarget};
use okbs_core::autoreplace::AutoReplacer;
use okbs_core::config::{AutoReplaceTrigger, Config, HotkeyAction, Hotkeys};
use okbs_core::layouts::{self, KeyPress};
use okbs_core::{Lang, ModState, PhysKey};
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct State {
    epoch: u64,
    replacer: AutoReplacer,
    trigger: AutoReplaceTrigger,
    hotkeys: Hotkeys,
    hotkeys_enabled: bool,
    autoswitch: bool,
    no_switch_on_tab_enter: bool,
    capturing: bool,
    mods: ModState,
    keys: Vec<KeyPress>,
    overflow: usize,
    max_len: usize,
    lang: Option<Lang>,
    target: Option<InputTarget>,
    last_external_target: Option<InputTarget>,
    last: Option<Instant>,
    active: bool,
    pending: usize,
    reserved: HashSet<PhysKey>,
    discard_until_release: HashSet<PhysKey>,
}

impl State {
    fn reset_word(&mut self) {
        self.keys.clear();
        self.overflow = 0;
        self.lang = None;
    }

    fn matches(&self) -> bool {
        let Some(lang) = self.lang else { return false };
        if self.overflow > 0 || self.keys.is_empty() {
            return false;
        }
        let typed = layouts::render(&self.keys, layouts::builtin_keymap(lang));
        let other = layouts::render(&self.keys, layouts::builtin_keymap(lang.other()));
        self.replacer.find(&typed, Some(&other)).is_some()
    }

    fn prepare(&mut self, target: Option<InputTarget>, lang: Option<Lang>, time: Instant) {
        if self.target != target
            || self.lang.is_some_and(|old| Some(old) != lang)
            || self
                .last
                .is_some_and(|last| time.saturating_duration_since(last) > Duration::from_secs(30))
        {
            self.reset_word();
        }
        self.target = target;
        self.last = Some(time);
    }

    fn feed(&mut self, event: InputEvent, lang: Option<Lang>) {
        match event {
            InputEvent::Key {
                key,
                pressed,
                repeat,
                ..
            } => {
                if key.is_modifier() {
                    self.mods.set(key, pressed);
                    return;
                }
                if !pressed {
                    return;
                }
                if self.mods.ctrl() || self.mods.alt() || self.mods.win() {
                    self.reset_word();
                    return;
                }
                if key == PhysKey::Backspace {
                    if self.overflow > 0 {
                        self.overflow -= 1;
                    } else {
                        self.keys.pop();
                    }
                    return;
                }
                let press = KeyPress {
                    key,
                    shift: self.mods.shift(),
                    caps: false,
                };
                if repeat || !layouts::is_word_key(press) {
                    self.reset_word();
                    return;
                }
                if self.keys.is_empty() {
                    self.lang = lang;
                }
                if self.keys.len() < self.max_len {
                    self.keys.push(press);
                } else {
                    self.overflow += 1;
                }
            }
            InputEvent::UnknownKey { pressed: true, .. } | InputEvent::MouseButton { .. } => {
                self.reset_word()
            }
            _ => {}
        }
    }
}

/// Shared by one physical input source and its engine. No locks are held while
/// querying the OS, injecting input or executing application operations.
pub struct AutoReplaceGate(Mutex<State>);

impl std::fmt::Debug for AutoReplaceGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoReplaceGate").finish_non_exhaustive()
    }
}

impl AutoReplaceGate {
    pub fn new(config: &Config) -> Self {
        Self(Mutex::new(State {
            epoch: 0,
            replacer: AutoReplacer::new(&config.autoreplace),
            trigger: config.autoreplace.trigger,
            hotkeys: config.hotkeys.clone(),
            hotkeys_enabled: config.general.autoswitch
                || !config.general.hotkeys_off_when_autoswitch_off,
            autoswitch: config.general.autoswitch,
            no_switch_on_tab_enter: config.troubleshooting.no_switch_on_tab_enter,
            capturing: false,
            mods: ModState::default(),
            keys: Vec::new(),
            overflow: 0,
            max_len: config
                .autoreplace
                .items
                .iter()
                .map(|item| item.from.trim().chars().count())
                .max()
                .unwrap_or(0)
                .max(128),
            lang: None,
            target: None,
            last_external_target: None,
            last: None,
            active: false,
            pending: 0,
            reserved: HashSet::new(),
            discard_until_release: HashSet::new(),
        }))
    }

    pub fn configure(&self, config: &Config) {
        let replacer = AutoReplacer::new(&config.autoreplace);
        if let Ok(mut s) = self.0.lock() {
            s.epoch = s.epoch.wrapping_add(1);
            s.replacer = replacer;
            s.trigger = config.autoreplace.trigger;
            s.hotkeys = config.hotkeys.clone();
            s.hotkeys_enabled =
                config.general.autoswitch || !config.general.hotkeys_off_when_autoswitch_off;
            s.autoswitch = config.general.autoswitch;
            s.no_switch_on_tab_enter = config.troubleshooting.no_switch_on_tab_enter;
            s.max_len = config
                .autoreplace
                .items
                .iter()
                .map(|item| item.from.trim().chars().count())
                .max()
                .unwrap_or(0)
                .max(128);
            s.reset_word();
        }
    }

    /// Returns true when the caller must withhold and send a Captured event.
    pub fn capture(
        &self,
        event: InputEvent,
        lang: Option<Lang>,
        target: Option<InputTarget>,
    ) -> bool {
        let Ok(mut s) = self.0.lock() else {
            return false;
        };
        if target.is_some() {
            s.last_external_target = target;
        }
        let (key, pressed, repeat, time) = match event {
            InputEvent::Key {
                key,
                pressed,
                repeat,
                time,
                injected: false,
            } => (Some(key), pressed, repeat, time),
            InputEvent::UnknownKey {
                code: _,
                pressed,
                time,
            } => (None, pressed, false, time),
            InputEvent::MouseButton { .. } => {
                s.epoch = s.epoch.wrapping_add(1);
                s.reset_word();
                return false;
            }
            _ => return false,
        };
        let reserved = key.is_some_and(|key| s.reserved.contains(&key));
        let mut intercept = s.active || reserved;
        if !intercept {
            s.prepare(target, lang, time);
            if let Some(key) = key
                && pressed
                && !repeat
                && !s.capturing
                && target.is_some()
            {
                let hotkey = s.hotkeys_enabled
                    && [
                        HotkeyAction::ShowAutoreplaceMenu,
                        HotkeyAction::ToggleAutoreplaceList,
                    ]
                    .into_iter()
                    .any(|action| {
                        s.hotkeys
                            .get(action)
                            .0
                            .is_some_and(|h| h.matches(key, s.mods))
                    });
                let boundary = s.autoswitch
                    && s.lang.is_some()
                    && !s.keys.is_empty()
                    && s.overflow == 0
                    && (key == PhysKey::Space
                        || (!s.no_switch_on_tab_enter
                            && matches!(key, PhysKey::Enter | PhysKey::NumpadEnter)));
                intercept = hotkey
                    || (s.mods.is_empty()
                        && (boundary || (s.matches() && accepts(s.trigger, key))));
            }
        }
        if intercept {
            s.active = true;
            s.pending += 1;
            if let Some(key) = key {
                if pressed {
                    s.reserved.insert(key);
                } else {
                    s.reserved.remove(&key);
                }
            }
        } else {
            s.feed(event, lang);
        }
        intercept
    }

    /// A picker consumed this press before returning focus to the editor.
    pub fn suppress_until_release(&self, key: PhysKey) {
        if let Ok(mut s) = self.0.lock() {
            s.discard_until_release.insert(key);
            if matches!(key, PhysKey::Enter | PhysKey::NumpadEnter) {
                s.discard_until_release.insert(PhysKey::Enter);
                s.discard_until_release.insert(PhysKey::NumpadEnter);
            }
        }
    }

    pub fn discard(&self, event: InputEvent) -> bool {
        let InputEvent::Key {
            key,
            pressed,
            injected: false,
            ..
        } = event
        else {
            return false;
        };
        let Ok(mut s) = self.0.lock() else {
            return false;
        };
        if !s.discard_until_release.contains(&key) {
            return false;
        }
        if !pressed {
            s.discard_until_release.remove(&key);
            if matches!(key, PhysKey::Enter | PhysKey::NumpadEnter) {
                s.discard_until_release.remove(&PhysKey::Enter);
                s.discard_until_release.remove(&PhysKey::NumpadEnter);
            }
        }
        true
    }

    pub fn replayed(&self, event: InputEvent, lang: Option<Lang>, target: Option<InputTarget>) {
        if let Ok(mut s) = self.0.lock() {
            let time = match event {
                InputEvent::Key { time, .. } | InputEvent::UnknownKey { time, .. } => time,
                _ => return,
            };
            s.prepare(target, lang, time);
            s.feed(event, lang);
        }
    }

    pub fn complete_event(&self, hold: bool) {
        if let Ok(mut s) = self.0.lock() {
            s.pending = s.pending.saturating_sub(1);
            s.active = hold || s.pending > 0;
        }
    }

    /// A physical click invalidates delayed work even inside the same HWND.
    pub fn epoch(&self) -> u64 {
        self.0.lock().map_or(u64::MAX, |s| s.epoch)
    }

    /// Last editor seen before a tray/menu click moved foreground focus away.
    pub fn last_target(&self) -> Option<InputTarget> {
        self.0.lock().ok().and_then(|s| s.last_external_target)
    }

    pub fn begin_operation(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.active = true;
        }
    }
    pub fn end_operation(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.active = s.pending > 0;
            s.reset_word();
        }
    }
    pub fn reset_word(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.reset_word();
        }
    }
    pub fn set_capture(&self, on: bool) {
        if let Ok(mut s) = self.0.lock() {
            s.capturing = on;
            s.reset_word();
        }
    }
    pub fn set_hotkeys_enabled(&self, enabled: bool) {
        if let Ok(mut s) = self.0.lock() {
            s.hotkeys_enabled = enabled;
        }
    }
    pub fn set_autoswitch(&self, enabled: bool) {
        if let Ok(mut s) = self.0.lock() {
            s.autoswitch = enabled;
        }
    }
    pub fn fail_open(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.active = false;
            s.pending = 0;
            s.reserved.clear();
            s.discard_until_release.clear();
            s.reset_word();
        }
    }
}

/// Keys consumed as confirmation (Esc cancels only a tooltip).
pub fn accepts(trigger: AutoReplaceTrigger, key: PhysKey) -> bool {
    match trigger {
        AutoReplaceTrigger::Tooltip => matches!(
            key,
            PhysKey::Enter | PhysKey::NumpadEnter | PhysKey::Tab | PhysKey::Escape
        ),
        AutoReplaceTrigger::Space => key == PhysKey::Space,
        AutoReplaceTrigger::Enter => matches!(key, PhysKey::Enter | PhysKey::NumpadEnter),
        AutoReplaceTrigger::Tab => key == PhysKey::Tab,
        AutoReplaceTrigger::Hotkey => false,
    }
}
