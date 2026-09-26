//! Input processing: word buffer, automatic conversion, hotkeys, undo.
//!
//! The processor sees every physical key event, keeps the word being typed as
//! physical key presses and, when a separator ends the word, asks the detector
//! whether it was typed in the wrong layout. A conversion erases the word and
//! the separator, switches the layout and replays the same keys.

use crate::sounds::{self, Sound};
use crate::{ConversionKind, Event, UiRequest};
use okbs_core::autoreplace::AutoReplacer;
use okbs_core::config::{
    AutoReplaceItem, AutoReplaceTrigger, Config, HotkeyAction, SoundEvent, SwitchKey,
};
use okbs_core::detect::{Decision, Detector};
use okbs_core::layouts::{self, KeyPress};
use okbs_core::rules::suggest_rule;
use okbs_core::{Lang, ModState, PhysKey, text, translit};
use okbs_platform::autoreplace_gate::{AutoReplaceGate, accepts};
use okbs_platform::{
    Clipboard, FocusEvent, FocusInfo, Injector, InputEvent, InputTarget, KeyStroke, LayoutId,
    LayoutInfo, LayoutManager, PlatformError, SoundPlayer,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The buffer is dropped after this much inactivity.
const IDLE_RESET: Duration = Duration::from_secs(30);
/// Two spaces within this interval become «, » («Запятая по двойному нажатию клавиши Пробел»).
const DOUBLE_SPACE: Duration = Duration::from_millis(400);
/// A layout change within this time after our own switch is ours.
const OWN_LAYOUT_CHANGE: Duration = Duration::from_secs(2);
/// Focus-dependent checks (exclusions) are cached this long without focus events.
const FOCUS_CACHE: Duration = Duration::from_secs(1);
/// Clipboard marker used to detect whether Ctrl+C copied anything.
const CLIPBOARD_MARKER: &str = "\u{2063}okbswitch\u{2063}";

/// Platform services used by the processor.
pub struct Backends {
    /// Keyboard injection.
    pub injector: Box<dyn Injector>,
    /// Layout control.
    pub layouts: Box<dyn LayoutManager>,
    /// Clipboard for selection and clipboard operations.
    pub clipboard: Option<Box<dyn Clipboard>>,
    /// Sound output.
    pub sound: Option<Box<dyn SoundPlayer>>,
    /// Foreground window information (own windows, excluded programs).
    pub focus: Option<Box<dyn FocusInfo>>,
}

impl std::fmt::Debug for Backends {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backends")
            .field("clipboard", &self.clipboard.is_some())
            .field("sound", &self.sound.is_some())
            .finish_non_exhaustive()
    }
}

/// Waiting times of clipboard operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long to wait for the application to copy the selection.
    pub copy_timeout: Duration,
    /// Pause after pasting before the clipboard is restored.
    pub paste_settle: Duration,
    /// Polling interval while waiting for the clipboard.
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            copy_timeout: Duration::from_millis(500),
            paste_settle: Duration::from_millis(150),
            poll: Duration::from_millis(15),
        }
    }
}

/// Text operations on the selection or the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOp {
    /// Convert between RU and EN layouts.
    Layout,
    /// Swap letter case.
    InvertCase,
    /// Transliterate Cyrillic ↔ Latin.
    Transliterate,
    /// Numbers in words.
    NumberToWords,
}

impl TextOp {
    fn apply(self, text: &str) -> Option<String> {
        match self {
            TextOp::Layout => Some(layouts::switch_text_layout(text)),
            TextOp::InvertCase => Some(text::invert_case(text)),
            TextOp::Transliterate => Some(translit::transliterate(text)),
            TextOp::NumberToWords => okbs_core::numwords::convert(text),
        }
    }
}

/// Work that must wait until all modifier keys are released, so that held
/// Shift or Alt does not change injected keys.
#[derive(Debug, Clone, PartialEq)]
enum Deferred {
    InsertAutoreplace {
        item: AutoReplaceItem,
        target: Option<InputTarget>,
    },
    /// A clipboard history entry chosen while modifiers were still held.
    InsertText {
        text: String,
        target: Option<InputTarget>,
    },
    /// Retyping of the word that has just been finished: conversion to `to`
    /// and/or a case fix (`caps_off` turns Caps Lock off first).
    Retype {
        to: Lang,
        automatic: bool,
        caps_off: bool,
    },
    /// A hotkey action.
    Action(HotkeyAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Conversion {
    from: Lang,
    to: Lang,
    automatic: bool,
}

/// The last finished word with the separators typed after it.
#[derive(Debug, Clone, PartialEq)]
struct LastWord {
    keys: Vec<KeyPress>,
    separator: Vec<KeyPress>,
    /// Layout the keys were typed in.
    typed_in: Lang,
    conversion: Option<Conversion>,
    target: Option<InputTarget>,
}

/// Word snapshot kept while its spelling is checked and the suggestion window
/// is open. The normal typing state is reset when that window receives focus.
#[derive(Debug, Clone, PartialEq)]
struct PendingSpelling {
    original: String,
    last: LastWord,
    captured_at: Instant,
}

/// The last spelling correction, undone by «Отменить конвертацию» (Break)
/// until the user types, clicks or moves elsewhere.
#[derive(Debug, Clone, PartialEq)]
struct SpellingUndo {
    target: InputTarget,
    original: String,
    corrected: String,
}

impl LastWord {
    fn all_keys(&self) -> impl Iterator<Item = KeyPress> + '_ {
        self.keys.iter().chain(&self.separator).copied()
    }

    /// Layout the word is displayed in now.
    fn shown_in(&self) -> Lang {
        self.conversion.map_or(self.typed_in, |c| c.to)
    }
}

fn keys_of(last: &Option<LastWord>) -> Vec<KeyPress> {
    last.as_ref().map(|l| l.keys.clone()).unwrap_or_default()
}

/// «Программы-исключения»: by executable path or file name, window title
/// substring (case-sensitive) or program folder.
pub fn is_excluded(
    exclusions: &okbs_core::config::Exclusions,
    window: &okbs_platform::WindowInfo,
) -> bool {
    let normalize = |s: &str| {
        s.trim()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_lowercase()
    };
    let exe = window
        .executable
        .as_ref()
        .map(|p| normalize(&p.to_string_lossy()));
    let exe_name = exe
        .as_deref()
        .and_then(|e| e.rsplit('/').next())
        .map(str::to_string);
    let app_id = window.app_id.as_deref().map(normalize);
    let by_exe = exclusions.executables.iter().any(|e| {
        let want = normalize(&e.path);
        !want.is_empty()
            && (exe.as_deref() == Some(want.as_str())
                || (!want.contains('/')
                    && (exe_name.as_deref() == Some(want.as_str())
                        || app_id.as_deref() == Some(want.as_str()))))
    });
    let by_title = window.title.as_deref().is_some_and(|title| {
        exclusions
            .titles
            .iter()
            .any(|t| !t.contains.is_empty() && title.contains(&t.contains))
    });
    let by_folder = exe.as_deref().is_some_and(|exe| {
        exclusions.folders.iter().any(|f| {
            let folder = normalize(&f.path);
            !folder.is_empty() && exe.starts_with(&format!("{folder}/"))
        })
    });
    by_exe || by_title || by_folder
}

/// The input processor. Single-threaded; the engine thread owns it.
pub struct Processor {
    input_gate: Option<Arc<AutoReplaceGate>>,
    consumed_keys: HashSet<PhysKey>,
    operation_target: Option<InputTarget>,
    operation_epoch: Option<u64>,
    last_input_target: Option<InputTarget>,
    /// Last text reported for the clipboard history, to skip repeats and the
    /// texts this program itself put on the clipboard for a moment.
    last_clipboard_text: Option<String>,
    /// Layout to return to once the menu opened with Alt is closed
    /// («Исправлять раскладку при работе с меню, содержащим горячие клавиши»).
    layout_before_menu: Option<Lang>,
    hint: Option<usize>,
    config: Config,
    detector: Detector,
    autoreplacer: AutoReplacer,
    backends: Backends,
    mods: ModState,
    caps: bool,
    word: Vec<KeyPress>,
    word_lang: Option<Lang>,
    word_blocked: bool,
    /// The word follows the previous one without a space (`site.com`,
    /// `C:\dir`, `user@mail`): part of an address, path or code.
    word_glued: bool,
    block_next_word: bool,
    last: Option<LastWord>,
    pending_spelling: Option<PendingSpelling>,
    spelling_undo: Option<SpellingUndo>,
    /// A word snapshot was dropped; the popup offering it has to close.
    spelling_expired: bool,
    /// Words whose correction was undone or skipped: not checked again while
    /// the program runs.
    declined_spelling: HashSet<String>,
    deferred: Vec<Deferred>,
    cancels: HashMap<String, u32>,
    last_key_time: Option<Instant>,
    layouts: Vec<LayoutInfo>,
    timing: Timing,
    /// Language of the previous word on the current line.
    context: Option<Lang>,
    /// Language of the immediately preceding token, separated by spaces only.
    previous_word: Option<Lang>,
    /// The settings window waits for a key combination.
    capturing: bool,
    /// A single switch key is held and may trigger on release.
    switch_candidate: Option<(PhysKey, Instant)>,
    /// Our own recent layout change, to tell it from the user's.
    own_layout_change: Option<(Lang, Instant)>,
    /// Layout last chosen with the switch keys («Единая раскладка»).
    chosen_lang: Option<Lang>,
    /// Time of the previous Space.
    last_space: Option<Instant>,
    /// The platform suppresses physical Caps Lock when configured to.
    swallows_capslock: bool,
    /// Cached «foreground program is excluded».
    excluded_cache: Option<(Instant, bool)>,
}

impl std::fmt::Debug for Processor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Processor")
            .field("word_len", &self.word.len())
            .field("autoswitch", &self.config.general.autoswitch)
            .finish_non_exhaustive()
    }
}

impl Processor {
    /// Creates a processor with a detector built from `config`.
    pub fn new(config: Config, backends: Backends) -> Self {
        let detector = Detector::from_config(&config);
        Self::with_detector(config, detector, backends)
    }

    /// Creates a processor with a custom detector.
    pub fn with_detector(config: Config, detector: Detector, backends: Backends) -> Self {
        let mut processor = Self {
            input_gate: None,
            consumed_keys: HashSet::new(),
            operation_target: None,
            operation_epoch: None,
            last_input_target: None,
            last_clipboard_text: None,
            layout_before_menu: None,
            hint: None,
            autoreplacer: AutoReplacer::new(&config.autoreplace),
            config,
            detector,
            backends,
            mods: ModState::default(),
            caps: false,
            word: Vec::new(),
            word_lang: None,
            word_blocked: false,
            word_glued: false,
            block_next_word: false,
            last: None,
            pending_spelling: None,
            spelling_undo: None,
            spelling_expired: false,
            declined_spelling: HashSet::new(),
            deferred: Vec::new(),
            cancels: HashMap::new(),
            last_key_time: None,
            layouts: Vec::new(),
            timing: Timing::default(),
            context: None,
            previous_word: None,
            capturing: false,
            switch_candidate: None,
            own_layout_change: None,
            chosen_lang: None,
            last_space: None,
            swallows_capslock: false,
            excluded_cache: None,
        };
        processor.refresh_layouts();
        processor
    }

    /// Declares that the platform suppresses physical Caps Lock presses when
    /// Caps Lock is disabled or used as the switch key (Windows hook).
    pub fn set_swallows_capslock(&mut self, on: bool) {
        self.swallows_capslock = on;
    }

    /// Connects the source's ordered interception gate (Windows).
    pub fn set_input_gate(&mut self, gate: Arc<AutoReplaceGate>) {
        gate.configure(&self.config);
        self.input_gate = Some(gate);
    }

    /// Enters or leaves the hotkey capture mode.
    pub fn set_capture(&mut self, on: bool) {
        if let Some(gate) = &self.input_gate {
            gate.set_capture(on);
        }
        self.capturing = on;
        self.reset_all();
    }

    /// Replaces the clipboard waiting times.
    pub fn set_timing(&mut self, timing: Timing) {
        self.timing = timing;
    }

    /// Current configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Sets the Caps Lock state, e.g. from the platform at startup.
    pub fn set_caps_lock(&mut self, on: bool) {
        self.caps = on;
    }

    /// Replaces the configuration and rebuilds the detector.
    pub fn apply_config(&mut self, config: Config) {
        if let Some(gate) = &self.input_gate {
            gate.configure(&config);
        }
        self.detector = Detector::from_config(&config);
        self.autoreplacer = AutoReplacer::new(&config.autoreplace);
        self.config = config;
        self.expire_spelling("config_changed");
        self.spelling_undo = None;
        self.reset_all();
    }

    /// Drops the word snapshot of a pending spelling check. A popup offering
    /// a replacement for it could never apply it, so the UI is told to close.
    fn expire_spelling(&mut self, reason: &str) {
        if self.pending_spelling.take().is_some() {
            tracing::debug!(target: "okbs_spelling", reason, "snapshot invalidated");
            self.spelling_expired = true;
        }
    }

    /// Turns automatic switching on or off; returns `true` if it changed.
    pub fn set_autoswitch(&mut self, on: bool) -> bool {
        let changed = self.config.general.autoswitch != on;
        self.config.general.autoswitch = on;
        if let Some(gate) = &self.input_gate {
            gate.set_hotkeys_enabled(on || !self.config.general.hotkeys_off_when_autoswitch_off);
            gate.set_autoswitch(on);
        }
        changed
    }

    /// Turns sounds on or off; returns `true` if it changed.
    pub fn set_sounds(&mut self, on: bool) -> bool {
        let changed = self.config.sounds.enabled != on;
        self.config.sounds.enabled = on;
        changed
    }

    fn refresh_layouts(&mut self) {
        match self.backends.layouts.layouts() {
            Ok(list) => self.layouts = list,
            Err(err) => tracing::warn!("cannot list keyboard layouts: {err}"),
        }
    }

    /// Language of a layout id, refreshing the layout list once if unknown.
    pub fn lang_of(&mut self, id: LayoutId) -> Option<Lang> {
        if let Some(info) = self.layouts.iter().find(|l| l.id == id) {
            return info.lang;
        }
        self.refresh_layouts();
        self.layouts
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| l.lang)
    }

    fn current_lang(&mut self) -> Option<Lang> {
        match self.backends.layouts.current() {
            Ok(id) => self.lang_of(id),
            Err(err) => {
                tracing::debug!("cannot read the current layout: {err}");
                None
            }
        }
    }

    fn set_lang(&mut self, lang: Lang) -> Result<(), PlatformError> {
        let find =
            |layouts: &[LayoutInfo]| layouts.iter().find(|l| l.lang == Some(lang)).map(|l| l.id);
        let id = match find(&self.layouts) {
            Some(id) => id,
            None => {
                self.refresh_layouts();
                find(&self.layouts).ok_or_else(|| {
                    PlatformError::Other(format!("no {lang} keyboard layout installed"))
                })?
            }
        };
        self.own_layout_change = Some((lang, Instant::now()));
        self.backends.layouts.set(id)
    }

    fn reset_word(&mut self) {
        self.word.clear();
        self.word_lang = None;
        self.word_blocked = false;
    }

    fn reset_all(&mut self) {
        self.reset_word();
        self.last = None;
        self.deferred.clear();
        self.context = None;
        self.previous_word = None;
        self.last_space = None;
        self.switch_candidate = None;
    }

    /// Handles a focus change: the caret moved to another place.
    /// Alt alone opens the menu bar: while it is open the layout has to match
    /// the underlined access keys, otherwise Alt+Ф never reaches «&File».
    fn match_menu_layout(&mut self) {
        if !self.config.advanced.fix_layout_in_menus || self.layout_before_menu.is_some() {
            return;
        }
        let Some(focus) = self.backends.focus.as_ref() else {
            return;
        };
        let menu = match focus.menu_access_language() {
            Ok(Some(lang)) => lang,
            Ok(None) => return,
            Err(err) => {
                tracing::debug!("cannot read the menu language: {err}");
                return;
            }
        };
        if !self.config.general.language_pair.contains(&menu) {
            return;
        }
        let Some(current) = self.current_lang() else {
            return;
        };
        if current == menu || self.foreground_excluded() {
            return;
        }
        match self.set_lang(menu) {
            Ok(()) => {
                self.layout_before_menu = Some(current);
                tracing::debug!(%menu, "layout matched to the menu access keys");
            }
            Err(err) => tracing::debug!("cannot match the menu layout: {err}"),
        }
    }

    /// Puts back the layout that was active before the menu was opened.
    fn restore_layout_after_menu(&mut self) {
        let Some(lang) = self.layout_before_menu.take() else {
            return;
        };
        if let Err(err) = self.set_lang(lang) {
            tracing::debug!("cannot restore the layout after the menu: {err}");
        }
    }

    pub fn handle_focus(&mut self, event: &FocusEvent) {
        if matches!(event, FocusEvent::MenuClosed | FocusEvent::WindowChanged(_)) {
            self.restore_layout_after_menu();
        }
        if matches!(event, FocusEvent::MenuClosed) {
            return;
        }
        let own_window = matches!(
            event,
            FocusEvent::WindowChanged(Some(window))
                if window.pid == Some(std::process::id())
        );
        let original_target = self
            .pending_spelling
            .as_ref()
            .is_some_and(|pending| self.target_matches(pending.last.target));
        if let Some(pending) = &self.pending_spelling {
            tracing::debug!(target: "okbs_spelling", expected = ?pending.last.target,
                current = ?self.backends.focus.as_ref().and_then(|f| f.input_target().ok().flatten()),
                own_window, original_target, retained = own_window || original_target,
                age_ms = pending.captured_at.elapsed().as_millis() as u64,
                "focus event while spelling pending");
        }
        if !own_window && !original_target {
            self.expire_spelling("focus_changed");
        }
        // Returning to the corrected control after a popup choice keeps the undo.
        let undo_target = self
            .spelling_undo
            .as_ref()
            .is_some_and(|undo| self.target_matches(Some(undo.target)));
        if !own_window && !undo_target {
            self.spelling_undo = None;
        }
        self.remember_target();
        self.reset_all();
        self.block_next_word = false;
        self.excluded_cache = None;
        if matches!(event, FocusEvent::WindowChanged(_))
            && self.config.switching.single_layout_for_all_windows
            && self.config.switching.switch_key != SwitchKey::None
            && let Some(lang) = self.chosen_lang
            && self.current_lang() != Some(lang)
            && let Err(err) = self.set_lang(lang)
        {
            tracing::debug!("cannot apply the single layout: {err}");
        }
    }

    /// Handles a layout change notification.
    pub fn handle_layout_change(&mut self, id: LayoutId) -> Vec<Event> {
        let lang = self.lang_of(id);
        let ours = matches!(self.own_layout_change, Some((l, t)) if Some(l) == lang && t.elapsed() <= OWN_LAYOUT_CHANGE);
        if ours {
            self.own_layout_change = None;
        } else {
            // The user switched the layout with the system shortcut or the language bar.
            if self.config.rules_options.improve_switching {
                self.previous_word = None;
            }
            self.block_next_word |= self
                .config
                .troubleshooting
                .no_switch_after
                .manual_layout_change;
            self.play(Sound::LayoutChanged);
        }
        vec![Event::LayoutChanged(self.layout_info(id))]
    }

    /// Description of a layout id from the cached list.
    fn layout_info(&self, id: LayoutId) -> Option<LayoutInfo> {
        self.layouts.iter().find(|l| l.id == id).cloned()
    }

    /// Description of the first layout of `lang`.
    fn layout_info_for(&self, lang: Lang) -> Option<LayoutInfo> {
        self.layouts.iter().find(|l| l.lang == Some(lang)).cloned()
    }

    /// Handles one input event and returns resulting notifications.
    pub fn handle_input(&mut self, event: InputEvent) -> Vec<Event> {
        if matches!(
            event,
            InputEvent::CapturedKey { .. } | InputEvent::CapturedUnknown { .. }
        ) {
            return self.handle_captured(event);
        }
        if matches!(
            event,
            InputEvent::Key {
                injected: false,
                ..
            }
        ) {
            self.remember_target();
        }
        let user_action = matches!(
            event,
            InputEvent::Key {
                pressed: true,
                injected: false,
                ..
            } | InputEvent::UnknownKey { pressed: true, .. }
                | InputEvent::MouseButton { .. }
        );
        let own_window = match event {
            InputEvent::MouseButton { in_own_window, .. } => in_own_window,
            _ => self.backends.focus.as_ref().is_some_and(|focus| {
                matches!(
                    focus.active_window(),
                    Ok(Some(window)) if window.pid == Some(std::process::id())
                )
            }),
        };
        if user_action && let Some(pending) = &self.pending_spelling {
            let kind = match event {
                InputEvent::MouseButton { .. } => "mouse_button",
                InputEvent::Key {
                    key: PhysKey::Space,
                    ..
                } => "space",
                InputEvent::Key {
                    key: PhysKey::Enter | PhysKey::NumpadEnter,
                    ..
                } => "enter",
                InputEvent::Key { .. } => "key",
                _ => "unknown_key",
            };
            tracing::debug!(target: "okbs_spelling", kind, own_window, retained = own_window,
                expected = ?pending.last.target,
                current = ?self.backends.focus.as_ref().and_then(|f| f.input_target().ok().flatten()),
                age_ms = pending.captured_at.elapsed().as_millis() as u64,
                "input event while spelling pending");
        }
        if user_action && !own_window {
            self.expire_spelling("input");
            // Break right after a correction undoes it; anything else keeps it.
            let undo_key = matches!(event, InputEvent::Key { key, .. }
                if key.is_modifier()
                    || self.matching_hotkey(key) == Some(HotkeyAction::CancelOrConvertLastWord));
            if !undo_key {
                self.spelling_undo = None;
            }
        }
        let mut out = Vec::new();
        match event {
            InputEvent::Key { injected: true, .. } => {}
            InputEvent::CapturedKey { .. } | InputEvent::CapturedUnknown { .. } => unreachable!(),
            InputEvent::MouseButton { .. } => {
                self.reset_all();
                self.block_next_word = false;
            }
            InputEvent::UnknownKey { pressed, .. } => {
                if pressed {
                    self.reset_all();
                }
            }
            InputEvent::Key {
                key,
                pressed: false,
                time,
                ..
            } => {
                self.mods.set(key, false);
                if let Some((candidate, pressed_at)) = self.switch_candidate.take()
                    && candidate == key
                    && self.mods.is_empty()
                {
                    let max_hold = Duration::from_millis(u64::from(
                        self.config.switching.switch_key_max_hold_ms,
                    ));
                    if time.saturating_duration_since(pressed_at) <= max_hold {
                        self.switch_key_released(key, &mut out);
                    }
                }
                if self.mods.is_empty() {
                    self.run_deferred(&mut out);
                }
            }
            InputEvent::Key {
                key,
                pressed: true,
                repeat,
                time,
                ..
            } => self.key_down(key, repeat, time, &mut out),
        }
        out.extend(self.autoreplace_feedback());
        out
    }

    fn remember_target(&mut self) {
        if let Some(target) = self
            .backends
            .focus
            .as_ref()
            .and_then(|f| f.input_target().ok().flatten())
        {
            if self.last_input_target != Some(target) {
                self.excluded_cache = None;
            }
            self.last_input_target = Some(target);
        }
    }

    fn target_matches(&self, target: Option<InputTarget>) -> bool {
        target.is_some()
            && self
                .backends
                .focus
                .as_ref()
                .and_then(|f| f.input_target().ok().flatten())
                == target
    }

    fn handle_captured(&mut self, event: InputEvent) -> Vec<Event> {
        let (raw, target, epoch) = match event {
            InputEvent::CapturedKey {
                epoch,
                key,
                pressed,
                repeat,
                time,
                target,
            } => (
                InputEvent::Key {
                    key,
                    pressed,
                    repeat,
                    time,
                    injected: false,
                },
                target,
                epoch,
            ),
            InputEvent::CapturedUnknown {
                epoch,
                code,
                pressed,
                time,
                target,
            } => (
                InputEvent::UnknownKey {
                    code,
                    pressed,
                    time,
                },
                target,
                epoch,
            ),
            _ => unreachable!(),
        };
        let mut out = Vec::new();
        let gate = self.input_gate.clone();
        self.operation_target = target;
        self.operation_epoch = Some(epoch);
        if !self.target_matches(target) || !self.epoch_matches(Some(epoch)) {
            if let Some(gate) = &gate {
                gate.reset_word();
            }
            // Never send a delayed confirmation or text to a different control.
            // Modifier releases still have to balance already-delivered presses.
            if let InputEvent::Key {
                key,
                pressed: false,
                ..
            } = raw
            {
                if key.is_modifier() {
                    let _ = self.backends.injector.send(&[KeyStroke::release(key)]);
                    self.mods.set(key, false);
                    let lang = self.current_lang();
                    if let Some(gate) = &gate {
                        gate.replayed(raw, lang, target);
                    }
                }
                self.consumed_keys.remove(&key);
            }
            self.reset_all();
        } else {
            let mut consume = false;
            if let InputEvent::Key {
                key,
                pressed,
                repeat,
                ..
            } = raw
            {
                let previously_consumed = if pressed {
                    self.consumed_keys.contains(&key)
                } else {
                    self.consumed_keys.remove(&key)
                };
                if previously_consumed {
                    consume = true;
                } else if pressed
                    && !self.capturing
                    && !(self.config.troubleshooting.ignore_excluded_apps_completely
                        && self.foreground_excluded())
                {
                    if !key.is_modifier() && self.matching_hotkey(key).is_some() {
                        self.consumed_keys.insert(key);
                        out.extend(self.handle_input(raw));
                        consume = true;
                    } else if !repeat
                        && self.mods.is_empty()
                        && accepts(self.config.autoreplace.trigger, key)
                    {
                        if key == PhysKey::Escape
                            && self.autoreplace_candidate().is_some()
                            && !self.focus_blocks()
                        {
                            if let Some(gate) = &gate {
                                gate.reset_word();
                            }
                            self.reset_all();
                            consume = true;
                        } else if key != PhysKey::Escape {
                            consume =
                                self.expand_current(key == PhysKey::Space, false, false, &mut out);
                        }
                        if consume {
                            self.consumed_keys.insert(key);
                        }
                    }
                }
            }
            if !consume
                && let InputEvent::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    time,
                    ..
                } = raw
                && !self.capturing
                && self.mods.is_empty()
                && self.config.general.autoswitch
                && (key == PhysKey::Space
                    || (!self.config.troubleshooting.no_switch_on_tab_enter
                        && matches!(key, PhysKey::Enter | PhysKey::NumpadEnter)))
            {
                if self
                    .last_key_time
                    .is_some_and(|last| time.saturating_duration_since(last) > IDLE_RESET)
                {
                    self.reset_all();
                }
                // The application has not received the separator yet. In a
                // terminal this is the last opportunity to correct the command.
                self.finish_word_at_boundary(KeyPress::plain(key), false, &mut out);
                if out.iter().any(|e| matches!(e, Event::Error(_)))
                    || !self.target_matches(target)
                    || !self.epoch_matches(Some(epoch))
                {
                    // Do not submit a stale or partially rewritten command.
                    consume = true;
                    self.consumed_keys.insert(key);
                    self.reset_all();
                    if let Some(gate) = &gate {
                        gate.reset_word();
                    }
                }
            }
            if !consume {
                let replay = match raw {
                    InputEvent::Key { key, pressed, .. } => {
                        self.backends.injector.send(&[KeyStroke { key, pressed }])
                    }
                    InputEvent::UnknownKey { code, pressed, .. } => {
                        self.backends.injector.send_unmapped(code, pressed)
                    }
                    _ => Ok(()),
                };
                match replay {
                    Ok(()) => {
                        let lang = self.current_lang();
                        if let Some(gate) = &gate {
                            gate.replayed(raw, lang, target);
                        }
                        out.extend(self.handle_input(raw));
                        // A captured boundary is checked before it reaches the
                        // editor. Its replay resets pending spelling and adds
                        // the separator to LastWord. Snapshot that final state,
                        // before publishing the already queued spelling request.
                        if let Some(text) = out.iter().find_map(|event| match event {
                            Event::CheckSpelling {
                                text,
                                interactive: true,
                                ..
                            } => Some(text),
                            _ => None,
                        }) && let Some(last) = self.last.clone()
                        {
                            self.pending_spelling = Some(PendingSpelling {
                                original: text.clone(),
                                last,
                                captured_at: Instant::now(),
                            });
                            // The replay only moved this word's own snapshot on.
                            out.retain(|event| *event != Event::SpellingExpired);
                            tracing::debug!(target: "okbs_spelling", ?target, epoch,
                                "snapshot refreshed after captured boundary replay");
                        }
                    }
                    Err(err) => self.fail("cannot replay pending input", &err, &mut out),
                }
            }
        }
        self.operation_target = None;
        self.operation_epoch = None;
        if let Some(gate) = gate {
            gate.complete_event(!self.deferred.is_empty());
        }
        out.extend(self.autoreplace_feedback());
        out
    }

    fn autoreplace_candidate(&mut self) -> Option<usize> {
        if self.word.is_empty() || self.word_blocked || self.autoreplacer.is_empty() {
            return None;
        }
        let lang = self.word_lang?;
        if self.current_lang() != Some(lang) {
            return None;
        }
        let typed = layouts::render(&self.word, self.detector.keymap(lang));
        let other = layouts::render(&self.word, self.detector.keymap(lang.other()));
        self.autoreplacer
            .find(&typed, Some(&other))
            .map(|entry| entry.index)
    }

    fn epoch_matches(&self, epoch: Option<u64>) -> bool {
        epoch.is_none_or(|expected| {
            self.input_gate
                .as_ref()
                .is_none_or(|gate| gate.epoch() == expected)
        })
    }

    /// Show/hide feedback only when the candidate changes; never expose password input.
    /// Also reports a spelling snapshot dropped since the last call.
    pub fn autoreplace_feedback(&mut self) -> Vec<Event> {
        if self
            .last_key_time
            .is_some_and(|time| time.elapsed() > IDLE_RESET)
        {
            self.reset_all();
            self.last_key_time = None;
        }
        let mut out = Vec::new();
        if std::mem::take(&mut self.spelling_expired) {
            out.push(Event::SpellingExpired);
        }
        out.extend(self.autoreplace_hint());
        out
    }

    fn autoreplace_hint(&mut self) -> Vec<Event> {
        let mut candidate = if self.config.autoreplace.trigger == AutoReplaceTrigger::Tooltip
            && self.config.advanced.show_tooltips
            && !self.capturing
        {
            self.autoreplace_candidate()
        } else {
            None
        };
        if candidate.is_some() && candidate != self.hint && self.focus_blocks() {
            candidate = None;
        }
        if candidate == self.hint {
            return Vec::new();
        }
        self.hint = candidate;
        vec![Event::AutoreplaceHint(candidate)]
    }

    pub fn autoreplace_list(&mut self, toggle: bool) -> Vec<Event> {
        self.remember_target();
        self.reset_all();
        let mut out = self.autoreplace_feedback();
        out.push(Event::AutoreplaceList {
            toggle,
            target: self.insertion_target(),
        });
        out
    }

    /// «Показать историю буфера обмена»: the controller owns the list, the
    /// engine only remembers where the chosen entry has to be inserted.
    pub fn clipboard_history(&mut self) -> Vec<Event> {
        self.remember_target();
        self.reset_all();
        let mut out = self.autoreplace_feedback();
        out.push(Event::ClipboardHistory {
            target: self.insertion_target(),
        });
        out
    }

    /// New clipboard contents for «Следить за буфером обмена».
    ///
    /// Notifications are handled after the operation that caused them, so the
    /// short-lived texts this program puts on the clipboard while copying a
    /// selection are already replaced by the restored text and are dropped by
    /// the comparison with the last reported one.
    pub fn handle_clipboard_change(&mut self) -> Vec<Event> {
        if !self.config.advanced.clipboard_history {
            return Vec::new();
        }
        let Some(clipboard) = self.backends.clipboard.as_mut() else {
            return Vec::new();
        };
        let text = match clipboard.text() {
            Ok(Some(text)) => text,
            Ok(None) => return Vec::new(),
            // Another program may hold the clipboard open; the next change is
            // reported again, so this is not worth an error event.
            Err(err) => {
                tracing::debug!("cannot read the clipboard: {err}");
                return Vec::new();
            }
        };
        if text.trim().is_empty()
            || text == CLIPBOARD_MARKER
            || self.last_clipboard_text.as_deref() == Some(text.as_str())
        {
            return Vec::new();
        }
        self.last_clipboard_text = Some(text.clone());
        vec![Event::ClipboardText(text)]
    }

    /// Inserts a clipboard history entry into the application it came from.
    pub fn insert_text(&mut self, text: &str, target: Option<InputTarget>) -> Vec<Event> {
        let mut out = Vec::new();
        if text.is_empty() {
            return out;
        }
        let gate = self.input_gate.clone();
        self.operation_epoch = gate.as_ref().map(|gate| gate.epoch());
        if let Some(gate) = &gate {
            gate.begin_operation();
        }
        if !self.mods.is_empty() {
            self.deferred.push(Deferred::InsertText {
                text: text.to_string(),
                target,
            });
            return out;
        }
        self.reset_all();
        let target = target.or_else(|| self.insertion_target());
        let result = (|| {
            let focus = self
                .backends
                .focus
                .as_ref()
                .ok_or(PlatformError::Unsupported("insertion target"))?;
            let target =
                target.ok_or_else(|| PlatformError::Other("no insertion target".into()))?;
            focus.activate_target(target)?;
            self.excluded_cache = None;
            if matches!(focus.is_password_field(), Ok(Some(true)))
                || (self.config.troubleshooting.ignore_excluded_apps_completely
                    && self.foreground_excluded())
            {
                return Err(PlatformError::Other(
                    "insertion is disabled in this control".into(),
                ));
            }
            self.operation_target = Some(target);
            self.paste_replacement(text, 0, 0)
        })();
        self.operation_target = None;
        self.operation_epoch = None;
        if let Err(err) = result {
            self.fail("cannot insert the clipboard entry", &err, &mut out);
        }
        if let Some(gate) = gate {
            gate.end_operation();
        }
        out.extend(self.autoreplace_feedback());
        out
    }

    fn insertion_target(&self) -> Option<InputTarget> {
        self.backends
            .focus
            .as_ref()
            .and_then(|focus| focus.input_target().ok().flatten())
            .or_else(|| self.input_gate.as_ref().and_then(|gate| gate.last_target()))
            .or(self.last_input_target)
    }

    /// Explicit insertion from a menu/list. Never replace a different entry if
    /// the configuration changed while the picker was open.
    pub fn insert_autoreplace(
        &mut self,
        item: AutoReplaceItem,
        target: Option<InputTarget>,
    ) -> Vec<Event> {
        let mut out = Vec::new();
        if !self.config.autoreplace.enabled || !self.config.autoreplace.items.contains(&item) {
            return out;
        }
        let gate = self.input_gate.clone();
        self.operation_epoch = gate.as_ref().map(|gate| gate.epoch());
        if let Some(gate) = &gate {
            gate.begin_operation();
        }
        if !self.mods.is_empty() {
            self.deferred
                .push(Deferred::InsertAutoreplace { item, target });
            return out;
        }
        self.reset_all();
        let target = target.or_else(|| self.insertion_target());
        let result = (|| {
            let focus = self
                .backends
                .focus
                .as_ref()
                .ok_or(PlatformError::Unsupported("insertion target"))?;
            let target =
                target.ok_or_else(|| PlatformError::Other("no insertion target".into()))?;
            focus.activate_target(target)?;
            self.excluded_cache = None;
            if matches!(focus.is_password_field(), Ok(Some(true)))
                || (self.config.troubleshooting.ignore_excluded_apps_completely
                    && self.foreground_excluded())
            {
                return Err(PlatformError::Other(
                    "insertion is disabled in this control".into(),
                ));
            }
            self.operation_target = Some(target);
            let left = usize::try_from(item.cursor_pos)
                .map_or(0, |pos| item.to.chars().count().saturating_sub(pos));
            self.paste_replacement(&item.to, 0, left)
        })();
        self.operation_target = None;
        self.operation_epoch = None;
        match result {
            Ok(()) => {
                out.push(Event::Autoreplaced);
                self.play(Sound::Autoreplace);
            }
            Err(err) => self.fail("cannot insert autoreplace entry", &err, &mut out),
        }
        if let Some(gate) = gate {
            gate.end_operation();
        }
        out.extend(self.autoreplace_feedback());
        out
    }

    fn key_down(&mut self, key: PhysKey, repeat: bool, time: Instant, out: &mut Vec<Event>) {
        if key.is_modifier() {
            if !repeat {
                let alone = self.mods.is_empty();
                self.mods.set(key, true);
                self.switch_candidate = (alone && self.is_switch_key(key)).then_some((key, time));
                if alone && matches!(key, PhysKey::AltLeft | PhysKey::AltRight) {
                    self.match_menu_layout();
                }
            }
            return;
        }
        self.switch_candidate = None;
        if self.capturing {
            if !repeat {
                self.capturing = false;
                if let Some(gate) = &self.input_gate {
                    gate.set_capture(false);
                }
                if key == PhysKey::Escape && self.mods.is_empty() {
                    out.push(Event::CaptureCancelled);
                } else {
                    out.push(Event::HotkeyCaptured(okbs_core::Hotkey::pressed(
                        key, self.mods,
                    )));
                }
            }
            return;
        }
        if !self.deferred.is_empty() {
            // The user kept typing before releasing modifiers: the text moved on.
            self.deferred.clear();
        }
        if let Some(prev) = self.last_key_time
            && time.saturating_duration_since(prev) > IDLE_RESET
        {
            self.reset_all();
        }
        self.last_key_time = Some(time);

        if key == PhysKey::CapsLock {
            if !repeat {
                let caps_switches = self.config.switching.switch_key == SwitchKey::CapsLock;
                let swallowed = self.swallows_capslock
                    && (caps_switches || self.config.advanced.disable_capslock);
                if !swallowed {
                    self.caps = !self.caps;
                }
                if caps_switches && self.mods.is_empty() {
                    self.toggle_layout(out);
                }
            }
            return;
        }
        if self.config.troubleshooting.ignore_excluded_apps_completely && self.foreground_excluded()
        {
            self.reset_all();
            return;
        }

        if let Some(action) = self.matching_hotkey(key) {
            if !repeat {
                if self.mods.is_empty() {
                    self.run_action(action, out);
                } else {
                    self.deferred.push(Deferred::Action(action));
                }
            }
            return;
        }

        if key == PhysKey::ScrollLock
            && self.config.advanced.scrolllock_as_capslock
            && self.mods.is_empty()
        {
            if !repeat {
                match self.backends.injector.tap(&[], PhysKey::CapsLock) {
                    Ok(()) => self.caps = !self.caps,
                    Err(err) => tracing::warn!("cannot toggle Caps Lock: {err}"),
                }
            }
            return;
        }
        if self.mods.ctrl() || self.mods.alt() || self.mods.win() {
            self.reset_all();
            return;
        }
        let blockers = self.config.troubleshooting.no_switch_after.clone();
        if repeat {
            self.reset_all();
            if key == PhysKey::Backspace && blockers.backspace {
                self.block_next_word = true;
            }
            return;
        }

        match key {
            PhysKey::Backspace => {
                if self.word.pop().is_some() {
                    self.word_blocked |= blockers.backspace;
                    if self.word.is_empty() {
                        self.reset_word();
                        self.block_next_word |= blockers.backspace;
                    }
                } else {
                    self.last = None;
                    self.block_next_word |= blockers.backspace;
                }
            }
            PhysKey::Space => {
                if self.config.autoreplace.trigger == AutoReplaceTrigger::Space
                    && self.autoreplace_space(out)
                {
                    return;
                }
                if self.double_space_comma(time) {
                    return;
                }
                self.last_space = Some(time);
                self.finish_word(KeyPress::plain(key), out);
            }
            PhysKey::Enter | PhysKey::NumpadEnter | PhysKey::Tab => {
                let submitted = key != PhysKey::Tab
                    && self
                        .backends
                        .focus
                        .as_ref()
                        .is_some_and(|f| f.is_terminal().unwrap_or(false));
                if self.config.troubleshooting.no_switch_on_tab_enter || submitted {
                    self.reset_all();
                } else {
                    self.finish_word(KeyPress::plain(key), out);
                    self.context = None;
                }
            }
            PhysKey::ArrowLeft
            | PhysKey::ArrowRight
            | PhysKey::ArrowUp
            | PhysKey::ArrowDown
            | PhysKey::Delete => {
                self.reset_all();
                let block = match key {
                    PhysKey::ArrowLeft => blockers.arrow_left,
                    PhysKey::ArrowRight => blockers.arrow_right,
                    PhysKey::ArrowUp => blockers.arrow_up,
                    PhysKey::ArrowDown => blockers.arrow_down,
                    _ => blockers.delete,
                };
                self.block_next_word |= block;
            }
            _ => {
                let press = KeyPress {
                    key,
                    shift: self.mods.shift(),
                    caps: self.caps,
                };
                let produces_char = Lang::ALL
                    .iter()
                    .any(|&l| layouts::char_for(layouts::builtin_keymap(l), press).is_some());
                if !produces_char {
                    self.reset_all();
                } else if layouts::is_word_key(press) {
                    self.push_word_key(press);
                } else {
                    self.finish_word(press, out);
                }
            }
        }
    }

    fn push_word_key(&mut self, press: KeyPress) {
        if self.word.is_empty() {
            self.word_glued = self.last.as_ref().is_some_and(|last| {
                !last.separator.is_empty()
                    && !last.separator.iter().any(|separator| {
                        matches!(
                            separator.key,
                            PhysKey::Space | PhysKey::Enter | PhysKey::NumpadEnter | PhysKey::Tab
                        )
                    })
            });
            self.last = None;
            self.word_lang = self.current_lang();
            self.word_blocked = std::mem::take(&mut self.block_next_word);
        }
        self.word.push(press);
    }

    /// Expand before the layout detector sees the abbreviation. Space has
    /// already reached the application; keep it after the expanded text.
    fn autoreplace_space(&mut self, out: &mut Vec<Event>) -> bool {
        self.expand_current(true, true, false, out)
    }

    fn expand_current(
        &mut self,
        space: bool,
        separator_typed: bool,
        manual: bool,
        out: &mut Vec<Event>,
    ) -> bool {
        if self.autoreplacer.is_empty() || self.word.is_empty() || !self.mods.is_empty() {
            return false;
        }
        let Some(lang) = self.word_lang else {
            return false;
        };
        if !manual && (self.current_lang() != Some(lang) || self.word_blocked) {
            return false;
        }
        let typed = layouts::render(&self.word, self.detector.keymap(lang));
        let other = layouts::render(&self.word, self.detector.keymap(lang.other()));
        let Some(expansion) = self.autoreplacer.find(&typed, Some(&other)) else {
            return false;
        };
        let mut replacement = expansion.item.to.clone();
        if space {
            replacement.push(' ');
        }
        let caret_left =
            expansion.caret_left + usize::from(space && expansion.item.cursor_pos >= 0);
        let erase = self.word.len() + usize::from(separator_typed);
        if self.focus_blocks_for(manual) {
            return false;
        }
        self.reset_all();
        if let Some(gate) = &self.input_gate {
            gate.reset_word();
        }
        match self.paste_replacement(&replacement, erase, caret_left) {
            Ok(()) => {
                out.push(Event::Autoreplaced);
                self.play(Sound::Autoreplace);
            }
            Err(err) => self.fail("autoreplace failed", &err, out),
        }
        true
    }

    /// Prepare the clipboard before deleting anything. Restore its previous
    /// text on success and on failure, unless someone copied a newer value.
    fn paste_replacement(
        &mut self,
        replacement: &str,
        erase: usize,
        caret_left: usize,
    ) -> Result<(), PlatformError> {
        let target = self.operation_target.or_else(|| {
            self.backends
                .focus
                .as_ref()
                .and_then(|f| f.input_target().ok().flatten())
        });
        let epoch = self
            .operation_epoch
            .or_else(|| self.input_gate.as_ref().map(|gate| gate.epoch()));
        tracing::debug!(target: "okbs_input", ?target, ?epoch, erase, caret_left,
            chars = replacement.chars().count(), "paste preparation");
        if (target.is_some() && !self.target_matches(target)) || !self.epoch_matches(epoch) {
            return Err(PlatformError::Other("focused control changed".into()));
        }
        let clipboard = self
            .backends
            .clipboard
            .as_mut()
            .ok_or(PlatformError::Unsupported("clipboard"))?;
        let saved = clipboard.text()?;
        clipboard.set_text(replacement)?;
        tracing::debug!(target: "okbs_input", "temporary clipboard text set");
        let result = (|| {
            if (target.is_some() && !self.target_matches(target)) || !self.epoch_matches(epoch) {
                return Err(PlatformError::Other("focused control changed".into()));
            }
            self.backends.injector.backspace(erase)?;
            tracing::debug!(target: "okbs_input", erase, "backspaces sent");
            self.backends
                .injector
                .tap(&[PhysKey::ControlLeft], PhysKey::KeyV)?;
            tracing::debug!(target: "okbs_input", "Ctrl+V sent");
            std::thread::sleep(self.timing.paste_settle);
            if (target.is_some() && !self.target_matches(target)) || !self.epoch_matches(epoch) {
                return Err(PlatformError::Other("focused control changed".into()));
            }
            let strokes: Vec<_> = (0..caret_left)
                .flat_map(|_| {
                    [
                        KeyStroke::press(PhysKey::ArrowLeft),
                        KeyStroke::release(PhysKey::ArrowLeft),
                    ]
                })
                .collect();
            self.backends.injector.send(&strokes)
        })();
        self.restore_clipboard(saved, replacement);
        tracing::debug!(target: "okbs_input", success = result.is_ok(),
            "paste finished and clipboard restoration attempted");
        result
    }

    fn finish_word(&mut self, separator: KeyPress, out: &mut Vec<Event>) {
        self.finish_word_at_boundary(separator, true, out);
    }

    fn finish_word_at_boundary(
        &mut self,
        separator: KeyPress,
        separator_typed: bool,
        out: &mut Vec<Event>,
    ) {
        // Only a word followed by a Space can be replaced later; after Enter,
        // Tab or punctuation a suggestion could never be applied.
        let check = separator.key == PhysKey::Space
            && !self.word.is_empty()
            && !self.word_blocked
            && !self.word_glued
            && self.current_lang() == self.word_lang
            && self.config.spellcheck.check_typed_words;
        self.finish_word_layout(separator, separator_typed, out);
        // Command names and arguments are not dictionary words.
        let terminal = || {
            self.backends
                .focus
                .as_ref()
                .is_some_and(|focus| focus.is_terminal().unwrap_or(false))
        };
        if check
            && self.deferred.is_empty()
            && !out.iter().any(|event| matches!(event, Event::Error(_)))
            && !terminal()
            && !self.focus_blocks()
            && let Some(last) = self.last.clone()
        {
            let text = layouts::render(&last.keys, self.detector.keymap(last.shown_in()));
            // `example.com`, `a/b`: the keys of `.` and `/` type letters in
            // the Russian layout, so an address can arrive as one word.
            let plain = text
                .chars()
                .all(|c| c.is_alphabetic() || okbs_core::lm::is_joiner(c));
            if !plain || self.declined_spelling.contains(&text.to_lowercase()) {
                return;
            }
            self.pending_spelling = Some(PendingSpelling {
                original: text.clone(),
                last: last.clone(),
                captured_at: Instant::now(),
            });
            tracing::debug!(target: "okbs_spelling", target = ?last.target,
                chars = text.chars().count(), separator_typed,
                separator_count = last.separator.len(), separator = ?separator.key,
                gated = self.input_gate.is_some(), "word snapshot created");
            out.push(Event::CheckSpelling {
                text,
                settings: self.config.spellcheck.clone(),
                interactive: true,
                target: last.target,
            });
        }
    }

    fn finish_word_layout(
        &mut self,
        separator: KeyPress,
        separator_typed: bool,
        out: &mut Vec<Event>,
    ) {
        let improve = self.config.rules_options.improve_switching;
        if self.word.is_empty() {
            if improve && separator.key != PhysKey::Space {
                self.previous_word = None;
            }
            if separator_typed && let Some(last) = &mut self.last {
                last.separator.push(separator);
            }
            return;
        }
        let keys = std::mem::take(&mut self.word);
        let typed_in = self.word_lang.take();
        let blocked = std::mem::take(&mut self.word_blocked);
        let Some(typed_in) = typed_in else {
            self.last = None;
            if improve {
                self.previous_word = None;
            }
            return;
        };
        let now = self.current_lang();
        let layout_changed = now != Some(typed_in);
        let manual_change_blocks = layout_changed
            && self
                .config
                .troubleshooting
                .no_switch_after
                .manual_layout_change;

        self.last = Some(LastWord {
            keys,
            separator: if separator_typed {
                vec![separator]
            } else {
                Vec::new()
            },
            typed_in,
            conversion: None,
            target: self.last_input_target,
        });
        if blocked || manual_change_blocks {
            if improve {
                self.previous_word = None;
            }
            return;
        }
        let Some(last) = &self.last else { return };
        let previous_word = if !matches!(
            separator.key,
            PhysKey::Space | PhysKey::Enter | PhysKey::NumpadEnter
        ) {
            None
        } else {
            self.previous_word
        };
        let analysis = self.detector.analyze_with_previous_word(
            &last.keys,
            typed_in,
            self.context,
            previous_word,
        );
        // Decision::Switch contains typed text. Log only non-text metadata.
        let (stay_reason, switch_reason) = match &analysis.decision {
            Decision::Stay(reason) => (Some(*reason), None),
            Decision::Switch { reason, .. } => (None, Some(*reason)),
            Decision::Suspicious => (None, None),
        };
        tracing::debug!(
            ?typed_in,
            ?previous_word,
            letters = analysis.current.score.letters,
            ?stay_reason,
            ?switch_reason,
            target = ?analysis.decision.switch_to(),
            suspicious = analysis.decision == Decision::Suspicious,
            "word finished"
        );
        if analysis
            .current
            .score
            .letters
            .max(analysis.other.score.letters)
            >= 2
        {
            match &analysis.decision {
                Decision::Switch { to, .. } => self.context = Some(*to),
                Decision::Stay(okbs_core::detect::StayReason::KnownWord)
                | Decision::Stay(okbs_core::detect::StayReason::ExtraRule) => {
                    self.context = Some(typed_in)
                }
                _ => {}
            }
        }
        let autoswitch = self.config.general.autoswitch;
        let target = match &analysis.decision {
            Decision::Switch { to, .. } if autoswitch => Some(*to),
            _ => None,
        };
        if improve {
            let shown = if target.is_some() {
                &analysis.other
            } else {
                &analysis.current
            };
            // Only the immediately preceding completed token is context.
            // A standalone Cyrillic letter also counts. Unknown tokens clear
            // the context, and prediction alone never changes its language.
            let recognised = shown.valid
                && (shown.in_dictionary
                    || shown.rank.is_some()
                    || shown.known_lenient
                    || shown.score.letters == 1);
            self.previous_word =
                (separator.key == PhysKey::Space && recognised).then_some(shown.lang);
        }
        let final_lang = target.unwrap_or(typed_in);
        let fix = self.case_fix(&keys_of(&self.last), final_lang);
        if target.is_none() && fix.is_none() {
            if autoswitch && analysis.decision == Decision::Suspicious {
                out.push(Event::Suspicious);
                self.play(Sound::Suspicious);
            }
            return;
        }
        if self.focus_blocks() {
            if improve {
                self.previous_word = None;
            }
            return;
        }
        let caps_off = fix.as_ref().is_some_and(|(_, off)| *off);
        if let Some((fixed, _)) = fix
            && let Some(last) = &mut self.last
        {
            last.keys = fixed;
        }
        let job = Deferred::Retype {
            to: final_lang,
            automatic: target.is_some(),
            caps_off,
        };
        if self.mods.is_empty() {
            self.run_job(job, out);
        } else {
            self.deferred.push(job);
        }
    }

    /// Case corrections of a finished word shown in `lang`: two initial
    /// capitals and accidental Caps Lock. Returns the corrected keys and whether
    /// Caps Lock must be turned off.
    fn case_fix(&self, keys: &[KeyPress], lang: Lang) -> Option<(Vec<KeyPress>, bool)> {
        let advanced = &self.config.advanced;
        let map = self.detector.keymap(lang);
        let text = layouts::render(keys, map);
        let letters: Vec<usize> = keys
            .iter()
            .enumerate()
            .filter(|(_, k)| layouts::char_for(map, **k).is_some_and(char::is_alphabetic))
            .map(|(i, _)| i)
            .collect();
        let first = *letters.first()?;
        if advanced.fix_accidental_capslock
            && self.caps
            && keys.iter().all(|k| k.caps)
            && text::fix_inverted_caps(&text).is_some()
        {
            let fixed = keys
                .iter()
                .enumerate()
                .map(|(i, k)| KeyPress {
                    key: k.key,
                    shift: if letters.contains(&i) {
                        i == first
                    } else {
                        k.shift
                    },
                    caps: false,
                })
                .collect();
            return Some((fixed, true));
        }
        if advanced.fix_two_capitals && text::fix_two_initial_caps(&text).is_some() {
            let second = *letters.get(1)?;
            let mut fixed = keys.to_vec();
            fixed[second].shift = fixed[second].caps;
            return Some((fixed, false));
        }
        None
    }

    fn run_job(&mut self, job: Deferred, out: &mut Vec<Event>) {
        match job {
            Deferred::InsertAutoreplace { item, target } => {
                out.extend(self.insert_autoreplace(item, target))
            }
            Deferred::InsertText { text, target } => out.extend(self.insert_text(&text, target)),
            Deferred::Retype {
                to,
                automatic,
                caps_off,
            } => {
                if caps_off {
                    match self.backends.injector.tap(&[], PhysKey::CapsLock) {
                        Ok(()) => self.caps = false,
                        Err(err) => return self.fail("cannot turn Caps Lock off", &err, out),
                    }
                }
                let shown = self.last.as_ref().map(LastWord::shown_in);
                if shown == Some(to) {
                    self.fix_case_in_place(out);
                } else {
                    self.convert_last(to, automatic, out);
                }
            }
            Deferred::Action(action) => self.run_action(action, out),
        }
    }

    /// Retypes the last word in its current layout with corrected case.
    fn fix_case_in_place(&mut self, out: &mut Vec<Event>) {
        let Some(last) = self.last.clone() else {
            return;
        };
        let keys: Vec<KeyPress> = last.all_keys().collect();
        let result = self.backends.injector.backspace(keys.len()).and_then(|()| {
            self.backends
                .injector
                .send(&Self::strokes(keys.iter().copied()))
        });
        match result {
            Ok(()) => {
                tracing::info!("letter case corrected");
                out.push(Event::CaseFixed);
                self.play(Sound::CaseFixed);
            }
            Err(err) => self.fail("case correction failed", &err, out),
        }
    }

    /// «Запятая по двойному нажатию клавиши Пробел».
    fn double_space_comma(&mut self, time: Instant) -> bool {
        let previous = self.last_space.take();
        if !self.config.advanced.double_space_comma || !self.word.is_empty() {
            return false;
        }
        let Some(last) = &self.last else { return false };
        let one_space = matches!(last.separator.as_slice(), [k] if k.key == PhysKey::Space);
        if !one_space
            || !previous.is_some_and(|t| time.saturating_duration_since(t) <= DOUBLE_SPACE)
            || !self.mods.is_empty()
        {
            return false;
        }
        let lang = last.shown_in();
        if self.focus_blocks() {
            return false;
        }
        let Some(mut comma) = layouts::keys_for_text(",", self.detector.keymap(lang)) else {
            return false;
        };
        comma.push(KeyPress::plain(PhysKey::Space));
        let result = self.backends.injector.backspace(2).and_then(|()| {
            self.backends
                .injector
                .send(&Self::strokes(comma.iter().copied()))
        });
        match result {
            Ok(()) => {
                if let Some(last) = &mut self.last {
                    last.separator = comma;
                }
                true
            }
            Err(err) => {
                tracing::warn!("cannot insert a comma: {err}");
                false
            }
        }
    }

    fn is_switch_key(&self, key: PhysKey) -> bool {
        let s = &self.config.switching;
        let direct = s.direct_keys.enabled
            && (s.direct_keys.ru.phys_key() == Some(key)
                || s.direct_keys.en.phys_key() == Some(key));
        let single = !matches!(s.switch_key, SwitchKey::CapsLock | SwitchKey::Space)
            && s.switch_key.phys_key() == Some(key);
        direct || single
    }

    fn switch_key_released(&mut self, key: PhysKey, out: &mut Vec<Event>) {
        let direct = &self.config.switching.direct_keys;
        let target = if direct.enabled && direct.ru.phys_key() == Some(key) {
            Some(Lang::Ru)
        } else if direct.enabled && direct.en.phys_key() == Some(key) {
            Some(Lang::En)
        } else {
            None
        };
        match target {
            Some(lang) => self.choose_layout(lang, out),
            None => self.toggle_layout(out),
        }
    }

    /// Switches to `lang` by the user's request.
    fn choose_layout(&mut self, lang: Lang, out: &mut Vec<Event>) {
        self.reset_all();
        if let Err(err) = self.set_lang(lang) {
            return self.fail("cannot switch the layout", &err, out);
        }
        self.chosen_lang = Some(lang);
        self.block_next_word |= self
            .config
            .troubleshooting
            .no_switch_after
            .manual_layout_change;
        out.push(Event::LayoutChanged(self.layout_info_for(lang)));
        self.play(Sound::LayoutChanged);
    }

    /// «Переключать по»: next layout, or the other of RU/EN with «Только русский/английский».
    fn toggle_layout(&mut self, out: &mut Vec<Event>) {
        let current = self.backends.layouts.current().ok();
        let current_lang = current.and_then(|id| self.lang_of(id));
        let pair_only = self.config.switching.switch_key_only_pair
            || self.layouts.iter().filter(|l| l.lang.is_some()).count() == self.layouts.len();
        if pair_only || self.layouts.len() < 2 {
            let target = current_lang.map_or(Lang::Ru, Lang::other);
            return self.choose_layout(target, out);
        }
        let index = current.and_then(|id| self.layouts.iter().position(|l| l.id == id));
        let next = self.layouts[index.map_or(0, |i| (i + 1) % self.layouts.len())].id;
        self.switch_to(next, out);
    }

    /// Activates a layout the user picked and remembers it as their choice.
    fn switch_to(&mut self, id: LayoutId, out: &mut Vec<Event>) {
        let lang = self.lang_of(id);
        self.reset_all();
        match self.backends.layouts.set(id) {
            Ok(()) => {
                if let Some(lang) = lang {
                    self.own_layout_change = Some((lang, Instant::now()));
                    self.chosen_lang = Some(lang);
                }
                self.block_next_word |= self
                    .config
                    .troubleshooting
                    .no_switch_after
                    .manual_layout_change;
                out.push(Event::LayoutChanged(self.layout_info(id)));
                self.play(Sound::LayoutChanged);
            }
            Err(err) => self.fail("cannot switch the layout", &err, out),
        }
    }

    /// A layout chosen in the tray menu belongs to the program the user typed
    /// in, not to the menu that briefly took the focus.
    pub fn select_layout(&mut self, id: LayoutId) -> Vec<Event> {
        let mut out = Vec::new();
        let activated = match (self.backends.focus.as_ref(), self.insertion_target()) {
            (Some(focus), Some(target)) => focus.activate_target(target),
            _ => Ok(()),
        };
        match activated {
            Ok(()) => self.switch_to(id, &mut out),
            Err(err) => self.fail("cannot return to the input window", &err, &mut out),
        }
        out
    }

    /// Automatic changes are not made in this program's own windows, in
    /// excluded programs and in password fields.
    fn focus_blocks(&mut self) -> bool {
        self.focus_blocks_for(false)
    }

    fn focus_blocks_for(&mut self, manual: bool) -> bool {
        let Some(focus) = &self.backends.focus else {
            return false;
        };
        if let Ok(Some(window)) = focus.active_window() {
            if window.pid == Some(std::process::id()) {
                return true;
            }
            if (!manual || self.config.troubleshooting.ignore_excluded_apps_completely)
                && is_excluded(&self.config.exclusions, &window)
            {
                tracing::debug!("excluded program, no automatic changes");
                return true;
            }
        }
        matches!(focus.is_password_field(), Ok(Some(true)))
    }

    fn foreground_excluded(&mut self) -> bool {
        if let Some((at, excluded)) = self.excluded_cache
            && at.elapsed() <= FOCUS_CACHE
        {
            return excluded;
        }
        let excluded = self
            .backends
            .focus
            .as_ref()
            .and_then(|f| f.active_window().ok().flatten())
            .is_some_and(|w| is_excluded(&self.config.exclusions, &w));
        self.excluded_cache = Some((Instant::now(), excluded));
        excluded
    }

    fn run_deferred(&mut self, out: &mut Vec<Event>) {
        for job in std::mem::take(&mut self.deferred) {
            self.run_job(job, out);
        }
    }

    fn strokes(keys: impl Iterator<Item = KeyPress>) -> Vec<KeyStroke> {
        let mut strokes = Vec::new();
        for press in keys {
            if press.shift {
                strokes.push(KeyStroke::press(PhysKey::ShiftLeft));
            }
            strokes.push(KeyStroke::press(press.key));
            strokes.push(KeyStroke::release(press.key));
            if press.shift {
                strokes.push(KeyStroke::release(PhysKey::ShiftLeft));
            }
        }
        strokes
    }

    fn pause(&self) {
        let ms = self.config.switching.layout_switch_delay_ms;
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(u64::from(ms)));
        }
    }

    /// Erases `erase` characters, switches to `to` and replays `keys`.
    fn retype(&mut self, erase: usize, to: Lang, keys: &[KeyPress]) -> Result<(), PlatformError> {
        let target = self.operation_target;
        let epoch = self.operation_epoch;
        if (target.is_some() && !self.target_matches(target)) || !self.epoch_matches(epoch) {
            return Err(PlatformError::Other(
                "input target changed before correction".into(),
            ));
        }
        self.set_lang(to)?;
        self.pause();
        if (target.is_some() && !self.target_matches(target)) || !self.epoch_matches(epoch) {
            return Err(PlatformError::Other(
                "input target changed during correction".into(),
            ));
        }
        self.backends.injector.backspace(erase)?;
        self.backends
            .injector
            .send(&Self::strokes(keys.iter().copied()))
    }

    fn convert_last(&mut self, to: Lang, automatic: bool, out: &mut Vec<Event>) {
        let Some(last) = self.last.clone() else {
            return;
        };
        let from = last.shown_in();
        if from == to {
            return;
        }
        let keys: Vec<KeyPress> = last.all_keys().collect();
        match self.retype(keys.len(), to, &keys) {
            Ok(()) => {
                if let Some(l) = &mut self.last {
                    l.conversion = Some(Conversion {
                        from,
                        to,
                        automatic,
                    });
                }
                let kind = if automatic {
                    ConversionKind::Automatic
                } else {
                    ConversionKind::Manual
                };
                tracing::info!(%from, %to, ?kind, "word converted");
                if self.config.rules_options.improve_switching && !automatic {
                    self.previous_word = (!last.separator.is_empty()
                        && last.separator.iter().all(|k| k.key == PhysKey::Space))
                    .then_some(to);
                }
                out.push(Event::Converted { from, to, kind });
                self.play(if automatic {
                    Sound::Autoswitch
                } else {
                    Sound::ManualConvert
                });
            }
            Err(err) => self.fail("conversion failed", &err, out),
        }
    }

    fn fail(&mut self, what: &str, err: &PlatformError, out: &mut Vec<Event>) {
        tracing::error!("{what}: {err}");
        out.push(Event::Error(format!("{what}: {err}")));
        self.play(Sound::Error);
        self.reset_all();
    }

    fn matching_hotkey(&self, key: PhysKey) -> Option<HotkeyAction> {
        let autoswitch_off = !self.config.general.autoswitch;
        self.config
            .hotkeys
            .iter()
            .filter(|(action, _)| {
                !(autoswitch_off
                    && self.config.general.hotkeys_off_when_autoswitch_off
                    && *action != HotkeyAction::ToggleAutoswitch)
            })
            .find(|(_, binding)| binding.0.is_some_and(|h| h.matches(key, self.mods)))
            .map(|(action, _)| action)
    }

    fn run_action(&mut self, action: HotkeyAction, out: &mut Vec<Event>) {
        tracing::info!(?action, "hotkey");
        match action {
            HotkeyAction::CancelOrConvertLastWord => self.cancel_or_convert(out),
            HotkeyAction::ConvertSelectionLayout => self.transform_selection(TextOp::Layout, out),
            HotkeyAction::InvertSelectionCase => self.transform_selection(TextOp::InvertCase, out),
            HotkeyAction::TransliterateSelection => {
                self.transform_selection(TextOp::Transliterate, out)
            }
            HotkeyAction::NumberToWords => self.transform_selection(TextOp::NumberToWords, out),
            HotkeyAction::ConvertClipboardLayout => self.transform_clipboard(TextOp::Layout, out),
            HotkeyAction::TransliterateClipboard => {
                self.transform_clipboard(TextOp::Transliterate, out)
            }
            HotkeyAction::PastePlain => self.paste_plain(out),
            HotkeyAction::ToggleAutoswitch => {
                let on = !self.config.general.autoswitch;
                self.set_autoswitch(on);
                out.push(Event::AutoswitchChanged(on));
            }
            HotkeyAction::ToggleSounds => {
                let on = !self.config.sounds.enabled;
                self.set_sounds(on);
                out.push(Event::SoundsChanged(on));
            }
            HotkeyAction::OpenSettings => out.push(Event::Ui(UiRequest::OpenSettings)),
            HotkeyAction::OpenAutoreplaceSettings => {
                out.push(Event::Ui(UiRequest::OpenAutoreplaceSettings))
            }
            HotkeyAction::ToggleAutoreplaceList => {
                out.extend(self.autoreplace_list(true));
            }
            HotkeyAction::ShowAutoreplaceMenu => {
                if self.config.autoreplace.trigger != AutoReplaceTrigger::Hotkey
                    || !self.expand_current(false, false, true, out)
                {
                    out.extend(self.autoreplace_list(false));
                }
            }
            HotkeyAction::AddSelectionToAutoreplace => {
                self.add_selection_to_autoreplace(out);
            }
            HotkeyAction::ShowClipboardHistory => out.extend(self.clipboard_history()),
            HotkeyAction::SpellcheckClipboard => {
                out.extend(self.spellcheck_selection_or_clipboard());
            }
            HotkeyAction::ToggleMaximizeWindow => {
                out.push(Event::Ui(UiRequest::ToggleMaximizeWindow))
            }
            HotkeyAction::MinimizeWindow => out.push(Event::Ui(UiRequest::MinimizeWindow)),
        }
    }

    /// «Отменить конвертацию раскладки»: undoes an automatic conversion of the
    /// last word, otherwise converts the current or last word manually.
    fn cancel_or_convert(&mut self, out: &mut Vec<Event>) {
        if self.word.is_empty()
            && let Some(undo) = self.spelling_undo.take()
        {
            return self.undo_spelling(undo, out);
        }
        if !self.word.is_empty() {
            let from = match self.word_lang.or_else(|| self.current_lang()) {
                Some(lang) => lang,
                None => return,
            };
            let to = from.other();
            let keys = self.word.clone();
            match self.retype(keys.len(), to, &keys) {
                Ok(()) => {
                    tracing::info!(%from, %to, "word being typed converted by hotkey");
                    self.word_lang = Some(to);
                    self.word_blocked = true;
                    self.context = Some(to);
                    self.previous_word = None;
                    out.push(Event::Converted {
                        from,
                        to,
                        kind: ConversionKind::Manual,
                    });
                    self.play(Sound::ManualConvert);
                }
                Err(err) => self.fail("conversion failed", &err, out),
            }
            return;
        }
        let Some(last) = self.last.clone() else {
            return;
        };
        match last.conversion {
            Some(Conversion {
                from,
                automatic: true,
                ..
            }) => {
                let keys: Vec<KeyPress> = last.all_keys().collect();
                if let Err(err) = self.retype(keys.len(), from, &keys) {
                    self.fail("undo failed", &err, out);
                    return;
                }
                if let Some(l) = &mut self.last {
                    l.conversion = None;
                }
                tracing::info!(from = %last.shown_in(), to = %from, "conversion undone");
                self.context = Some(from);
                if self.config.rules_options.improve_switching {
                    self.previous_word = (!last.separator.is_empty()
                        && last.separator.iter().all(|k| k.key == PhysKey::Space))
                    .then_some(from);
                }
                out.push(Event::Converted {
                    from: last.shown_in(),
                    to: from,
                    kind: ConversionKind::Undo,
                });
                self.play(Sound::Cancel);
                self.count_cancel(&last, out);
            }
            _ => {
                let to = last.shown_in().other();
                self.convert_last(to, false, out);
            }
        }
    }

    fn count_cancel(&mut self, last: &LastWord, out: &mut Vec<Event>) {
        let threshold = self.config.rules_options.suggest_rule_after_cancels;
        if threshold == 0 {
            return;
        }
        let word: String = layouts::render(&last.keys, self.detector.keymap(last.typed_in))
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if word.is_empty() {
            return;
        }
        let count = self.cancels.entry(word.clone()).or_insert(0);
        *count += 1;
        if *count >= threshold {
            self.cancels.remove(&word);
            out.push(Event::SuggestRule(suggest_rule(
                &word,
                okbs_core::config::RuleAction::Stay,
            )));
        }
    }

    fn play(&self, sound: Sound) {
        let sounds = &self.config.sounds;
        if !sounds.enabled {
            return;
        }
        let Some(player) = &self.backends.sound else {
            return;
        };
        let events = &sounds.events;
        let setting: &SoundEvent = match sound {
            Sound::Autoswitch => &events.autoswitch,
            Sound::ManualConvert => &events.manual_convert,
            Sound::LayoutChanged => &events.layout_changed,
            Sound::Cancel => &events.cancel,
            Sound::Suspicious => &events.suspicious,
            Sound::Autoreplace => &events.autoreplace,
            Sound::CaseFixed => &events.case_fixed,
            Sound::ClipboardConvert => &events.clipboard_convert,
            Sound::Error => &events.error,
            Sound::SpellingError => &events.spelling_error,
            Sound::SpellingCorrected => &events.spelling_corrected,
        };
        if !setting.enabled {
            return;
        }
        let result = match sounds.mode {
            okbs_core::config::SoundMode::Beep => player.beep(),
            okbs_core::config::SoundMode::File if !setting.file.is_empty() => {
                player.play_file(std::path::Path::new(&setting.file))
            }
            okbs_core::config::SoundMode::File => player.play_wav(sounds::wav(sound)),
        };
        if let Err(err) = result {
            tracing::debug!("cannot play sound: {err}");
        }
    }

    /// Copies the selection through the clipboard, returning the previous clipboard text.
    fn copy_selection(&mut self) -> Result<Option<(String, Option<String>)>, PlatformError> {
        let Some(clipboard) = self.backends.clipboard.as_mut() else {
            return Err(PlatformError::Unsupported("clipboard"));
        };
        let saved = clipboard.text()?;
        clipboard.set_text(CLIPBOARD_MARKER)?;
        if let Err(err) = self
            .backends
            .injector
            .tap(&[PhysKey::ControlLeft], PhysKey::KeyC)
        {
            self.restore_clipboard(saved, CLIPBOARD_MARKER);
            return Err(err);
        }
        let started = Instant::now();
        loop {
            let Some(clipboard) = self.backends.clipboard.as_mut() else {
                return Err(PlatformError::Unsupported("clipboard"));
            };
            match clipboard.text() {
                Ok(Some(text)) if text != CLIPBOARD_MARKER => return Ok(Some((text, saved))),
                _ if started.elapsed() >= self.timing.copy_timeout => {
                    self.restore_clipboard(saved, CLIPBOARD_MARKER);
                    return Ok(None);
                }
                _ => std::thread::sleep(self.timing.poll),
            }
        }
    }

    fn transform_selection(&mut self, op: TextOp, out: &mut Vec<Event>) {
        self.reset_all();
        let copied = match self.copy_selection() {
            Ok(Some(copied)) => copied,
            Ok(None) => {
                tracing::debug!("nothing selected");
                return;
            }
            Err(err) => return self.fail("cannot copy the selection", &err, out),
        };
        let (text, saved) = copied;
        let Some(result) = op.apply(&text) else {
            self.restore_clipboard(saved, &text);
            self.play(Sound::Error);
            return;
        };
        let pasted = (|| -> Result<(), PlatformError> {
            let clipboard = self
                .backends
                .clipboard
                .as_mut()
                .ok_or(PlatformError::Unsupported("clipboard"))?;
            clipboard.set_text(&result)?;
            self.backends
                .injector
                .tap(&[PhysKey::ControlLeft], PhysKey::KeyV)?;
            Ok(())
        })();
        if let Err(err) = pasted {
            // Setting the result may have failed before replacing the copied text.
            self.restore_clipboard(saved.clone(), &result);
            self.restore_clipboard(saved, &text);
            return self.fail("cannot paste the result", &err, out);
        }
        if op == TextOp::Layout
            && let Some(lang) = layouts::guess_text_lang(&result)
            && let Err(err) = self.set_lang(lang)
        {
            tracing::debug!("cannot switch layout after selection conversion: {err}");
        }
        std::thread::sleep(self.timing.paste_settle);
        self.restore_clipboard(saved, &result);
        out.push(Event::SelectionConverted(op));
        self.play(Sound::ManualConvert);
    }

    /// Copy before opening our window, while the original application has
    /// focus. The UI only edits a draft; the shortcut never saves a rule.
    fn add_selection_to_autoreplace(&mut self, out: &mut Vec<Event>) {
        self.reset_all();
        if self
            .backends
            .focus
            .as_ref()
            .is_some_and(|focus| matches!(focus.is_password_field(), Ok(Some(true))))
        {
            return;
        }
        match self.copy_selection() {
            Ok(Some((text, saved))) => {
                self.restore_clipboard(saved, &text);
                if !text.is_empty() {
                    out.push(Event::AutoreplaceSelection(text));
                }
            }
            Ok(None) => {}
            Err(err) => self.fail("cannot copy selection for autoreplace", &err, out),
        }
    }

    fn restore_clipboard(&mut self, saved: Option<String>, temporary: &str) {
        let Some(clipboard) = self.backends.clipboard.as_mut() else {
            return;
        };
        if !matches!(clipboard.text(), Ok(Some(text)) if text == temporary) {
            return;
        }
        let result = match saved {
            Some(saved) => clipboard.set_text(&saved),
            None => clipboard.clear(),
        };
        if let Err(err) = result {
            tracing::debug!("cannot restore the clipboard: {err}");
        }
    }

    fn transform_clipboard(&mut self, op: TextOp, out: &mut Vec<Event>) {
        let Some(clipboard) = self.backends.clipboard.as_mut() else {
            return;
        };
        let text = match clipboard.text() {
            Ok(Some(text)) => text,
            Ok(None) => return,
            Err(err) => return self.fail("cannot read the clipboard", &err, out),
        };
        let Some(result) = op.apply(&text) else {
            return;
        };
        if let Err(err) = clipboard.set_text(&result) {
            return self.fail("cannot write the clipboard", &err, out);
        }
        out.push(Event::ClipboardConverted { op, result });
        self.play(Sound::ClipboardConvert);
    }

    /// Runs a clipboard operation requested from the tray menu.
    pub fn clipboard_op(&mut self, op: TextOp) -> Vec<Event> {
        let mut out = Vec::new();
        self.transform_clipboard(op, &mut out);
        out
    }

    /// Snapshot for the spelling worker, which must not block keyboard input.
    pub fn spellcheck_clipboard(&mut self) -> Vec<Event> {
        if !self.config.spellcheck.check_on_command {
            return Vec::new();
        }
        let Some(clipboard) = self.backends.clipboard.as_mut() else {
            return Vec::new();
        };
        match clipboard.text() {
            Ok(Some(text)) => vec![Event::CheckSpelling {
                text,
                settings: self.config.spellcheck.clone(),
                interactive: false,
                target: None,
            }],
            Ok(None) => Vec::new(),
            Err(err) => {
                let mut out = Vec::new();
                self.fail("cannot read the clipboard", &err, &mut out);
                out
            }
        }
    }

    /// Checks selected text first when the user chose that mode. The clipboard
    /// is restored before the background checker starts, so a result can never
    /// overwrite a later copy operation.
    pub fn spellcheck_selection_or_clipboard(&mut self) -> Vec<Event> {
        if !self.config.spellcheck.check_on_command {
            return Vec::new();
        }
        if self.config.spellcheck.prefer_selection {
            match self.copy_selection() {
                Ok(Some((text, saved))) if !text.trim().is_empty() => {
                    self.restore_clipboard(saved, &text);
                    return vec![Event::CheckSpelling {
                        text,
                        settings: self.config.spellcheck.clone(),
                        interactive: false,
                        target: None,
                    }];
                }
                Ok(Some((text, saved))) => self.restore_clipboard(saved, &text),
                Ok(None) => {}
                Err(err) => {
                    tracing::debug!("cannot copy selection for spellcheck: {err}");
                }
            }
        }
        self.spellcheck_clipboard()
    }

    /// Background spelling results must not overwrite a newer copy operation.
    pub fn correct_clipboard(&mut self, original: &str, corrected: &str) -> Vec<Event> {
        let mut out = Vec::new();
        let Some(clipboard) = self.backends.clipboard.as_mut() else {
            return out;
        };
        let result = clipboard.text().and_then(|text| {
            if text.as_deref() == Some(original) && original != corrected {
                clipboard.set_text(corrected)?;
            }
            Ok(())
        });
        if let Err(err) = result {
            self.fail("cannot write spelling corrections", &err, &mut out);
        }
        out
    }

    /// Replaces only the still-current last word. The result of a slow
    /// spelling query must never alter a word after the user continued typing,
    /// moved to another control, or pressed Enter.
    pub fn correct_typed_spelling(
        &mut self,
        target: InputTarget,
        original: &str,
        corrected: &str,
    ) -> Vec<Event> {
        let mut out = Vec::new();
        tracing::debug!(target: "okbs_spelling", ?target,
            current = ?self.backends.focus.as_ref().and_then(|f| f.input_target().ok().flatten()),
            pending = self.pending_spelling.is_some(), original_chars = original.chars().count(),
            corrected_chars = corrected.chars().count(), modifiers_held = !self.mods.is_empty(),
            "replacement command received");
        if corrected.is_empty() || corrected == original {
            tracing::debug!(target: "okbs_spelling", empty = corrected.is_empty(),
                unchanged = corrected == original, "replacement rejected: no changed suggestion");
            return out;
        }
        let Some(pending) = self.pending_spelling.take() else {
            tracing::debug!(target: "okbs_spelling", "replacement rejected: snapshot missing or invalidated");
            return out;
        };
        tracing::debug!(target: "okbs_spelling",
            age_ms = pending.captured_at.elapsed().as_millis() as u64,
            snapshot_target = ?pending.last.target, separator_count = pending.last.separator.len(),
            "validating replacement snapshot");
        let last = pending.last;
        if pending.original != original
            || last.target != Some(target)
            || !matches!(last.separator.as_slice(), [separator] if separator.key == PhysKey::Space)
            || layouts::render(&last.keys, self.detector.keymap(last.shown_in())) != original
        {
            tracing::debug!(target: "okbs_spelling",
                original_matches = pending.original == original,
                target_matches = last.target == Some(target),
                space_terminated = matches!(last.separator.as_slice(), [s] if s.key == PhysKey::Space),
                rendered_matches = layouts::render(&last.keys, self.detector.keymap(last.shown_in())) == original,
                "replacement rejected: snapshot mismatch");
            return out;
        }
        let result = (|| {
            let focus = self
                .backends
                .focus
                .as_ref()
                .ok_or(PlatformError::Unsupported("spelling correction target"))?;
            tracing::debug!(target: "okbs_spelling", ?target, "restoring editor focus");
            focus.activate_target(target)?;
            tracing::debug!(target: "okbs_spelling", ?target, "editor focus restored");
            if matches!(focus.is_password_field(), Ok(Some(true))) {
                return Err(PlatformError::Other(
                    "spelling correction is disabled in this control".into(),
                ));
            }
            self.excluded_cache = None;
            if !self.target_matches(Some(target)) || self.foreground_excluded() {
                return Err(PlatformError::Other(
                    "spelling correction target changed".into(),
                ));
            }
            self.operation_target = Some(target);
            let replacement = format!("{corrected} ");
            tracing::debug!(target: "okbs_spelling", erase = last.keys.len() + 1,
                paste_chars = replacement.chars().count(), "starting spelling paste");
            self.paste_replacement(&replacement, last.keys.len() + 1, 0)
        })();
        self.operation_target = None;
        if let Err(err) = result {
            tracing::debug!(target: "okbs_spelling", %err, "replacement failed");
            self.fail("spelling correction failed", &err, &mut out);
            return out;
        }
        self.reset_all();
        tracing::debug!(target: "okbs_spelling", ?target,
            "replacement input sent successfully; editor text not read back");
        self.spelling_undo = Some(SpellingUndo {
            target,
            original: original.to_string(),
            corrected: corrected.to_string(),
        });
        out.push(Event::SpellingCorrected);
        out
    }

    /// Puts back the word a spelling correction replaced; the word is not
    /// checked again while the program runs.
    fn undo_spelling(&mut self, undo: SpellingUndo, out: &mut Vec<Event>) {
        let result = if self.target_matches(Some(undo.target)) {
            self.operation_target = Some(undo.target);
            self.paste_replacement(
                &format!("{} ", undo.original),
                undo.corrected.chars().count() + 1,
                0,
            )
        } else {
            Err(PlatformError::Other("spelling undo target changed".into()))
        };
        self.operation_target = None;
        match result {
            Ok(()) => {
                tracing::info!("spelling correction undone");
                self.decline_spelling(&undo.original);
                self.play(Sound::Cancel);
            }
            Err(err) => self.fail("cannot undo the spelling correction", &err, out),
        }
    }

    /// The user kept `word` as typed: do not offer or apply corrections for it
    /// until the program restarts.
    pub fn decline_spelling(&mut self, word: &str) {
        self.declined_spelling.insert(word.to_lowercase());
    }

    fn paste_plain(&mut self, out: &mut Vec<Event>) {
        self.reset_all();
        let Some(clipboard) = self.backends.clipboard.as_mut() else {
            return;
        };
        let text = match clipboard.text() {
            Ok(Some(text)) => text,
            Ok(None) => return,
            Err(err) => return self.fail("cannot read the clipboard", &err, out),
        };
        let result = clipboard.set_text(&text).and_then(|()| {
            self.backends
                .injector
                .tap(&[PhysKey::ControlLeft], PhysKey::KeyV)
        });
        if let Err(err) = result {
            self.fail("cannot paste plain text", &err, out);
        }
    }
}
