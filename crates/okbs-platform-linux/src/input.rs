//! Ordered evdev forwarding. A grabbed keyboard is released by Device::drop;
//! no device is grabbed until the virtual output and engine channel are ready.
use crate::desktop::{LinuxDesktop, error};
use evdev::{
    AttributeSet, Device, EventType, InputEvent as EvEvent, KeyCode, uinput::VirtualDevice,
};
use okbs_core::config::{Config, SwitchKey};
use okbs_core::{Lang, ModState, PhysKey};
use okbs_platform::autoreplace_gate::AutoReplaceGate;
use okbs_platform::{Injector, InputEvent, KeyStroke, KeyboardSource, Result, StopGuard};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

pub const VIRTUAL_NAME: &str = "Own Keyboard Switch virtual keyboard";
#[derive(Debug, Default)]
struct PhysicalKeys(HashMap<PathBuf, HashSet<u16>>);
impl PhysicalKeys {
    fn event(&mut self, path: &PathBuf, code: u16, value: i32) -> bool {
        let held = self.0.values().any(|keys| keys.contains(&code));
        if value == 0 {
            if let Some(keys) = self.0.get_mut(path) {
                keys.remove(&code);
            }
            !self.0.values().any(|keys| keys.contains(&code)) && held
        } else {
            self.0.entry(path.clone()).or_default().insert(code);
            !held || value == 2
        }
    }
    fn remove(&mut self, path: &PathBuf) -> Vec<u16> {
        self.0
            .remove(path)
            .unwrap_or_default()
            .into_iter()
            .filter(|code| !self.0.values().any(|keys| keys.contains(code)))
            .collect()
    }
}
#[derive(Debug)]
struct Output {
    device: VirtualDevice,
    down: HashSet<u16>,
}
impl Output {
    fn emit(&mut self, code: u16, value: i32) -> Result<()> {
        self.device
            .emit(&[EvEvent::new(EventType::KEY.0, code, value)])
            .map_err(error)?;
        if value == 0 {
            self.down.remove(&code);
        } else {
            self.down.insert(code);
        }
        Ok(())
    }
    fn release(&mut self) {
        for code in self.down.clone() {
            let _ = self.emit(code, 0);
        }
    }
}
struct ReleaseOutput {
    output: Arc<Mutex<Output>>,
    gate: Arc<AutoReplaceGate>,
    alive: Arc<AtomicBool>,
}
impl Drop for ReleaseOutput {
    fn drop(&mut self) {
        self.gate.fail_open();
        if let Ok(mut output) = self.output.lock() {
            output.release();
        }
        self.alive.store(false, Ordering::Release);
    }
}
#[derive(Debug, Clone)]
pub struct LinuxInjector {
    output: Arc<Mutex<Output>>,
    delay: Arc<AtomicU64>,
    desktop: LinuxDesktop,
}
impl Injector for LinuxInjector {
    fn send(&mut self, strokes: &[KeyStroke]) -> Result<()> {
        let snapshot = self.desktop.cached();
        if snapshot.locked || snapshot.session_inactive {
            return Err(error("no active unlocked graphical session"));
        }
        // Linux terminal copy/paste uses Ctrl+Shift+C/V. Translate only the
        // engine's complete clipboard chord, never replayed physical keys.
        let terminal = okbs_platform::FocusInfo::is_terminal(&self.desktop).unwrap_or(false);
        if let [down, key_down, key_up, up] = strokes
            && terminal
            && down.pressed
            && !up.pressed
            && down.key == up.key
            && matches!(down.key, PhysKey::ControlLeft | PhysKey::ControlRight)
            && key_down.pressed
            && !key_up.pressed
            && key_down.key == key_up.key
            && matches!(key_down.key, PhysKey::KeyC | PhysKey::KeyV)
        {
            let expanded = [
                *down,
                KeyStroke::press(PhysKey::ShiftLeft),
                *key_down,
                *key_up,
                KeyStroke::release(PhysKey::ShiftLeft),
                *up,
            ];
            return self.emit(&expanded);
        }
        self.emit(strokes)
    }
    fn send_unmapped(&mut self, code: u32, pressed: bool) -> Result<()> {
        let snapshot = self.desktop.cached();
        if snapshot.locked || snapshot.session_inactive {
            return Err(error("no active unlocked graphical session"));
        }
        self.output
            .lock()
            .map_err(error)?
            .emit(u16::try_from(code).map_err(error)?, i32::from(pressed))
    }
}
impl LinuxInjector {
    fn emit(&mut self, strokes: &[KeyStroke]) -> Result<()> {
        for stroke in strokes {
            self.output
                .lock()
                .map_err(error)?
                .emit(stroke.key.evdev_code(), i32::from(stroke.pressed))?;
            let delay = self.delay.load(Ordering::Relaxed);
            if delay != 0 {
                std::thread::sleep(Duration::from_millis(delay));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct InputFilter {
    config: Arc<Mutex<Config>>,
    pub gate: Arc<AutoReplaceGate>,
    delay: Arc<AtomicU64>,
    desktop: Option<LinuxDesktop>,
}
impl InputFilter {
    pub fn new(config: &Config) -> Self {
        Self {
            config: Arc::new(Mutex::new(config.clone())),
            gate: Arc::new(AutoReplaceGate::new(config)),
            delay: Arc::new(AtomicU64::new(u64::from(
                config.switching.inject_key_delay_ms,
            ))),
            desktop: None,
        }
    }
    pub fn configure(&self, config: &Config) {
        if let Some(desktop) = &self.desktop {
            desktop.enable_menu_access(config.advanced.fix_layout_in_menus);
        }
        self.delay.store(
            u64::from(config.switching.inject_key_delay_ms),
            Ordering::Relaxed,
        );
        self.gate.configure(config);
        if let Ok(mut current) = self.config.lock() {
            *current = config.clone();
        }
    }
}
#[derive(Debug)]
pub struct LinuxSource {
    output: Arc<Mutex<Output>>,
    pub filter: InputFilter,
    desktop: LinuxDesktop,
    alive: Arc<AtomicBool>,
}
impl LinuxSource {
    /// Device nodes for the owned output keyboard, useful for diagnostics and
    /// isolated acceptance drivers. Never used to capture this device again.
    pub fn virtual_nodes(&mut self) -> Result<Vec<PathBuf>> {
        self.output
            .lock()
            .map_err(error)?
            .device
            .enumerate_dev_nodes_blocking()
            .map_err(error)?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(error)
    }
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub fn new(config: &Config, desktop: LinuxDesktop) -> Result<(Self, LinuxInjector)> {
        let mut keys = AttributeSet::new();
        for code in 1..=767 {
            keys.insert(KeyCode(code));
        }
        let virtual_device = VirtualDevice::builder()
            .map_err(error)?
            .name(VIRTUAL_NAME)
            .with_keys(&keys)
            .map_err(error)?
            .build()
            .map_err(error)?;
        let output = Arc::new(Mutex::new(Output {
            device: virtual_device,
            down: HashSet::new(),
        }));
        let mut filter = InputFilter::new(config);
        filter.desktop = Some(desktop.clone());
        desktop.enable_menu_access(config.advanced.fix_layout_in_menus);
        let injector = LinuxInjector {
            output: output.clone(),
            delay: filter.delay.clone(),
            desktop: desktop.clone(),
        };
        Ok((
            Self {
                output,
                filter,
                desktop,
                alive: Arc::new(AtomicBool::new(false)),
            },
            injector,
        ))
    }
}
#[derive(Debug)]
struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    output: Arc<Mutex<Output>>,
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Ok(mut output) = self.output.lock() {
            output.release();
        }
    }
}
impl StopGuard for Capture {
    fn stop(self: Box<Self>) {
        drop(self);
    }
}
impl KeyboardSource for LinuxSource {
    fn caps_lock_on(&self) -> Option<bool> {
        if let Some(caps) = self.desktop.cached().caps {
            return Some(caps);
        }
        let scan = crate::access::scan_input_devices();
        scan.keyboards.iter().find_map(|keyboard| {
            let device = Device::open(&keyboard.path).ok()?;
            let leds = device.get_led_state().ok()?;
            Some(leds.contains(evdev::LedCode::LED_CAPSL))
        })
    }
    fn start(&mut self, sink: crossbeam_channel::Sender<InputEvent>) -> Result<Box<dyn StopGuard>> {
        let config = self.filter.config.lock().map_err(error)?.clone();
        let mut devices = discover(&config.linux.devices)?;
        let deadline = Instant::now() + Duration::from_millis(500);
        while !devices.values().any(|device| device.is_grabbed()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            devices.extend(discover_missing(&config.linux.devices, &devices)?);
        }
        if !devices.values().any(|d| d.is_grabbed()) {
            return Err(error(
                "no accessible physical keyboard; configure input access first",
            ));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let output = self.output.clone();
        let filter = self.filter.clone();
        let desktop = self.desktop.clone();
        let alive = self.alive.clone();
        alive.store(true, Ordering::Release);
        let thread = std::thread::spawn(move || {
            let _release = ReleaseOutput {
                output: output.clone(),
                gate: filter.gate.clone(),
                alive,
            };
            let mut mods = ModState::default();
            let mut consumed = HashSet::new();
            let mut physical_keys = PhysicalKeys::default();
            let mut last_scan = Instant::now();
            let mut last_progress = Instant::now();
            let mut previous_pending = 0;
            let mut paused = false;
            while !signal.load(Ordering::Acquire) {
                let mut gone = Vec::new();
                let session = desktop.cached();
                let inactive = session.session_inactive || session.locked;
                if inactive && !paused {
                    filter.gate.fail_open();
                    if let Ok(mut output) = output.lock() {
                        output.release();
                    }
                    for device in devices.values_mut() {
                        if device.is_grabbed() {
                            let _ = device.ungrab();
                        }
                    }
                    physical_keys = PhysicalKeys::default();
                    desktop.reset_menu_keys();
                    mods = ModState::default();
                    consumed.clear();
                    let _ = sink.send(InputEvent::UnknownKey {
                        code: 0,
                        pressed: true,
                        time: Instant::now(),
                    });
                    paused = true;
                } else if !inactive && paused {
                    // Never acquire a keyboard in the middle of a held chord.
                    let released =
                        devices
                            .values()
                            .filter(|device| is_keyboard(device))
                            .all(|device| {
                                device
                                    .get_key_state()
                                    .is_ok_and(|keys| keys.iter().next().is_none())
                            });
                    if released {
                        for device in devices.values_mut().filter(|device| is_keyboard(device)) {
                            if device.grab().is_err() {
                                return;
                            }
                        }
                        paused = false;
                    }
                }
                let pending = filter.gate.pending_events();
                if pending == 0 || pending < previous_pending {
                    last_progress = Instant::now();
                }
                previous_pending = pending;
                if pending != 0 && last_progress.elapsed() > Duration::from_secs(3) {
                    tracing::error!(
                        "input engine stopped acknowledging events; releasing physical keyboards"
                    );
                    return;
                }
                if !paused && last_scan.elapsed() > Duration::from_secs(1) {
                    last_scan = Instant::now();
                    if let Ok(config) = filter.config.lock() {
                        if !config.linux.devices.is_empty() {
                            let selected: HashSet<_> = config
                                .linux
                                .devices
                                .iter()
                                .filter_map(|path| std::fs::canonicalize(path).ok())
                                .collect();
                            gone.extend(
                                devices
                                    .keys()
                                    .filter(|path| {
                                        std::fs::canonicalize(path)
                                            .is_ok_and(|path| !selected.contains(&path))
                                    })
                                    .cloned(),
                            );
                        }
                        if let Ok(new_devices) = discover_missing(&config.linux.devices, &devices) {
                            devices.extend(new_devices);
                        }
                    }
                }
                for (path, device) in &mut devices {
                    if gone.contains(path) {
                        continue;
                    }
                    let events: Vec<_> = match device.fetch_events() {
                        Ok(events) => events.collect(),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(_) => {
                            gone.push(path.clone());
                            continue;
                        }
                    };
                    for physical in events {
                        if physical.event_type() != EventType::KEY {
                            continue;
                        }
                        let time = Instant::now();
                        let code = physical.code();
                        let value = physical.value();
                        let key = PhysKey::from_evdev(code);
                        if !device.is_grabbed() {
                            if is_keyboard(device) {
                                continue;
                            }
                            if value == 1 {
                                let event = InputEvent::MouseButton {
                                    time,
                                    in_own_window: desktop.cached().pid == std::process::id(),
                                };
                                filter.gate.capture(event, None, None);
                                if sink.send(event).is_err() {
                                    return;
                                }
                            }
                            continue;
                        }
                        if !physical_keys.event(path, code, value) {
                            continue;
                        }
                        if let Some(key) = key {
                            desktop.note_menu_key(key, value != 0);
                        }
                        let mut event = match key {
                            Some(key) => InputEvent::Key {
                                key,
                                pressed: value != 0,
                                repeat: value == 2,
                                injected: false,
                                time,
                            },
                            None => InputEvent::UnknownKey {
                                code: u32::from(code),
                                pressed: value != 0,
                                time,
                            },
                        };
                        if filter.gate.discard(event) {
                            // The GUI received the original down event. Even
                            // when repeats are suppressed, its compositor must
                            // receive the release or the virtual key stays held.
                            if value == 0
                                && let Ok(mut output) = output.lock()
                            {
                                let _ = output.emit(code, value);
                            }
                            continue;
                        }
                        let current = desktop.cached();
                        let target = desktop.target();
                        if current.pid == std::process::id() && !current.locked {
                            if let Some(key) = key
                                && key.is_modifier()
                            {
                                mods.set(key, value != 0);
                            }
                            if value == 0 {
                                consumed.remove(&code);
                            }
                            if let Ok(mut output) = output.lock() {
                                let _ = output.emit(code, value);
                            }
                            filter.gate.observe_own_window(event, None);
                            if sink.send(event).is_err() {
                                return;
                            }
                            continue;
                        }
                        // Lock screens and consoles are outside the graphical
                        // input target. Never collect or change their text.
                        if current.locked || target.is_none() {
                            filter.gate.fail_open();
                            if let Ok(mut output) = output.lock() {
                                let _ = output.emit(code, value);
                            }
                            // Modifier state and releases contain no text and
                            // must stay synchronized across a lock/unlock.
                            if key.is_some_and(PhysKey::is_modifier) || value == 0 {
                                if let Some(key) = key
                                    && key.is_modifier()
                                {
                                    mods.set(key, value != 0);
                                }
                                filter.gate.capture(event, None, None);
                                if sink.send(event).is_err() {
                                    return;
                                }
                            }
                            if value == 0 {
                                consumed.remove(&code);
                            }
                            continue;
                        }
                        let lang = current
                            .sources
                            .get(current.group as usize)
                            .and_then(|source| match source.code.split('+').next() {
                                Some("ru") => Some(Lang::Ru),
                                Some("us" | "gb") => Some(Lang::En),
                                _ => None,
                            });
                        let (swallow_caps, hotkey) = {
                            let config = match filter.config.lock() {
                                Ok(config) => config,
                                Err(_) => return,
                            };
                            let swallow_caps = key == Some(PhysKey::CapsLock)
                                && (config.advanced.disable_capslock
                                    || config.switching.switch_key == SwitchKey::CapsLock);
                            let hotkey = filter.gate.hotkeys_enabled()
                                && !filter.gate.is_capturing()
                                && key.is_some_and(|key| {
                                    value == 1
                                        && config.hotkeys.iter().any(|(_, binding)| {
                                            binding
                                                .0
                                                .is_some_and(|binding| binding.matches(key, mods))
                                        })
                                });
                            (swallow_caps, hotkey)
                        };
                        if hotkey {
                            consumed.insert(code);
                        }
                        let swallowed = swallow_caps || consumed.contains(&code);
                        if value == 0 {
                            consumed.remove(&code);
                        }
                        if let Some(key) = key
                            && key.is_modifier()
                        {
                            mods.set(key, value != 0);
                        }
                        let captured = !swallowed && filter.gate.capture(event, lang, target);
                        if captured {
                            let epoch = filter.gate.epoch();
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
                        } else if !swallowed
                            && let Ok(mut output) = output.lock()
                            && output.emit(code, value).is_err()
                        {
                            return;
                        }
                        if sink.send(event).is_err() {
                            filter.gate.fail_open();
                            if captured && let Ok(mut output) = output.lock() {
                                let _ = output.emit(code, value);
                            }
                            return;
                        }
                    }
                }
                if !gone.is_empty() {
                    filter.gate.fail_open();
                }
                for path in gone {
                    devices.remove(&path);
                    for code in physical_keys.remove(&path) {
                        if let Some(key) = PhysKey::from_evdev(code) {
                            desktop.note_menu_key(key, false);
                            if key.is_modifier() {
                                mods.set(key, false);
                            }
                            let _ = sink.send(InputEvent::Key {
                                key,
                                pressed: false,
                                repeat: false,
                                injected: false,
                                time: Instant::now(),
                            });
                        }
                        consumed.remove(&code);
                        if let Ok(mut output) = output.lock() {
                            let _ = output.emit(code, 0);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            filter.gate.fail_open();
        });
        Ok(Box::new(Capture {
            stop,
            thread: Some(thread),
            output: self.output.clone(),
        }))
    }
}
fn discover(requested: &[String]) -> Result<HashMap<PathBuf, Device>> {
    discover_missing(requested, &HashMap::new())
}
fn is_keyboard(device: &Device) -> bool {
    device
        .supported_keys()
        .is_some_and(|keys| keys.contains(KeyCode::KEY_A) && keys.contains(KeyCode::KEY_Z))
}
fn discover_missing(
    requested: &[String],
    existing: &HashMap<PathBuf, Device>,
) -> Result<HashMap<PathBuf, Device>> {
    let mut devices = HashMap::new();
    let paths: Vec<PathBuf> = if requested.is_empty() {
        std::fs::read_dir("/dev/input")
            .map_err(error)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("event"))
            })
            .collect()
    } else {
        requested.iter().map(PathBuf::from).collect()
    };
    for path in paths {
        if existing.contains_key(&path) {
            continue;
        }
        let Ok(mut device) = Device::open(&path) else {
            continue;
        };
        if device.name() == Some(VIRTUAL_NAME) {
            continue;
        }
        let keyboard = device
            .supported_keys()
            .is_some_and(|keys| keys.contains(KeyCode::KEY_A) && keys.contains(KeyCode::KEY_Z));
        let mouse = device
            .supported_keys()
            .is_some_and(|keys| keys.contains(KeyCode::BTN_LEFT));
        if !keyboard && !mouse {
            continue;
        }
        device.set_nonblocking(true).map_err(error)?;
        if keyboard {
            if device
                .get_key_state()
                .map_err(error)?
                .iter()
                .next()
                .is_some()
            {
                continue;
            }
            // An observer opened before the grab may already have queued keys
            // that the application received. Do not forward those a second time.
            while let Ok(events) = device.fetch_events() {
                if events.count() == 0 {
                    break;
                }
            }
            device.grab().map_err(error)?;
        }
        devices.insert(path, device);
    }
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_keyboards_release_a_shared_modifier_only_after_the_last_release() {
        let mut state = PhysicalKeys::default();
        let a = PathBuf::from("/dev/input/event1");
        let b = PathBuf::from("/dev/input/event2");
        let shift = PhysKey::ShiftLeft.evdev_code();
        assert!(state.event(&a, shift, 1));
        assert!(!state.event(&b, shift, 1));
        assert!(!state.event(&a, shift, 0));
        assert!(state.event(&b, shift, 0));
        assert!(state.event(&a, shift, 1));
        assert!(!state.event(&b, shift, 1));
        assert!(state.remove(&a).is_empty());
        assert_eq!(state.remove(&b), vec![shift]);
    }
    #[test]
    #[ignore = "requires uinput access and an isolated X11 display; creates only a test keyboard"]
    fn real_evdev_source_withholds_boundaries_and_releases_the_device() {
        use x11rb::wrapper::ConnectionExt as _;
        use x11rb::{
            connection::Connection,
            protocol::xproto::{self, ConnectionExt},
        };
        let (connection, screen) = x11rb::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        let window = connection.generate_id().unwrap();
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                200,
                100,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                0,
                &xproto::CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
        connection.map_window(window).unwrap().check().unwrap();
        connection
            .change_property8(
                xproto::PropMode::REPLACE,
                window,
                xproto::AtomEnum::WM_CLASS,
                xproto::AtomEnum::STRING,
                b"konsole\0Konsole\0",
            )
            .unwrap()
            .check()
            .unwrap();
        connection
            .set_input_focus(xproto::InputFocus::PARENT, window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        connection.flush().unwrap();
        let mut keys = AttributeSet::new();
        for key in [KeyCode::KEY_A, KeyCode::KEY_Z, KeyCode::KEY_SPACE] {
            keys.insert(key);
        }
        let mut hardware = VirtualDevice::builder()
            .unwrap()
            .name("OKBS isolated test keyboard")
            .with_keys(&keys)
            .unwrap()
            .build()
            .unwrap();
        let path = hardware.enumerate_dev_nodes_blocking().unwrap().next()
            .expect("the kernel must provide evdev event nodes; this WSL kernel cannot run physical input acceptance tests").unwrap();
        let mut config = Config::default();
        config.linux.devices = vec![path.to_string_lossy().into_owned()];
        let desktop = LinuxDesktop::connect().unwrap();
        let (mut source, mut injector) = LinuxSource::new(&config, desktop.clone()).unwrap();
        let virtual_path = source
            .output
            .lock()
            .unwrap()
            .device
            .enumerate_dev_nodes_blocking()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let mut output_reader = Device::open(virtual_path).unwrap();
        output_reader.set_nonblocking(true).unwrap();
        injector
            .tap(&[PhysKey::ControlLeft], PhysKey::KeyC)
            .unwrap();
        let chord: Vec<_> = output_reader
            .fetch_events()
            .unwrap()
            .filter(|event| event.event_type() == EventType::KEY)
            .map(|event| (event.code(), event.value()))
            .collect();
        assert_eq!(
            chord,
            vec![(29, 1), (42, 1), (46, 1), (46, 0), (42, 0), (29, 0)]
        );
        let (sink, events) = crossbeam_channel::unbounded();
        let guard = source.start(sink).unwrap();
        desktop.set_test_session_inactive(true);
        std::thread::sleep(Duration::from_millis(50));
        let mut second_reader = Device::open(&path).unwrap();
        second_reader
            .grab()
            .expect("inactive session must release the physical keyboard");
        second_reader.ungrab().unwrap();
        assert!(
            injector.tap(&[], PhysKey::KeyA).is_err(),
            "inactive session must reject synthetic input"
        );
        desktop.set_test_session_inactive(false);
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            second_reader.grab().is_err(),
            "active session must reacquire its keyboard"
        );
        desktop.set_test_session_locked(true);
        std::thread::sleep(Duration::from_millis(50));
        second_reader
            .grab()
            .expect("locked session must release its keyboard");
        second_reader.ungrab().unwrap();
        desktop.set_test_session_locked(false);
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            second_reader.grab().is_err(),
            "unlocked session must reacquire its keyboard"
        );
        let mut other_hardware = VirtualDevice::builder()
            .unwrap()
            .name("OKBS second selected keyboard")
            .with_keys(&keys)
            .unwrap()
            .build()
            .unwrap();
        let other_path = other_hardware
            .enumerate_dev_nodes_blocking()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let mut other_reader = Device::open(&other_path).unwrap();
        let mut changed = config.clone();
        changed.linux.devices = vec![other_path.to_string_lossy().into_owned()];
        changed.switching.inject_key_delay_ms = 17;
        source.filter.configure(&changed);
        assert_eq!(
            injector.delay.load(Ordering::Relaxed),
            17,
            "injector delay must update live"
        );
        std::thread::sleep(Duration::from_millis(1200));
        second_reader
            .grab()
            .expect("deselected keyboard must be released without restart");
        second_reader.ungrab().unwrap();
        assert!(
            other_reader.grab().is_err(),
            "newly selected keyboard must be captured"
        );
        source.filter.configure(&config);
        std::thread::sleep(Duration::from_millis(1200));
        other_reader
            .grab()
            .expect("the replacement device must be released after restoring selection");
        other_reader.ungrab().unwrap();
        assert!(second_reader.grab().is_err());
        let _ = events.try_iter().count();
        hardware
            .emit(&[EvEvent::new(EventType::KEY.0, KeyCode::KEY_A.0, 1)])
            .unwrap();
        let event = events.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            event,
            InputEvent::Key {
                key: PhysKey::KeyA,
                pressed: true,
                ..
            }
        ));
        hardware
            .emit(&[EvEvent::new(EventType::KEY.0, KeyCode::KEY_A.0, 0)])
            .unwrap();
        events.recv_timeout(Duration::from_secs(2)).unwrap();
        hardware
            .emit(&[EvEvent::new(EventType::KEY.0, KeyCode::KEY_SPACE.0, 1)])
            .unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(2)).unwrap(),
            InputEvent::CapturedKey {
                key: PhysKey::Space,
                ..
            }
        ));
        assert!(
            !source
                .output
                .lock()
                .unwrap()
                .down
                .contains(&KeyCode::KEY_SPACE.0)
        );
        // Deliberately leave the captured boundary unacknowledged, as if the
        // engine had hung in a platform call. The source must release by itself.
        let deadline = Instant::now() + Duration::from_secs(5);
        while source.is_alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !source.is_alive(),
            "a hung engine must not keep the keyboard grabbed"
        );
        drop(guard);
        let mut reader = Device::open(&path).unwrap();
        reader
            .grab()
            .expect("the source must release its exclusive grab");
        reader.ungrab().unwrap();
    }
}
