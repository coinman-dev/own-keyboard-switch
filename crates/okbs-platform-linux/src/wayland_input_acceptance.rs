//! Test-only transport from OKBS uinput to a private Wayland compositor.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// Headless compositors have no libinput seat. This test-only driver delivers
/// production uinput output to their private virtual keyboard transports.
/// It never sends test input to the host desktop or bypasses the engine.
pub(crate) struct KeyboardDriver {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    releases: std::sync::Arc<Vec<std::sync::atomic::AtomicUsize>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for KeyboardDriver {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.expect("Wayland virtual keyboard driver failed");
            }
        }
    }
}
impl KeyboardDriver {
    pub(crate) fn start(
        path: PathBuf,
        remote: Option<(zbus::zvariant::OwnedObjectPath, zbus::blocking::Connection)>,
    ) -> Self {
        let mut reader = evdev::Device::open(path).unwrap();
        reader.set_nonblocking(true).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = stop.clone();
        let releases = std::sync::Arc::new(
            (0..768)
                .map(|_| std::sync::atomic::AtomicUsize::new(0))
                .collect::<Vec<_>>(),
        );
        let delivered = releases.clone();
        let mut transport = Transport::new(remote);
        let thread = std::thread::spawn(move || {
            // Mutter binds the session to its creating D-Bus sender. A clone
            // shares that connection; a newly connected driver is denied.
            while !stopping.load(std::sync::atomic::Ordering::Acquire) {
                match reader.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            if event.event_type() == evdev::EventType::KEY {
                                transport.key(event.code(), event.value() != 0);
                                if event.value() == 0 {
                                    delivered[usize::from(event.code())]
                                        .fetch_add(1, std::sync::atomic::Ordering::Release);
                                }
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) if error.raw_os_error() == Some(libc::ENODEV) => break,
                    Err(error) => panic!("cannot read test uinput output: {error}"),
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        Self {
            stop,
            releases,
            thread: Some(thread),
        }
    }
    pub(crate) fn type_text(&self, hardware: &mut evdev::uinput::VirtualDevice, text: &str) {
        use okbs_core::{
            Lang,
            layouts::{builtin_keymap, keys_for_text},
        };
        for character in text.chars() {
            let single = character.to_string();
            let key = if character == '\n' {
                okbs_core::PhysKey::Enter
            } else {
                keys_for_text(&single, builtin_keymap(Lang::En)).unwrap()[0].key
            };
            let counter = &self.releases[usize::from(key.evdev_code())];
            let before = counter.load(std::sync::atomic::Ordering::Acquire);
            if character == '\n' {
                hardware
                    .emit(&[
                        evdev::InputEvent::new(evdev::EventType::KEY.0, key.evdev_code(), 1),
                        evdev::InputEvent::new(evdev::EventType::KEY.0, key.evdev_code(), 0),
                    ])
                    .unwrap();
            } else {
                crate::acceptance::type_physical(hardware, &single);
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while counter.load(std::sync::atomic::Ordering::Acquire) == before {
                assert!(
                    Instant::now() < deadline,
                    "Wayland compositor did not receive fixture key {key:?}"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

struct TestProcess(std::process::Child);
impl Drop for TestProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
enum Transport {
    Gnome(zbus::zvariant::OwnedObjectPath, zbus::blocking::Connection),
    Kde {
        _process: TestProcess,
        input: std::process::ChildStdin,
        output: std::io::BufReader<std::process::ChildStdout>,
    },
}
impl Transport {
    fn new(remote: Option<(zbus::zvariant::OwnedObjectPath, zbus::blocking::Connection)>) -> Self {
        use std::io::BufRead;
        match remote {
            Some((path, bus)) => Self::Gnome(path, bus),
            None => {
                let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
                let mut process = TestProcess(
                    std::process::Command::new(
                        workspace.join("tmp/linux-kde-keyboard/keyboard-fixture"),
                    )
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .expect("build the KDE keyboard fixture before device acceptance"),
                );
                let input = process.0.stdin.take().unwrap();
                let mut output = std::io::BufReader::new(process.0.stdout.take().unwrap());
                let mut ready = String::new();
                output.read_line(&mut ready).unwrap();
                assert_eq!(
                    ready.trim(),
                    "ready",
                    "private KWin keyboard fixture did not start"
                );
                Self::Kde {
                    _process: process,
                    input,
                    output,
                }
            }
        }
    }
    fn key(&mut self, code: u16, pressed: bool) {
        use std::io::{BufRead, Write};
        match self {
            Self::Gnome(path, bus) => {
                let remote = zbus::blocking::Proxy::new(
                    bus,
                    "org.gnome.Mutter.RemoteDesktop",
                    path.as_str(),
                    "org.gnome.Mutter.RemoteDesktop.Session",
                )
                .unwrap();
                remote
                    .call::<_, _, ()>("NotifyKeyboardKeycode", &(u32::from(code), pressed))
                    .unwrap();
            }
            Self::Kde { input, output, .. } => {
                let request = format!("{code} {}", u8::from(pressed));
                writeln!(input, "{request}").unwrap();
                input.flush().unwrap();
                let mut response = String::new();
                output.read_line(&mut response).unwrap();
                assert_eq!(
                    response.trim(),
                    request,
                    "KWin test keyboard delivery failed"
                );
            }
        }
    }
}
