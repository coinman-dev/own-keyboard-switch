//! Acceptance fixture uses real evdev/uinput and an XTEST driver bridge into
//! an isolated Xvfb server. xterm runs a real shell/readline, never a text mock.
use crate::{desktop::LinuxDesktop, input::LinuxSource, services::LinuxClipboard};
use evdev::{
    AttributeSet, Device, EventType, InputEvent as EvEvent, KeyCode, uinput::VirtualDevice,
};
use okbs_core::{
    Lang, PhysKey,
    config::Config,
    layouts::{builtin_keymap, keys_for_text},
};
use okbs_engine::{Backends, Command, EngineHandle, Event, Inputs, Processor};
use okbs_platform::{KeyboardSource, LayoutId, LayoutManager};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{self, ConnectionExt},
        xtest,
    },
    rust_connection::RustConnection,
};

struct Child(std::process::Child);
pub(crate) fn virtual_device_node(device: &mut VirtualDevice) -> PathBuf {
    // UI_DEV_CREATE can publish its sysfs event child before udev has made
    // /dev/input/eventN. "blocking" in evdev means synchronous directory IO,
    // not waiting for device-node creation; require the real node explicitly.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(path) = device
            .enumerate_dev_nodes_blocking()
            .ok()
            .and_then(|nodes| nodes.filter_map(Result::ok).find(|path| path.exists()))
        {
            return path;
        }
        assert!(
            Instant::now() < deadline,
            "virtual keyboard has no evdev node; verify kernel evdev support and udev in the isolated test VM"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires isolated X11, a normal user and the built application; opens the setup UI"]
fn real_setup_window_stays_open_without_input_access_and_closes_cleanly() {
    assert!(
        !crate::permissions::is_root(),
        "run the GUI acceptance as a regular user"
    );
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = workspace.join("tmp/linux-startup");
    std::fs::create_dir_all(&scratch).unwrap();
    let temporary = tempfile::TempDir::new_in(&scratch).unwrap();
    let config = temporary.path().join("config.toml");
    let mut settings = Config::default();
    settings.linux.devices = vec!["/nonexistent/okbs-startup-fixture".into()];
    settings.general.run_elevated = false;
    settings.general.ui_language = okbs_core::config::UiLanguage::En;
    let fake_bin = temporary.path().join("fake-bin");
    std::fs::create_dir(&fake_bin).unwrap();
    let fake_helper = fake_bin.join("pkexec");
    // Only the child application sees this stub. Neither this test nor the
    // stub invokes the administrative helper or changes device permissions.
    std::fs::write(&fake_helper, "#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$OKBS_FIXTURE_PID\"\nprintf '%s\\n' \"$@\" > \"$OKBS_FIXTURE_ARGS\"\nif [ \"$OKBS_FIXTURE_MODE\" = pending ]; then exec sleep 30; fi\nexit 126\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake_helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let state = crate::seat::Seat::connect().state();
    let authorization_available = state.active
        && !state.locked
        && matches!(
            crate::access::uinput_access(),
            crate::access::DeviceAccess::Denied | crate::access::DeviceAccess::Missing
        );
    let modes: &[&str] = if authorization_available {
        &["none", "cancel", "pending"]
    } else {
        &["none"]
    };
    for &mode in modes {
        settings.general.run_elevated = mode != "none";
        okbs_core::config::save(&config, &settings).unwrap();
        let helper_pid = temporary.path().join(format!("{mode}-pid.txt"));
        let helper_args = temporary.path().join(format!("{mode}-args.txt"));
        let log = temporary.path().join("application.log");
        let output = std::fs::File::create(&log).unwrap();
        let mut application = Child(
            std::process::Command::new(workspace.join("target/debug/okbswitch"))
                .arg("--config")
                .arg(&config)
                .arg("--no-tray")
                .env_remove("APPIMAGE")
                .env(
                    "PATH",
                    format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
                )
                .env("OKBS_FIXTURE_MODE", mode)
                .env("OKBS_FIXTURE_PID", &helper_pid)
                .env("OKBS_FIXTURE_ARGS", &helper_args)
                .stdout(output.try_clone().unwrap())
                .stderr(output)
                .spawn()
                .unwrap(),
        );
        let (connection, screen) = x11rb::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        let deadline = Instant::now() + Duration::from_secs(10);
        let window = loop {
            assert!(
                application.0.try_wait().unwrap().is_none(),
                "setup exited before showing a window: {}",
                std::fs::read_to_string(&log).unwrap()
            );
            if let Some(window) = connection
                .query_tree(root)
                .unwrap()
                .reply()
                .unwrap()
                .children
                .into_iter()
                .find(|&window| {
                    title(&connection, window).contains("Own Keyboard Switch")
                        && connection
                            .get_window_attributes(window)
                            .unwrap()
                            .reply()
                            .unwrap()
                            .map_state
                            == xproto::MapState::VIEWABLE
                })
            {
                break window;
            }
            assert!(
                Instant::now() < deadline,
                "setup window was not shown: {}",
                std::fs::read_to_string(&log).unwrap()
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        if mode != "none" {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !helper_args.exists() {
                assert!(
                    Instant::now() < deadline,
                    "the configured authorization request did not start"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            let args = std::fs::read_to_string(&helper_args).unwrap();
            assert!(args.contains("--setup-linux-input-helper"));
            assert!(
                !args.contains("--config"),
                "user configuration is not passed to the administrative helper"
            );
        }
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            application.0.try_wait().unwrap().is_none(),
            "missing access must leave the setup UI available"
        );
        let capture = std::process::Command::new("python3").args(["-c", "import gi,sys; gi.require_version('Gdk','3.0'); from gi.repository import Gdk; w=Gdk.get_default_root_window(); p=Gdk.pixbuf_get_from_window(w,0,0,w.get_width(),w.get_height()); p.savev(sys.argv[1],'png',[],[])"])
        .arg(scratch.join(format!("setup-{mode}.png"))).status().unwrap();
        assert!(capture.success());
        let protocol = connection
            .intern_atom(false, b"WM_PROTOCOLS")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let close = connection
            .intern_atom(false, b"WM_DELETE_WINDOW")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        connection
            .send_event(
                false,
                window,
                xproto::EventMask::NO_EVENT,
                xproto::ClientMessageEvent::new(
                    32,
                    window,
                    protocol,
                    [close, x11rb::CURRENT_TIME, 0, 0, 0],
                ),
            )
            .unwrap()
            .check()
            .unwrap();
        connection.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = application.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "closing setup failed: {}",
                    std::fs::read_to_string(&log).unwrap()
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "closing setup left a running application"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        if mode != "none" {
            let pid: u32 = std::fs::read_to_string(&helper_pid)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(
                !std::path::Path::new(&format!("/proc/{pid}")).exists(),
                "closing setup left the authorization request running"
            );
        }
    }
}
struct Driver {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    enters: Arc<AtomicUsize>,
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn output_driver(path: PathBuf, root: u32) -> Driver {
    let mut reader = Device::open(path).unwrap();
    reader.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let enters = Arc::new(AtomicUsize::new(0));
    let counting = enters.clone();
    let thread = std::thread::spawn(move || {
        let (connection, _) = x11rb::connect(None).unwrap();
        while !stopping.load(Ordering::Acquire) {
            if let Ok(events) = reader.fetch_events() {
                for event in events {
                    if event.event_type() == EventType::KEY {
                        if event.code() == PhysKey::Enter.evdev_code() && event.value() == 1 {
                            counting.fetch_add(1, Ordering::Release);
                        }
                        let code = u8::try_from(event.code() + 8).unwrap();
                        let kind = if event.value() == 0 {
                            xproto::KEY_RELEASE_EVENT
                        } else {
                            xproto::KEY_PRESS_EVENT
                        };
                        xtest::fake_input(&connection, kind, code, 0, root, 0, 0, 0)
                            .unwrap()
                            .check()
                            .unwrap();
                    }
                }
                connection.flush().unwrap();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    Driver {
        stop,
        thread: Some(thread),
        enters,
    }
}
fn title(conn: &RustConnection, window: u32) -> String {
    for name in [b"_NET_WM_NAME".as_slice(), b"WM_NAME".as_slice()] {
        let atom = conn.intern_atom(false, name).unwrap().reply().unwrap().atom;
        let value = conn
            .get_property(false, window, atom, xproto::AtomEnum::ANY, 0, 256)
            .unwrap()
            .reply()
            .unwrap()
            .value;
        if !value.is_empty() {
            return String::from_utf8_lossy(&value)
                .trim_end_matches('\0')
                .into();
        }
    }
    String::new()
}
pub(crate) fn type_physical(device: &mut VirtualDevice, text: &str) {
    for press in keys_for_text(text, builtin_keymap(Lang::En)).unwrap() {
        if press.shift {
            device
                .emit(&[EvEvent::new(
                    EventType::KEY.0,
                    PhysKey::ShiftLeft.evdev_code(),
                    1,
                )])
                .unwrap();
        }
        device
            .emit(&[
                EvEvent::new(EventType::KEY.0, press.key.evdev_code(), 1),
                EvEvent::new(EventType::KEY.0, press.key.evdev_code(), 0),
            ])
            .unwrap();
        if press.shift {
            device
                .emit(&[EvEvent::new(
                    EventType::KEY.0,
                    PhysKey::ShiftLeft.evdev_code(),
                    0,
                )])
                .unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires root/uinput, isolated X11, xterm; uses a real shell"]
fn real_terminal_receives_corrected_line_before_enter_and_enter_is_not_replayed() {
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/linux-terminal");
    std::fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::TempDir::new_in(&scratch).unwrap();
    let result = temp.path().join("submitted.txt");
    let terminal = Child(
        std::process::Command::new("xterm")
            .args([
                "-title",
                "OKBS terminal acceptance",
                "-u8",
                "-e",
                "bash",
                "--noprofile",
                "--norc",
                "-c",
                r#"IFS= read -r line; printf '%s\n' "$line" > "$1"; sleep 15"#,
                "bash",
            ])
            .arg(&result)
            .spawn()
            .unwrap(),
    );
    let (connection, screen) = x11rb::connect(None).unwrap();
    let root = connection.setup().roots[screen].root;
    let deadline = Instant::now() + Duration::from_secs(5);
    let window = loop {
        let windows = connection
            .query_tree(root)
            .unwrap()
            .reply()
            .unwrap()
            .children;
        if let Some(window) = windows.into_iter().find(|window| {
            title(&connection, *window) == "OKBS terminal acceptance"
                && connection
                    .get_window_attributes(*window)
                    .unwrap()
                    .reply()
                    .unwrap()
                    .map_state
                    == xproto::MapState::VIEWABLE
        }) {
            break window;
        }
        assert!(
            Instant::now() < deadline,
            "xterm did not create its test window"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    connection
        .set_input_focus(xproto::InputFocus::PARENT, window, x11rb::CURRENT_TIME)
        .unwrap()
        .check()
        .unwrap();
    let mut keys = AttributeSet::new();
    for &key in PhysKey::ALL {
        keys.insert(KeyCode(key.evdev_code()));
    }
    let mut hardware = VirtualDevice::builder()
        .unwrap()
        .name("OKBS terminal fixture keyboard")
        .with_keys(&keys)
        .unwrap()
        .build()
        .unwrap();
    let path = virtual_device_node(&mut hardware);
    let mut config = Config::default();
    config.linux.devices = vec![path.to_string_lossy().into_owned()];
    config.general.passwords_to_english = false;
    config.switching.inject_key_delay_ms = 0;
    let mut desktop = LinuxDesktop::connect().unwrap();
    desktop.set(LayoutId(1)).unwrap();
    let (mut source, injector) = LinuxSource::new(&config, desktop.clone()).unwrap();
    let output_path = source.virtual_nodes().unwrap().into_iter().next().unwrap();
    let driver = output_driver(output_path, root);
    let (sink, input) = crossbeam_channel::unbounded();
    let mut processor = Processor::new(
        config.clone(),
        Backends {
            injector: Box::new(injector),
            layouts: Box::new(desktop.clone()),
            clipboard: Some(Box::new(LinuxClipboard::new().unwrap())),
            sound: None,
            focus: Some(Box::new(desktop.clone())),
        },
    );
    processor.set_input_gate(source.filter.gate.clone());
    processor.set_swallows_capslock(true);
    let engine = EngineHandle::spawn(
        processor,
        Inputs {
            input,
            layout: None,
            focus: None,
            clipboard: None,
        },
    )
    .unwrap();
    let guard = source.start(sink).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    type_physical(&mut hardware, "Github b linux");
    hardware
        .emit(&[
            EvEvent::new(EventType::KEY.0, PhysKey::Enter.evdev_code(), 1),
            EvEvent::new(EventType::KEY.0, PhysKey::Enter.evdev_code(), 0),
        ])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    // The shell creates the redirection target just before printf writes it.
    // Wait for the completed write rather than racing the empty new file.
    while std::fs::read_to_string(&result).is_ok_and(|text| text.is_empty()) || !result.exists() {
        assert!(
            Instant::now() < deadline,
            "the terminal did not receive Enter"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(&result).unwrap(),
        "Github и linux\n"
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        driver.enters.load(Ordering::Acquire),
        1,
        "terminal Enter must be delivered exactly once"
    );
    let conversions = engine
        .events()
        .try_iter()
        .filter(|event| matches!(event, Event::Converted { .. }))
        .count();
    assert!(conversions >= 3);
    assert!(engine.send(Command::SetAutoswitch(false)));
    engine.shutdown();
    drop(guard);
    drop(driver);
    drop(terminal);
}

#[test]
#[ignore = "requires root/uinput, isolated X11 and GTK accessibility; uses real GTK fields"]
fn stale_spelling_popup_never_changes_another_real_field() {
    use okbs_platform::FocusInfo;
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/linux-fields");
    std::fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::TempDir::new_in(scratch).unwrap();
    let state = temp.path().join("state.json");
    let command = temp.path().join("command.txt");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/linux-field-fixture.py");
    let child = Child(
        std::process::Command::new("python3")
            .arg(fixture)
            .arg(&state)
            .arg(&command)
            .spawn()
            .unwrap(),
    );
    let (connection, screen) = x11rb::connect(None).unwrap();
    let root = connection.setup().roots[screen].root;
    let deadline = Instant::now() + Duration::from_secs(5);
    let window = loop {
        if let Some(window) = connection
            .query_tree(root)
            .unwrap()
            .reply()
            .unwrap()
            .children
            .into_iter()
            .find(|window| {
                title(&connection, *window) == "OKBS two-field acceptance"
                    && connection
                        .get_window_attributes(*window)
                        .unwrap()
                        .reply()
                        .unwrap()
                        .map_state
                        == xproto::MapState::VIEWABLE
            })
        {
            break window;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    connection
        .set_input_focus(xproto::InputFocus::PARENT, window, x11rb::CURRENT_TIME)
        .unwrap()
        .check()
        .unwrap();
    let mut desktop = LinuxDesktop::connect().unwrap();
    desktop.set(LayoutId(0)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let target = loop {
        desktop.refresh().unwrap();
        if desktop.is_password_field().unwrap() == Some(false) {
            break desktop.input_target().unwrap().unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "field identity did not become available"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut keys = AttributeSet::new();
    for &key in PhysKey::ALL {
        keys.insert(KeyCode(key.evdev_code()));
    }
    let mut hardware = VirtualDevice::builder()
        .unwrap()
        .name("OKBS field fixture keyboard")
        .with_keys(&keys)
        .unwrap()
        .build()
        .unwrap();
    let path = virtual_device_node(&mut hardware);
    let mut config = Config::default();
    config.linux.devices = vec![path.to_string_lossy().into_owned()];
    config.spellcheck.check_typed_words = true;
    config.general.passwords_to_english = false;
    config.switching.inject_key_delay_ms = 0;
    let (mut source, injector) = LinuxSource::new(&config, desktop.clone()).unwrap();
    let driver = output_driver(source.virtual_nodes().unwrap().remove(0), root);
    let (sink, input) = crossbeam_channel::unbounded();
    let (focus_sink, focus_input) = crossbeam_channel::unbounded();
    let focus_watch = FocusInfo::subscribe(&mut desktop, focus_sink).unwrap();
    let mut processor = Processor::new(
        config.clone(),
        Backends {
            injector: Box::new(injector),
            layouts: Box::new(desktop.clone()),
            clipboard: Some(Box::new(LinuxClipboard::new().unwrap())),
            sound: None,
            focus: Some(Box::new(desktop.clone())),
        },
    );
    processor.set_input_gate(source.filter.gate.clone());
    let engine = EngineHandle::spawn(
        processor,
        Inputs {
            input,
            layout: None,
            focus: Some(focus_input),
            clipboard: None,
        },
    )
    .unwrap();
    let guard = source.start(sink).unwrap();
    type_physical(&mut hardware, "wrold ");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if engine
            .events()
            .recv_timeout(Duration::from_millis(100))
            .is_ok_and(|event| matches!(event, Event::CheckSpelling { .. }))
        {
            break;
        }
        assert!(Instant::now() < deadline, "word snapshot was not captured");
    }
    engine.send(Command::ReplaceTypedSpelling {
        request_id: 90,
        target,
        original: "wrold".into(),
        corrected: "world".into(),
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(Event::SpellingReplacementFinished {
            request_id: 90,
            success,
        }) = engine.events().recv_timeout(Duration::from_millis(100))
        {
            assert!(
                success,
                "a valid popup must restore and replace the original field"
            );
            break;
        }
        assert!(Instant::now() < deadline);
    }
    std::thread::sleep(Duration::from_millis(100));
    let valid: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
    assert_eq!(valid["first"], "world ");
    type_physical(&mut hardware, "wrold ");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if engine
            .events()
            .recv_timeout(Duration::from_millis(100))
            .is_ok_and(|event| matches!(event, Event::CheckSpelling { .. }))
        {
            break;
        }
        assert!(Instant::now() < deadline);
    }
    std::fs::write(&command, "second").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        desktop.refresh().unwrap();
        if desktop
            .input_target()
            .unwrap()
            .is_some_and(|actual| actual.control != target.control)
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    engine.send(Command::ReplaceTypedSpelling {
        request_id: 91,
        target,
        original: "wrold".into(),
        corrected: "world".into(),
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(Event::SpellingReplacementFinished {
            request_id: 91,
            success,
        }) = engine.events().recv_timeout(Duration::from_millis(100))
        {
            assert!(
                !success,
                "stale popup must not replace text in a different field"
            );
            break;
        }
        assert!(Instant::now() < deadline);
    }
    let values: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
    assert_eq!(values["first"], "world wrold ");
    assert_eq!(values["second"], "");
    std::fs::write(&command, "password").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        desktop.refresh().unwrap();
        if desktop.is_password_field().unwrap() == Some(true) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    type_physical(&mut hardware, "ghbdtn ");
    std::thread::sleep(Duration::from_millis(150));
    let protected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
    assert_eq!(
        protected["second"], "ghbdtn ",
        "password field must remain untouched by layout detection"
    );
    // Continue in the same real GTK process, through the same physical input
    // path. Alt must match the menu, Escape must put the typing layout back.
    config.advanced.fix_layout_in_menus = true;
    config.spellcheck.check_typed_words = false;
    source.filter.configure(&config);
    engine.send(Command::ApplyConfig(Box::new(config.clone())));
    for (menu, language, initial, expected) in [
        ("en", Some(Lang::En), LayoutId(1), LayoutId(0)),
        ("ru", Some(Lang::Ru), LayoutId(0), LayoutId(1)),
        ("none", None, LayoutId(1), LayoutId(1)),
    ] {
        std::fs::write(&command, format!("menu-{menu}")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if !command.exists()
                && desktop.is_password_field().unwrap() == Some(false)
                && desktop.menu_access_language().unwrap() == language
            {
                break;
            }
            assert!(Instant::now() < deadline, "menu {menu} was not detected");
            std::thread::sleep(Duration::from_millis(20));
        }
        desktop.set(initial).unwrap();
        hardware
            .emit(&[EvEvent::new(
                EventType::KEY.0,
                PhysKey::AltLeft.evdev_code(),
                1,
            )])
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));
        let access_key = match menu {
            "en" => PhysKey::KeyF,
            "ru" => PhysKey::KeyA,
            _ => PhysKey::F10,
        };
        if menu == "none" {
            hardware
                .emit(&[EvEvent::new(
                    EventType::KEY.0,
                    PhysKey::AltLeft.evdev_code(),
                    0,
                )])
                .unwrap();
        }
        hardware
            .emit(&[
                EvEvent::new(EventType::KEY.0, access_key.evdev_code(), 1),
                EvEvent::new(EventType::KEY.0, access_key.evdev_code(), 0),
                EvEvent::new(EventType::KEY.0, PhysKey::AltLeft.evdev_code(), 0),
            ])
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let values: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
            if values["menu_open"] == true && desktop.current().unwrap() == expected {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Alt did not match/open {menu} menu: state={values}, layout={:?}",
                desktop.current().unwrap()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // Let the focus watcher observe the opened menu before Escape.
        std::thread::sleep(Duration::from_millis(250));
        hardware
            .emit(&[
                EvEvent::new(EventType::KEY.0, PhysKey::Escape.evdev_code(), 1),
                EvEvent::new(EventType::KEY.0, PhysKey::Escape.evdev_code(), 0),
            ])
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let values: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
            if values["menu_open"] == false && desktop.current().unwrap() == initial {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "typing layout was not restored after {menu} menu closed: state={values}, layout={:?}, desktop={:?}",
                desktop.current().unwrap(),
                desktop.cached()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    // GTK only exposes the access keys on Alt alone. It need not open a menu,
    // so the temporary layout still has to return after releasing Alt.
    std::fs::write(&command, "menu-en").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while command.exists() || desktop.menu_access_language().unwrap() != Some(Lang::En) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    desktop.set(LayoutId(1)).unwrap();
    hardware
        .emit(&[EvEvent::new(
            EventType::KEY.0,
            PhysKey::AltLeft.evdev_code(),
            1,
        )])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while desktop.current().unwrap() != LayoutId(0) {
        assert!(
            Instant::now() < deadline,
            "Alt must prepare the access language"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    hardware
        .emit(&[EvEvent::new(
            EventType::KEY.0,
            PhysKey::AltLeft.evdev_code(),
            0,
        )])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while desktop.current().unwrap() != LayoutId(1) {
        assert!(
            Instant::now() < deadline,
            "Alt alone stranded the menu layout"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    hardware
        .emit(&[EvEvent::new(
            EventType::KEY.0,
            PhysKey::AltLeft.evdev_code(),
            1,
        )])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while desktop.current().unwrap() != LayoutId(0) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    config.advanced.fix_layout_in_menus = false;
    source.filter.configure(&config);
    engine.send(Command::ApplyConfig(Box::new(config.clone())));
    let deadline = Instant::now() + Duration::from_secs(3);
    while desktop.current().unwrap() != LayoutId(1) {
        assert!(
            Instant::now() < deadline,
            "disabling menu correction must restore the typing layout"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    hardware
        .emit(&[EvEvent::new(
            EventType::KEY.0,
            PhysKey::AltLeft.evdev_code(),
            0,
        )])
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    hardware
        .emit(&[EvEvent::new(
            EventType::KEY.0,
            PhysKey::AltLeft.evdev_code(),
            1,
        )])
        .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        desktop.current().unwrap(),
        LayoutId(1),
        "disabled menu option changed the layout"
    );
    hardware
        .emit(&[EvEvent::new(
            EventType::KEY.0,
            PhysKey::AltLeft.evdev_code(),
            0,
        )])
        .unwrap();
    config.advanced.fix_layout_in_menus = true;
    source.filter.configure(&config);
    engine.send(Command::ApplyConfig(Box::new(config.clone())));
    std::thread::sleep(Duration::from_millis(100));
    hardware
        .emit(&[
            EvEvent::new(EventType::KEY.0, PhysKey::ControlLeft.evdev_code(), 1),
            EvEvent::new(EventType::KEY.0, PhysKey::AltLeft.evdev_code(), 1),
        ])
        .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        desktop.current().unwrap(),
        LayoutId(1),
        "Ctrl+Alt must remain an ordinary chord"
    );
    hardware
        .emit(&[
            EvEvent::new(EventType::KEY.0, PhysKey::AltLeft.evdev_code(), 0),
            EvEvent::new(EventType::KEY.0, PhysKey::ControlLeft.evdev_code(), 0),
        ])
        .unwrap();
    // Installed layout IDs can change when the desktop is reconfigured.
    // Applying a backend must refresh the engine's ID-to-language mapping.
    assert!(
        std::process::Command::new("setxkbmap")
            .args(["-layout", "ru,us"])
            .status()
            .unwrap()
            .success()
    );
    desktop.set(LayoutId(1)).unwrap();
    config.linux.layout_backend = okbs_core::config::LayoutBackend::X11;
    let prepared = desktop.prepare_layout(&config).unwrap().unwrap();
    source.filter.gate.fail_open();
    desktop.apply_layout(prepared).unwrap();
    source.filter.configure(&config);
    engine.send(Command::ApplyConfig(Box::new(config.clone())));
    std::thread::sleep(Duration::from_millis(150));
    type_physical(&mut hardware, "ghbdtn ");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let values: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
        if values["first"].as_str().unwrap().ends_with("привет ") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "layout IDs were stale after applying the backend: {values}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        std::process::Command::new("setxkbmap")
            .args(["-layout", "us,ru"])
            .status()
            .unwrap()
            .success()
    );
    engine.shutdown();
    drop(guard);
    drop(focus_watch);
    drop(driver);
    drop(child);
}
