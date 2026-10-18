//! «Управление мышью»: mice whose assigned buttons must not reach
//! applications are grabbed and forwarded through a virtual twin without
//! those buttons. Everything else (motion, wheel, other buttons) passes
//! unchanged. The grab is released by Device::drop, on a locked or inactive
//! session, and whenever the settings no longer block any button.
use crate::desktop::LinuxDesktop;
use evdev::{
    AttributeSet, Device, EventType, InputEvent as EvEvent, KeyCode, RelativeAxisCode,
    SynchronizationCode, uinput::VirtualDevice,
};
use okbs_core::config::{Config, Mouse, MouseButton};
use okbs_platform::InputEvent;
use okbs_platform::autoreplace_gate::AutoReplaceGate;
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Input id version of the virtual twins, which are never grabbed again.
/// Vendor, product and name stay those of the real mouse, so desktop
/// pointer settings keep applying. (evdev's UI_SET_PHYS fails with EINVAL.)
const TWIN_VERSION: u16 = 0x6f6b;

pub(crate) fn button_of(code: u16) -> Option<MouseButton> {
    match KeyCode(code) {
        KeyCode::BTN_MIDDLE => Some(MouseButton::Middle),
        KeyCode::BTN_SIDE => Some(MouseButton::Back),
        KeyCode::BTN_EXTRA => Some(MouseButton::Forward),
        _ => None,
    }
}

pub(crate) fn is_own_virtual(device: &Device) -> bool {
    device.input_id().version() == TWIN_VERSION
}

/// A relative pointer with at least one button the settings may withhold.
/// Touchpads, tablets and keyboards are never grabbed here.
fn is_remappable_mouse(device: &Device, mouse: &Mouse) -> bool {
    if is_own_virtual(device)
        || device
            .supported_absolute_axes()
            .is_some_and(|a| a.iter().next().is_some())
    {
        return false;
    }
    let Some(keys) = device.supported_keys() else {
        return false;
    };
    if keys.contains(KeyCode::KEY_A) || !keys.contains(KeyCode::BTN_LEFT) {
        return false;
    }
    let relative = device
        .supported_relative_axes()
        .is_some_and(|axes| axes.contains(RelativeAxisCode::REL_X));
    relative
        && keys
            .iter()
            .filter_map(|key| button_of(key.0))
            .any(|button| mouse.blocks(button))
}

struct Grabbed {
    device: Device,
    twin: VirtualDevice,
}

impl Grabbed {
    fn open(path: &PathBuf) -> Option<Self> {
        let mut device = Device::open(path).ok()?;
        // Never take over a mouse in the middle of a drag or click.
        if device.get_key_state().ok()?.iter().next().is_some() {
            return None;
        }
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in device.supported_keys()?.iter() {
            keys.insert(key);
        }
        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        for axis in device.supported_relative_axes()?.iter() {
            axes.insert(axis);
        }
        let mut properties = AttributeSet::new();
        for property in device.properties().iter() {
            properties.insert(property);
        }
        let name = device.name().unwrap_or("mouse").to_owned();
        let id = device.input_id();
        let twin = VirtualDevice::builder()
            .ok()?
            .name(&name)
            .input_id(evdev::InputId::new(
                id.bus_type(),
                id.vendor(),
                id.product(),
                TWIN_VERSION,
            ))
            .with_keys(&keys)
            .ok()?
            .with_relative_axes(&axes)
            .ok()?
            .with_properties(&properties)
            .ok()?
            .build()
            .ok()?;
        device.set_nonblocking(true).ok()?;
        // Drop events queued before the grab; applications already got them.
        while let Ok(events) = device.fetch_events() {
            if events.count() == 0 {
                break;
            }
        }
        device.grab().ok()?;
        tracing::info!(device = %name, path = %path.display(), "mouse grabbed for button remapping");
        Some(Self { device, twin })
    }
}

#[derive(Debug)]
pub(crate) struct MouseRemap {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MouseRemap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl MouseRemap {
    pub(crate) fn start(
        config: Arc<Mutex<Config>>,
        gate: Arc<AutoReplaceGate>,
        desktop: LinuxDesktop,
        sink: crossbeam_channel::Sender<InputEvent>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let thread = std::thread::Builder::new()
            .name("okbs-mouse".into())
            .spawn(move || run(&signal, &config, &gate, &desktop, &sink))
            .ok();
        Self { stop, thread }
    }
}

fn run(
    stop: &AtomicBool,
    config: &Mutex<Config>,
    gate: &AutoReplaceGate,
    desktop: &LinuxDesktop,
    sink: &crossbeam_channel::Sender<InputEvent>,
) {
    let mut grabbed: HashMap<PathBuf, Grabbed> = HashMap::new();
    let mut last_scan: Option<Instant> = None;
    while !stop.load(Ordering::Acquire) {
        let session = desktop.cached();
        let (mouse, requested) = match config.lock() {
            Ok(config) => (config.mouse.clone(), config.linux.devices.clone()),
            Err(_) => return,
        };
        if session.locked || session.session_inactive {
            grabbed.clear();
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        if last_scan.is_none_or(|at| at.elapsed() > Duration::from_secs(1)) {
            last_scan = Some(Instant::now());
            let nodes = event_nodes(&requested);
            grabbed
                .retain(|path, g| nodes.contains(path) && is_remappable_mouse(&g.device, &mouse));
            for path in nodes {
                if grabbed.contains_key(&path) {
                    continue;
                }
                let wanted = Device::open(&path).is_ok_and(|d| is_remappable_mouse(&d, &mouse));
                if wanted && let Some(g) = Grabbed::open(&path) {
                    grabbed.insert(path, g);
                }
            }
        }
        if grabbed.is_empty() {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        wait_readable(grabbed.values().map(|g| g.device.as_raw_fd()), 50);
        let mut gone = Vec::new();
        for (path, g) in &mut grabbed {
            let events: Vec<EvEvent> = match g.device.fetch_events() {
                Ok(events) => events.collect(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(_) => {
                    gone.push(path.clone());
                    continue;
                }
            };
            let mut frame = Vec::new();
            for event in events {
                if event.event_type() == EventType::SYNCHRONIZATION {
                    if event.code() == SynchronizationCode::SYN_REPORT.0 && !frame.is_empty() {
                        let _ = g.twin.emit(&frame);
                        frame.clear();
                    }
                    continue;
                }
                if event.event_type() == EventType::MISC {
                    continue;
                }
                if event.event_type() == EventType::KEY {
                    let blocked = button_of(event.code()).filter(|&b| mouse.blocks(b));
                    if event.value() == 1 {
                        let click = InputEvent::MouseButton {
                            time: Instant::now(),
                            in_own_window: desktop.cached().pid == std::process::id(),
                        };
                        gate.capture(click, None, None);
                        let _ = sink.send(click);
                        if let Some(button) = blocked {
                            let _ = sink.send(InputEvent::MouseShortcut {
                                button,
                                time: Instant::now(),
                            });
                        }
                    }
                    if blocked.is_some() {
                        continue;
                    }
                }
                frame.push(event);
            }
            if !frame.is_empty() {
                let _ = g.twin.emit(&frame);
            }
        }
        for path in gone {
            grabbed.remove(&path);
        }
    }
}

/// Selected devices, or every event node when the selection is empty.
fn event_nodes(requested: &[String]) -> Vec<PathBuf> {
    if !requested.is_empty() {
        return requested.iter().map(PathBuf::from).collect();
    }
    std::fs::read_dir("/dev/input")
        .map(|dir| {
            dir.filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("event"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Blocks until one of `fds` is readable or `timeout_ms` passes, so pointer
/// motion is forwarded without polling latency.
#[allow(unsafe_code)]
fn wait_readable(fds: impl Iterator<Item = i32>, timeout_ms: i32) {
    let mut polls: Vec<libc::pollfd> = fds
        .map(|fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    // SAFETY: `polls` is a valid, initialized array of `polls.len()` pollfd
    // entries that outlives the call; the descriptors stay open meanwhile.
    unsafe {
        libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, timeout_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_buttons_map_to_back_and_forward() {
        assert_eq!(button_of(KeyCode::BTN_SIDE.0), Some(MouseButton::Back));
        assert_eq!(button_of(KeyCode::BTN_EXTRA.0), Some(MouseButton::Forward));
        assert_eq!(button_of(KeyCode::BTN_MIDDLE.0), Some(MouseButton::Middle));
        assert_eq!(button_of(KeyCode::BTN_LEFT.0), None);
    }

    fn key_events(reader: &mut Device) -> Vec<(u16, i32)> {
        let mut out = Vec::new();
        while let Ok(events) = reader.fetch_events() {
            let batch: Vec<_> = events
                .filter(|e| matches!(e.event_type(), EventType::KEY | EventType::RELATIVE))
                .map(|e| (e.code(), e.value()))
                .collect();
            if batch.is_empty() {
                break;
            }
            out.extend(batch);
        }
        out
    }

    #[test]
    #[ignore = "requires uinput access and a graphical session; creates only a test mouse"]
    fn real_test_mouse_is_grabbed_and_forwarded_without_assigned_buttons() {
        const NAME: &str = "OKBS isolated test mouse";
        let mut keys = AttributeSet::new();
        for key in [
            KeyCode::BTN_LEFT,
            KeyCode::BTN_RIGHT,
            KeyCode::BTN_MIDDLE,
            KeyCode::BTN_SIDE,
            KeyCode::BTN_EXTRA,
        ] {
            keys.insert(key);
        }
        let mut axes = AttributeSet::new();
        for axis in [
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
        ] {
            axes.insert(axis);
        }
        let mut hardware = VirtualDevice::builder()
            .unwrap()
            .name(NAME)
            .with_keys(&keys)
            .unwrap()
            .with_relative_axes(&axes)
            .unwrap()
            .build()
            .unwrap();
        let path = hardware
            .enumerate_dev_nodes_blocking()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        // udev grants the session user access shortly after creation.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Device::open(&path).is_err() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut config = Config::default();
        config.mouse.enabled = true;
        config.linux.devices = vec![path.to_string_lossy().into_owned()];
        let config = Arc::new(Mutex::new(config));
        let gate = Arc::new(AutoReplaceGate::new(&Config::default()));
        let desktop = LinuxDesktop::connect().unwrap();
        // No KWin bridge runs here; the test stands for an unlocked session.
        desktop.set_test_session_locked(false);
        desktop.set_test_session_inactive(false);
        let (sink, events) = crossbeam_channel::unbounded();
        let remap = MouseRemap::start(config.clone(), gate, desktop, sink);
        let grabbed = |path: &PathBuf| {
            let mut probe = Device::open(path).unwrap();
            let free = probe.grab().is_ok();
            if free {
                probe.ungrab().unwrap();
            }
            !free
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while !grabbed(&path) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            grabbed(&path),
            "a mouse with assigned buttons must be grabbed"
        );
        let twin_path = event_nodes(&[])
            .into_iter()
            .find(|node| {
                Device::open(node).is_ok_and(|d| is_own_virtual(&d) && d.name() == Some(NAME))
            })
            .expect("the virtual twin must exist");
        let mut twin = Device::open(&twin_path).unwrap();
        twin.set_nonblocking(true).unwrap();
        let _ = key_events(&mut twin);

        let key = |code: KeyCode, value| EvEvent::new(EventType::KEY.0, code.0, value);
        hardware.emit(&[key(KeyCode::BTN_EXTRA, 1)]).unwrap();
        hardware.emit(&[key(KeyCode::BTN_EXTRA, 0)]).unwrap();
        hardware
            .emit(&[EvEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_X.0,
                5,
            )])
            .unwrap();
        hardware.emit(&[key(KeyCode::BTN_LEFT, 1)]).unwrap();
        hardware.emit(&[key(KeyCode::BTN_LEFT, 0)]).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            key_events(&mut twin),
            vec![
                (RelativeAxisCode::REL_X.0, 5),
                (KeyCode::BTN_LEFT.0, 1),
                (KeyCode::BTN_LEFT.0, 0)
            ],
            "the assigned button is withheld, everything else passes"
        );
        let received: Vec<_> = events.try_iter().collect();
        assert!(matches!(
            received.as_slice(),
            [
                InputEvent::MouseButton { .. },
                InputEvent::MouseShortcut {
                    button: MouseButton::Forward,
                    ..
                },
                InputEvent::MouseButton { .. },
            ]
        ));

        config.lock().unwrap().mouse.enabled = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while grabbed(&path) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !grabbed(&path),
            "turning the feature off releases the mouse"
        );
        drop(remap);
    }
}
