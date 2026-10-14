//! Visible Writer and a real shell inside a native Wayland terminal.
use crate::{
    desktop::LinuxDesktop, input::LinuxSource, services::LinuxClipboard, session::Desktop,
    wayland_input_acceptance::KeyboardDriver,
};
use evdev::{AttributeSet, KeyCode, uinput::VirtualDevice};
use okbs_core::{
    PhysKey,
    config::{
        AutoReplaceItem, AutoReplaceTrigger, Config, Exclusions, ExecutableExclusion,
        FolderExclusion, HotkeyBinding, TitleExclusion,
    },
};
use okbs_engine::{Backends, Command, EngineHandle, Event, Inputs, Processor};
use okbs_platform::{Clipboard, FocusInfo, KeyboardSource, LayoutId, LayoutManager, StopGuard};
use std::{
    path::{Path, PathBuf},
    process::{Child, Stdio},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(8);
        while self.0.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Session {
    desktop: LinuxDesktop,
    bus: zbus::blocking::Connection,
    remote: Option<zbus::zvariant::OwnedObjectPath>,
    _bridge: Option<crate::desktop::BridgeConnection>,
    /// KDE fake-input helper: its stdin takes pointer commands.
    keyboard: Option<(
        Process,
        std::process::ChildStdin,
        std::io::BufReader<std::process::ChildStdout>,
    )>,
}
impl Session {
    fn new() -> Self {
        let kde = crate::SessionInfo::detect().desktop == Desktop::Kde;
        let bridge = kde.then(|| crate::desktop::serve_kde_bridge().unwrap());
        let keyboard = kde.then(|| {
            use std::io::BufRead;
            let mut process = Process(
                std::process::Command::new(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tmp/linux-kde-keyboard/keyboard-fixture"),
                )
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
            );
            let input = process.0.stdin.take().unwrap();
            let mut output = std::io::BufReader::new(process.0.stdout.take().unwrap());
            let mut ready = String::new();
            output.read_line(&mut ready).unwrap();
            assert_eq!(ready.trim(), "ready");
            (process, input, output)
        });
        let bus = zbus::blocking::connection::Builder::session()
            .unwrap()
            .method_timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let remote = (!kde).then(|| {
            let manager = zbus::blocking::Proxy::new(
                &bus,
                "org.gnome.Mutter.RemoteDesktop",
                "/org/gnome/Mutter/RemoteDesktop",
                "org.gnome.Mutter.RemoteDesktop",
            )
            .unwrap();
            let path: zbus::zvariant::OwnedObjectPath = manager.call("CreateSession", &()).unwrap();
            let proxy = zbus::blocking::Proxy::new(
                &bus,
                "org.gnome.Mutter.RemoteDesktop",
                path.as_str(),
                "org.gnome.Mutter.RemoteDesktop.Session",
            )
            .unwrap();
            proxy.call::<_, _, ()>("Start", &()).unwrap();
            for pressed in [true, false] {
                proxy
                    .call::<_, _, ()>("NotifyKeyboardKeycode", &(42u32, pressed))
                    .unwrap();
            }
            drop(proxy);
            path
        });
        let desktop = LinuxDesktop::connect().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !desktop.integration_ready() {
            assert!(
                Instant::now() < deadline,
                "private desktop integration unavailable"
            );
            desktop.refresh().unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        Self {
            desktop,
            bus,
            remote,
            _bridge: bridge,
            keyboard,
        }
    }
}
/// A real pointer action in the private compositor.
#[derive(Debug, Clone, Copy)]
enum Pointer {
    DoubleClick([i32; 2]),
    Drag([i32; 2], [i32; 2]),
}
impl Session {
    fn pointer(&mut self, action: Pointer) {
        if let Some((_, input, output)) = &mut self.keyboard {
            use std::io::{BufRead, Write};
            let command = match action {
                Pointer::DoubleClick([x, y]) => format!("d {x} {y}"),
                Pointer::Drag([x, y], [x2, y2]) => format!("g {x} {y} {x2} {y2}"),
            };
            writeln!(input, "{command}").unwrap();
            input.flush().unwrap();
            let mut reply = String::new();
            output.read_line(&mut reply).unwrap();
            assert_eq!(reply.trim(), command, "KDE pointer helper");
            return;
        }
        let remote = self.remote.clone().expect("GNOME remote desktop session");
        let proxy = zbus::blocking::Proxy::new(
            &self.bus,
            "org.gnome.Mutter.RemoteDesktop",
            remote.as_str(),
            "org.gnome.Mutter.RemoteDesktop.Session",
        )
        .unwrap();
        let button = |pressed: bool| {
            proxy
                .call::<_, _, ()>("NotifyPointerButton", &(272i32, pressed))
                .unwrap();
        };
        let start = match action {
            Pointer::DoubleClick(point) | Pointer::Drag(point, _) => point,
        };
        self.gnome_move_to(&proxy, start);
        match action {
            Pointer::DoubleClick(_) => {
                for _ in 0..2 {
                    button(true);
                    button(false);
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            Pointer::Drag(from, to) => {
                button(true);
                std::thread::sleep(Duration::from_millis(40));
                for _ in 0..10 {
                    proxy
                        .call::<_, _, ()>(
                            "NotifyPointerMotionRelative",
                            &(
                                f64::from(to[0] - from[0]) / 10.0,
                                f64::from(to[1] - from[1]) / 10.0,
                            ),
                        )
                        .unwrap();
                    std::thread::sleep(Duration::from_millis(20));
                }
                button(false);
            }
        }
    }
    /// A new virtual pointer does not share the previous device's position:
    /// move relatively until the Shell reports the requested position.
    fn gnome_move_to(&self, proxy: &zbus::blocking::Proxy<'_>, point: [i32; 2]) {
        let relative = |dx: f64, dy: f64| {
            proxy
                .call::<_, _, ()>("NotifyPointerMotionRelative", &(dx, dy))
                .unwrap();
        };
        relative(100.0, 100.0);
        let mut reached = false;
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(40));
            let [x, y] = self.desktop.refresh().unwrap().cursor;
            if (x - point[0]).abs() < 2 && (y - point[1]).abs() < 2 {
                reached = true;
                break;
            }
            relative(f64::from(point[0] - x), f64::from(point[1] - y));
        }
        assert!(reached, "private Mutter pointer did not reach {point:?}");
        // A corrective move can cross the headless Shell's hot corner.
        let shell = zbus::blocking::Proxy::new(
            &self.bus,
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.freedesktop.DBus.Properties",
        )
        .unwrap();
        let _ = shell.call::<_, _, ()>(
            "Set",
            &(
                "org.gnome.Shell",
                "OverviewActive",
                zbus::zvariant::Value::from(false),
            ),
        );
        std::thread::sleep(Duration::from_millis(300));
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(path) = &self.remote
            && let Ok(proxy) = zbus::blocking::Proxy::new(
                &self.bus,
                "org.gnome.Mutter.RemoteDesktop",
                path.as_str(),
                "org.gnome.Mutter.RemoteDesktop.Session",
            )
        {
            let _ = proxy.call::<_, _, ()>("Stop", &());
        }
    }
}
struct RunningEngine {
    engine: EngineHandle,
    _capture: Box<dyn StopGuard>,
    _focus: Box<dyn StopGuard>,
    driver: KeyboardDriver,
    hardware: VirtualDevice,
    filter: crate::input::InputFilter,
    config: Config,
}
impl RunningEngine {
    fn new(session: &Session) -> Self {
        let mut keys = AttributeSet::new();
        for &key in PhysKey::ALL {
            keys.insert(KeyCode(key.evdev_code()));
        }
        let mut hardware = VirtualDevice::builder()
            .unwrap()
            .name("OKBS native application fixture keyboard")
            .with_keys(&keys)
            .unwrap()
            .build()
            .unwrap();
        let path = crate::acceptance::virtual_device_node(&mut hardware);
        let mut config = Config::default();
        config.linux.devices = vec![path.to_string_lossy().into_owned()];
        config.general.passwords_to_english = false;
        config.switching.inject_key_delay_ms = 4;
        let (mut source, injector) = LinuxSource::new(&config, session.desktop.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let output = loop {
            if let Some(path) = source
                .virtual_nodes()
                .unwrap()
                .into_iter()
                .find(|path| path.exists())
            {
                break path;
            }
            assert!(
                Instant::now() < deadline,
                "owned output keyboard unavailable"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        let driver = KeyboardDriver::start(
            output,
            session
                .remote
                .as_ref()
                .map(|path| (path.clone(), session.bus.clone())),
        );
        // Adding a virtual keyboard can reconfigure KWin's XKB group.
        session.desktop.clone().set(LayoutId(1)).unwrap();
        let (sink, input) = crossbeam_channel::unbounded();
        let (focus_sink, focus_input) = crossbeam_channel::unbounded();
        let focus = FocusInfo::subscribe(&mut session.desktop.clone(), focus_sink).unwrap();
        let mut processor = Processor::new(
            config.clone(),
            Backends {
                injector: Box::new(injector),
                layouts: Box::new(session.desktop.clone()),
                clipboard: Some(Box::new(LinuxClipboard::new().unwrap())),
                sound: None,
                focus: Some(Box::new(session.desktop.clone())),
            },
        );
        // The same Caps Lock setup as the application.
        if let Some(caps) = source.caps_lock_on() {
            processor.set_caps_lock(caps);
        }
        processor.set_swallows_capslock(true);
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
        let capture = source.start(sink).unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut stable = None;
        let mut previous = None;
        loop {
            session.desktop.refresh().unwrap();
            let target = session.desktop.target();
            let ready = target.is_some_and(|target| {
                target.control != 0 || session.desktop.is_terminal().unwrap()
            }) && session.desktop.current().unwrap() == LayoutId(1);
            if ready && target == previous {
                let since = stable.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(400) {
                    break;
                }
            } else {
                stable = None;
            }
            previous = target;
            assert!(
                Instant::now() < deadline,
                "native input target did not settle at startup"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
        Self {
            engine,
            _capture: capture,
            _focus: focus,
            driver,
            hardware,
            filter: source.filter.clone(),
            config,
        }
    }
    fn type_text(&mut self, text: &str) {
        self.driver.type_text(&mut self.hardware, text);
    }
    fn chord(&mut self, modifiers: &[PhysKey], key: PhysKey) {
        use evdev::{EventType, InputEvent};
        for &modifier in modifiers {
            self.hardware
                .emit(&[InputEvent::new(EventType::KEY.0, modifier.evdev_code(), 1)])
                .unwrap();
        }
        self.hardware
            .emit(&[
                InputEvent::new(EventType::KEY.0, key.evdev_code(), 1),
                InputEvent::new(EventType::KEY.0, key.evdev_code(), 0),
            ])
            .unwrap();
        for &modifier in modifiers.iter().rev() {
            self.hardware
                .emit(&[InputEvent::new(EventType::KEY.0, modifier.evdev_code(), 0)])
                .unwrap();
        }
    }
    /// A lone key held like a real key press, released in a later frame.
    fn tap(&mut self, key: PhysKey) {
        use evdev::{EventType, InputEvent};
        self.hardware
            .emit(&[InputEvent::new(EventType::KEY.0, key.evdev_code(), 1)])
            .unwrap();
        std::thread::sleep(Duration::from_millis(40));
        self.hardware
            .emit(&[InputEvent::new(EventType::KEY.0, key.evdev_code(), 0)])
            .unwrap();
    }
    fn configure(&mut self, config: Config, path: &Path) {
        // Exercise actual configuration serialization and the same live
        // source/engine update used by the application's settings controller.
        okbs_core::config::save(path, &config).unwrap();
        let restored = okbs_core::config::load_or_create(path).unwrap().config;
        assert_eq!(restored.autoreplace, config.autoreplace);
        self.filter.configure(&restored);
        assert!(
            self.engine
                .send(Command::ApplyConfig(Box::new(restored.clone())))
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self
                .engine
                .events()
                .recv_timeout(Duration::from_millis(100))
                .is_ok_and(|event| event == Event::ConfigApplied)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "live configuration was not acknowledged"
            );
        }
        self.config = restored;
    }
}

/// A GTK window with two fields from tools/linux-field-fixture.py.
struct FieldWindow {
    process: Process,
    state: PathBuf,
    command: PathBuf,
    title: String,
}
impl FieldWindow {
    fn open(session: &Session, workspace: &Path, directory: &Path, title: &str) -> Self {
        let name: String = title
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        let state = directory.join(format!("{name}.json"));
        let command = directory.join(format!("{name}.command"));
        let log = std::fs::File::create(directory.join(format!("{name}.log"))).unwrap();
        let process = Process(
            std::process::Command::new("python3")
                .arg(workspace.join("tools/linux-field-fixture.py"))
                .args([&state, &command])
                .arg("--no-select-on-focus")
                .arg(format!("--title={title}"))
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
        let window = Self {
            process,
            state,
            command,
            title: title.into(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while session
            .desktop
            .place_fixture_window(window.process.0.id(), title, [40, 40])
            .is_err()
        {
            assert!(Instant::now() < deadline, "{title} was not mapped");
            session.desktop.refresh().unwrap();
            std::thread::sleep(Duration::from_millis(30));
        }
        window
    }
    /// Gives the first field keyboard focus and waits until it is the target.
    fn activate(&self, session: &Session) {
        let pid = self.process.0.id();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let native = session.desktop.refresh().unwrap();
            if native.pid == pid && native.control != 0 {
                break;
            }
            if let Ok(Some(window)) = session.desktop.fixture_window(pid, &self.title)
                && native.window != window.window
            {
                let _ = session.desktop.command("ActivateWindow", window.window);
            }
            assert!(
                Instant::now() < deadline,
                "{} did not become the input target: {native:?}",
                self.title
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    fn first(&self) -> serde_json::Value {
        std::fs::read(&self.state)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .map_or(serde_json::Value::Null, |value| value["first"].clone())
    }
    fn expect_first(&self, text: &str, scratch: &Path) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while self.first() != text {
            assert!(
                Instant::now() < deadline,
                "{}: expected {text:?}, got {:?}; scratch {}",
                self.title,
                self.first(),
                scratch.display()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn clear_first(&self) {
        std::fs::write(&self.command, "clear-first").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.command.exists() || self.first() != "" {
            assert!(Instant::now() < deadline, "{} was not cleared", self.title);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
#[ignore = "requires root/uinput and private GNOME/KDE Wayland with per-window layouts; set OKBS_TEST_APP=layout in the device test script"]
fn real_single_layout_follows_window_focus() {
    assert!(
        crate::permissions::is_root(),
        "run only in an isolated development VM"
    );
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = workspace.join("tmp/linux-native-input");
    std::fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::TempDir::new_in(&scratch).unwrap().keep();
    let session = Session::new();
    let first = FieldWindow::open(&session, &workspace, &temp, "OKBS layout window A");
    let second = FieldWindow::open(&session, &workspace, &temp, "OKBS layout window B");
    let wait_layout = |layout: LayoutId, what: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let current = session.desktop.current().unwrap();
            if current == layout {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{what}: layout {current:?}, expected {layout:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    // Each window starts in English under the session's per-window policy.
    second.activate(&session);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    first.activate(&session);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    let mut running = RunningEngine::new(&session);
    let config_path = temp.join("layout-config.toml");
    let mut options = running.config.clone();
    options.general.autoswitch = false;
    options.switching.switch_key = okbs_core::config::SwitchKey::RightCtrl;
    running.configure(options.clone(), &config_path);
    // RunningEngine leaves Russian active; start window A from English.
    session.desktop.clone().set(LayoutId(0)).unwrap();
    wait_layout(LayoutId(0), "window A in English");
    // Without the option the session keeps a layout per window.
    running.tap(PhysKey::ControlRight);
    wait_layout(LayoutId(1), "switch key in window A");
    second.activate(&session);
    wait_layout(LayoutId(0), "per-window policy restores window B");
    first.activate(&session);
    wait_layout(LayoutId(1), "per-window policy restores window A");
    // «Одна раскладка для всех окон»: the chosen layout follows focus.
    options.switching.single_layout_for_all_windows = true;
    running.configure(options.clone(), &config_path);
    assert!(running.config.switching.single_layout_for_all_windows);
    second.activate(&session);
    wait_layout(LayoutId(1), "single layout applied to window B");
    second.clear_first();
    running.type_text("f");
    second.expect_first("а", &temp);
    running.tap(PhysKey::ControlRight);
    wait_layout(LayoutId(0), "switch key in window B");
    first.activate(&session);
    wait_layout(LayoutId(0), "single layout applied to window A");
    first.clear_first();
    running.type_text("f");
    first.expect_first("f", &temp);
    drop(running);
    drop((first, second));
    eprintln!("single layout for all windows accepted over a per-window session policy");
}

#[test]
#[ignore = "requires root/uinput, private GNOME/KDE Wayland, Writer/UNO and foot; set OKBS_TEST_APP=native in the device test script"]
fn real_wayland_writer_and_terminal_correct_paste_and_submit() {
    assert!(
        crate::permissions::is_root(),
        "run only in an isolated development VM"
    );
    // Engine decisions in the test log; typed text is never logged.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .try_init();
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = workspace.join("tmp/linux-native-input");
    std::fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::TempDir::new_in(&scratch).unwrap().keep();
    let state = temp.join("state.json");
    let command = temp.join("command.json");
    let session = Session::new();
    let log = std::fs::File::create(temp.join("writer.log")).unwrap();
    let writer = Process(
        std::process::Command::new("python3")
            .arg(workspace.join("tools/linux-writer-fixture.py"))
            .args([&state, &command])
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let page = || -> serde_json::Value {
        std::fs::read(&state)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(serde_json::Value::Null)
    };
    let prepare = |document: &str, text: &str, select_all: bool, token: u32| {
        let temporary = command.with_extension("new");
        std::fs::write(&temporary, serde_json::json!({"document":document,"text":text,"select_all":select_all,"token":token}).to_string()).unwrap();
        std::fs::rename(temporary, &command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(25);
        let mut registered = false;
        let mut clicked = false;
        let mut stable = None;
        let mut previous = None;
        loop {
            let native = session.desktop.refresh().unwrap();
            let value = page();
            if value["acknowledgement"] == token
                && let (Some(pid), Some(title)) =
                    (value["pid"].as_u64(), value["titles"][document].as_str())
            {
                if !registered {
                    session
                        .desktop
                        .place_fixture_window(pid as u32, title, [40, 40])
                        .unwrap();
                    registered = true;
                }
                if let Some(window) = session.desktop.fixture_window(pid as u32, title).unwrap()
                    && native.window != window.window
                {
                    session
                        .desktop
                        .command("ActivateWindow", window.window)
                        .unwrap();
                }
            }
            if value["acknowledgement"] == token
                && value["pid"] == native.pid
                && native.title.contains(&format!("okbs-{document}"))
            {
                if native.control != 0 {
                    let current = (native.window, native.control);
                    if previous == Some(current) {
                        let since = stable.get_or_insert_with(Instant::now);
                        if since.elapsed() >= Duration::from_millis(400) {
                            break;
                        }
                    } else {
                        stable = None;
                    }
                    previous = Some(current);
                } else {
                    stable = None;
                    previous = None;
                }
                if native.control == 0 && session.remote.is_none() && !clicked {
                    let [x, y, w, h] = native.client;
                    assert!(
                        std::process::Command::new(
                            workspace.join("tmp/linux-kde-keyboard/keyboard-fixture")
                        )
                        .args([
                            "--click",
                            &(x + w / 2).to_string(),
                            &(y + h / 2).to_string()
                        ])
                        .status()
                        .unwrap()
                        .success()
                    );
                    clicked = true;
                }
            }
            assert!(
                Instant::now() < deadline,
                "Writer document did not become the input target: {native:?}, state {value:?}; scratch {}",
                temp.display()
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let expect = |document: &str, text: &str| {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let value = page();
            if value["values"][document] == text {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Writer {document}: expected {text:?}, state {value:?}; scratch {}",
                temp.display()
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    prepare("first", "", false, 1);
    let mut running = RunningEngine::new(&session);
    let mut clipboard = LinuxClipboard::new().unwrap();
    clipboard.set_text("synthetic preserved clipboard").unwrap();
    running.type_text("Github b linux windows plan b ");
    expect("first", "Github и linux windows plan b ");
    prepare("first", "ghbdtn", true, 2);
    running.chord(&[PhysKey::ShiftLeft], PhysKey::Pause);
    expect("first", "привет");
    let deadline = Instant::now() + Duration::from_secs(5);
    while clipboard.text().unwrap().as_deref() != Some("synthetic preserved clipboard") {
        assert!(
            Instant::now() < deadline,
            "Writer selection did not restore clipboard"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    prepare("second", "", false, 3);
    assert!(running.engine.send(Command::InsertText {
        text: "история буфера\nsecond synthetic line".into(),
        target: session.desktop.input_target().unwrap()
    }));
    expect("second", "история буфера\nsecond synthetic line");
    expect("first", "привет");
    let deadline = Instant::now() + Duration::from_secs(5);
    while clipboard.text().unwrap().as_deref() != Some("synthetic preserved clipboard") {
        assert!(
            Instant::now() < deadline,
            "Writer paste did not restore clipboard"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let config_path = temp.join("native-config.toml");
    let mut changed = running.config.clone();
    changed.general.autoswitch = false;
    changed.autoreplace.enabled = true;
    changed.autoreplace.also_in_other_layout = true;
    changed.hotkeys.show_autoreplace_menu = HotkeyBinding::some("Ctrl+F12".parse().unwrap());
    let item = AutoReplaceItem {
        from: "sig".into(),
        to: "Привет,\n<name>".into(),
        cursor_pos: 9,
    };
    changed.autoreplace.items = vec![item.clone()];
    let mut token = 10;
    for layout in [LayoutId(0), LayoutId(1)] {
        for (trigger, key) in [
            (AutoReplaceTrigger::Space, PhysKey::Space),
            (AutoReplaceTrigger::Enter, PhysKey::Enter),
            (AutoReplaceTrigger::Enter, PhysKey::NumpadEnter),
            (AutoReplaceTrigger::Tab, PhysKey::Tab),
            (AutoReplaceTrigger::Tooltip, PhysKey::Enter),
            (AutoReplaceTrigger::Tooltip, PhysKey::Tab),
            (AutoReplaceTrigger::Hotkey, PhysKey::F12),
        ] {
            changed.autoreplace.trigger = trigger;
            running.configure(changed.clone(), &config_path);
            token += 1;
            prepare("first", "", false, token);
            session.desktop.clone().set(layout).unwrap();
            running.type_text("sig");
            let abbreviation_target = session.desktop.input_target().unwrap();
            if trigger == AutoReplaceTrigger::Tooltip {
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    if running
                        .engine
                        .events()
                        .recv_timeout(Duration::from_millis(100))
                        .is_ok_and(|event| event == Event::AutoreplaceHint(Some(0)))
                    {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "Writer tooltip candidate missing"
                    );
                }
            }
            running.chord(
                if trigger == AutoReplaceTrigger::Hotkey {
                    &[PhysKey::ControlLeft]
                } else {
                    &[]
                },
                key,
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut feedback = Vec::new();
            loop {
                if let Ok(event) = running
                    .engine
                    .events()
                    .recv_timeout(Duration::from_millis(100))
                {
                    if event == Event::Autoreplaced {
                        break;
                    }
                    feedback.push(event);
                }
                assert!(
                    Instant::now() < deadline,
                    "Writer {trigger:?}/{layout:?} did not expand: {feedback:?}, desktop {:?}",
                    session.desktop.cached()
                );
            }
            let suffix = if trigger == AutoReplaceTrigger::Space {
                " "
            } else {
                ""
            };
            let expected = format!("{}{suffix}", item.to);
            expect("first", &expected);
            assert_eq!(
                session.desktop.input_target().unwrap(),
                abbreviation_target,
                "a multiline expansion must retain the same editor target"
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while page()["cursor"]["first"] != 9
                || clipboard.text().unwrap().as_deref() != Some("synthetic preserved clipboard")
            {
                assert!(
                    Instant::now() < deadline,
                    "Writer {trigger:?}/{layout:?} cursor/clipboard not restored: {:?}",
                    page()
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(session.desktop.current().unwrap(), layout);
            running.type_text("x");
            let marker = if layout == LayoutId(0) { "x" } else { "ч" };
            expect("first", &format!("Привет,\n<{marker}name>{suffix}"));
        }
    }
    // Escape dismisses a tooltip, and its Enter must reach the editor normally.
    changed.autoreplace.trigger = AutoReplaceTrigger::Tooltip;
    running.configure(changed.clone(), &config_path);
    token += 1;
    prepare("first", "", false, token);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    running.type_text("sig");
    running.chord(&[], PhysKey::Escape);
    running.chord(&[], PhysKey::Enter);
    // Writer's own sentence capitalization still applies after dismissal.
    expect("first", "Sig\n");
    // Disable the feature live and verify the delimiter remains ordinary input.
    changed.autoreplace.enabled = false;
    changed.autoreplace.trigger = AutoReplaceTrigger::Space;
    running.configure(changed.clone(), &config_path);
    token += 1;
    prepare("first", "", false, token);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    running.type_text("sig ");
    expect("first", "Sig ");
    // The other-layout option must also take effect through a live update.
    changed.autoreplace.enabled = true;
    changed.autoreplace.also_in_other_layout = false;
    running.configure(changed.clone(), &config_path);
    token += 1;
    prepare("first", "prefix ", false, token);
    session.desktop.clone().set(LayoutId(1)).unwrap();
    running.type_text("sig ");
    expect("first", "prefix ышп ");
    // An explicit menu/list insertion uses the configured entry and its end
    // position; it follows the production command path without a text mock.
    changed.autoreplace.also_in_other_layout = true;
    changed.autoreplace.items[0].cursor_pos = -1;
    running.configure(changed.clone(), &config_path);
    token += 1;
    prepare("first", "", false, token);
    assert!(running.engine.send(Command::InsertAutoreplace {
        item: changed.autoreplace.items[0].clone(),
        target: session.desktop.input_target().unwrap()
    }));
    expect("first", &item.to);
    let deadline = Instant::now() + Duration::from_secs(5);
    while page()["cursor"]["first"] != item.to.chars().count()
        || clipboard.text().unwrap().as_deref() != Some("synthetic preserved clipboard")
    {
        assert!(
            Instant::now() < deadline,
            "explicit Writer insertion did not preserve end position/clipboard"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Break undoes the automatic conversion; it can also convert an unfinished word.
    changed.general.autoswitch = true;
    changed.autoreplace.enabled = false;
    running.configure(changed.clone(), &config_path);
    token += 1;
    // Keep Writer's independent sentence-capitalization outside the undo
    // comparison; the program should undo only the word it converted.
    prepare("first", "prefix ", false, token);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    running.type_text("ghbdtn ");
    expect("first", "prefix привет ");
    running.chord(&[], PhysKey::Pause);
    expect("first", "prefix ghbdtn ");
    assert_eq!(session.desktop.current().unwrap(), LayoutId(0));
    token += 1;
    prepare("first", "", false, token);
    changed.general.autoswitch = false;
    running.configure(changed.clone(), &config_path);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    running.type_text("ghbdtn");
    running.chord(&[], PhysKey::Pause);
    expect("first", "привет");
    running.chord(&[], PhysKey::Pause);
    expect("first", "ghbdtn");
    // Excluded programs, identified from the compositor's window PID the way
    // the application does: /proc/<pid>/exe, its folder, title and app_id.
    let writer_pid = page()["pid"].as_u64().expect("Writer fixture pid");
    let executable = std::fs::read_link(format!("/proc/{writer_pid}/exe")).unwrap();
    let app_id = session.desktop.cached().app_id;
    eprintln!("Writer exclusions use {executable:?}, app_id {app_id:?}");
    // A converted word passes through the typed text first: wait past the
    // conversion before accepting the unchanged value.
    let expect_stable = |document: &str, text: &str| {
        expect(document, text);
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(
            page()["values"][document],
            text,
            "{document} changed after it settled; scratch {}",
            temp.display()
        );
    };
    let excluded = |kind: &str| {
        let mut exclusions = Exclusions::default();
        let value = match kind {
            "name" => executable
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            "path" => executable.to_string_lossy().into_owned(),
            "app_id" => app_id.clone(),
            "folder" => executable.parent().unwrap().to_string_lossy().into_owned(),
            _ => "okbs-first".into(),
        };
        match kind {
            "folder" => exclusions.folders.push(FolderExclusion { path: value }),
            "title" => exclusions.titles.push(TitleExclusion { contains: value }),
            _ => exclusions
                .executables
                .push(ExecutableExclusion { path: value }),
        }
        exclusions
    };
    changed.general.autoswitch = true;
    let mut kinds = vec!["name", "path", "folder", "title"];
    if !app_id.is_empty() && !app_id.contains('/') {
        kinds.push("app_id");
    }
    for kind in kinds {
        changed.exclusions = excluded(kind);
        running.configure(changed.clone(), &config_path);
        assert_eq!(running.config.exclusions, changed.exclusions);
        token += 1;
        prepare("first", "prefix ", false, token);
        session.desktop.clone().set(LayoutId(0)).unwrap();
        running.type_text("ghbdtn ");
        expect_stable("first", "prefix ghbdtn ");
        assert_eq!(
            session.desktop.current().unwrap(),
            LayoutId(0),
            "layout switched in a program excluded by {kind}"
        );
    }
    // A manual conversion still works in an excluded program, unless the
    // program is to be left alone completely.
    changed.exclusions = excluded("title");
    for ignore in [false, true] {
        changed.troubleshooting.ignore_excluded_apps_completely = ignore;
        running.configure(changed.clone(), &config_path);
        token += 1;
        prepare("first", "", false, token);
        session.desktop.clone().set(LayoutId(0)).unwrap();
        running.type_text("ghbdtn");
        running.chord(&[], PhysKey::Pause);
        if ignore {
            expect_stable("first", "ghbdtn");
        } else {
            expect("first", "привет");
        }
    }
    // Removing the exclusion live restores automatic switching.
    changed.exclusions = Exclusions::default();
    changed.troubleshooting.ignore_excluded_apps_completely = false;
    running.configure(changed.clone(), &config_path);
    token += 1;
    prepare("first", "prefix ", false, token);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    running.type_text("ghbdtn ");
    expect("first", "prefix привет ");
    eprintln!("Writer exclusions by name/path/folder/title/app_id and manual-only mode accepted");
    // Hotkey actions through the physical evdev chord, the input filter and
    // the engine. Expected texts come from the shared text functions; this
    // checks the Linux selection, clipboard and window paths, not the algorithms.
    let wait_event = |running: &RunningEngine, wanted: &Event| {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        loop {
            if let Ok(event) = running
                .engine
                .events()
                .recv_timeout(Duration::from_millis(100))
            {
                if &event == wanted {
                    break;
                }
                seen.push(event);
            }
            assert!(
                Instant::now() < deadline,
                "missing {wanted:?}, got {seen:?}"
            );
        }
    };
    let wait_clipboard = |clipboard: &mut LinuxClipboard, text: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while clipboard.text().unwrap().as_deref() != Some(text) {
            assert!(Instant::now() < deadline, "clipboard is not {text:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let bind = |chord: &str| HotkeyBinding::some(chord.parse().unwrap());
    let ctrl_shift = [PhysKey::ControlLeft, PhysKey::ShiftLeft];
    // Writer handles many Ctrl+Shift+F-key chords itself (F5 opens the
    // Navigator), which matters while hotkeys rest: use Scroll Lock there.
    changed.hotkeys.invert_selection_case = bind("Ctrl+Shift+ScrollLock");
    changed.hotkeys.transliterate_selection = bind("Ctrl+Shift+F6");
    changed.hotkeys.number_to_words = bind("Ctrl+Shift+F7");
    changed.hotkeys.paste_plain = bind("Ctrl+Shift+F8");
    changed.hotkeys.convert_clipboard_layout = bind("Ctrl+Shift+F9");
    changed.hotkeys.transliterate_clipboard = bind("Ctrl+Shift+F10");
    changed.hotkeys.toggle_autoswitch = bind("Ctrl+Shift+F11");
    changed.general.hotkeys_off_when_autoswitch_off = true;
    running.configure(changed.clone(), &config_path);
    assert_eq!(running.config.hotkeys, changed.hotkeys);
    for (key, text, expected) in [
        (
            PhysKey::ScrollLock,
            "Linux Mint",
            okbs_core::text::invert_case("Linux Mint"),
        ),
        (
            PhysKey::F6,
            "привет мир",
            okbs_core::translit::transliterate("привет мир"),
        ),
        (
            PhysKey::F7,
            "123",
            okbs_core::numwords::convert("123").expect("number in words"),
        ),
    ] {
        assert_ne!(text, expected);
        token += 1;
        prepare("first", text, true, token);
        running.chord(&ctrl_shift, key);
        expect("first", &expected);
        wait_clipboard(&mut clipboard, "synthetic preserved clipboard");
    }
    token += 1;
    prepare("first", "prefix ", false, token);
    clipboard.set_text("plain synthetic text").unwrap();
    running.chord(&ctrl_shift, PhysKey::F8);
    expect("first", "prefix plain synthetic text");
    for (key, text, expected) in [
        (
            PhysKey::F9,
            "ghbdtn",
            okbs_core::layouts::switch_text_layout("ghbdtn"),
        ),
        (
            PhysKey::F10,
            "привет",
            okbs_core::translit::transliterate("привет"),
        ),
    ] {
        token += 1;
        prepare("first", "unchanged ", false, token);
        clipboard.set_text(text).unwrap();
        running.chord(&ctrl_shift, key);
        wait_clipboard(&mut clipboard, &expected);
        expect_stable("first", "unchanged ");
    }
    // With autoswitch off, other hotkeys rest except the toggle itself.
    running.chord(&ctrl_shift, PhysKey::F11);
    wait_event(&running, &Event::AutoswitchChanged(false));
    token += 1;
    prepare("first", "prefix ", false, token);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    running.type_text("ghbdtn ");
    expect_stable("first", "prefix ghbdtn ");
    token += 1;
    prepare("first", "Linux", true, token);
    running.chord(&ctrl_shift, PhysKey::ScrollLock);
    expect_stable("first", "Linux");
    running.chord(&ctrl_shift, PhysKey::F11);
    wait_event(&running, &Event::AutoswitchChanged(true));
    token += 1;
    prepare("first", "prefix ", false, token);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    running.type_text("ghbdtn ");
    expect("first", "prefix привет ");
    eprintln!("Writer hotkey selection/clipboard/paste/autoswitch actions accepted");
    eprintln!(
        "Writer autoreplace all triggers/EN-RU/cursor, live config serialization, Escape and Pause accepted"
    );
    drop(running);
    drop(writer);

    let submitted = temp.join("submitted.txt");
    let log = std::fs::File::create(temp.join("foot.log")).unwrap();
    let terminal = Process(std::process::Command::new("foot").args(["--title=OKBS Wayland terminal acceptance", "--app-id=foot", "bash", "--noprofile", "--norc", "-c",
        r#"IFS= read -r line; printf '%s\n' "$line" > "$1"; IFS= read -r pasted; printf '%s\n' "$pasted" >> "$1"; sleep 30"#, "bash"])
        .arg(&submitted).stdout(log.try_clone().unwrap()).stderr(log).spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let native = session.desktop.refresh().unwrap();
        if native.pid == terminal.0.id()
            && native.title.contains("OKBS Wayland terminal acceptance")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native Wayland terminal unavailable: {native:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(session.desktop.is_terminal().unwrap());
    let mut running = RunningEngine::new(&session);
    running.type_text("Github b linux\n");
    let deadline = Instant::now() + Duration::from_secs(8);
    while std::fs::read_to_string(&submitted).ok().as_deref() != Some("Github и linux\n") {
        assert!(
            Instant::now() < deadline,
            "foot did not submit the corrected line before Enter: {:?}",
            std::fs::read_to_string(&submitted)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(running.engine.send(Command::InsertText {
        text: "synthetic terminal paste".into(),
        target: session.desktop.input_target().unwrap()
    }));
    std::thread::sleep(Duration::from_millis(300));
    running.type_text("\n");
    let deadline = Instant::now() + Duration::from_secs(8);
    while std::fs::read_to_string(&submitted).ok().as_deref()
        != Some("Github и linux\nsynthetic terminal paste\n")
    {
        assert!(
            Instant::now() < deadline,
            "foot history paste or terminal Ctrl+Shift+V failed: {:?}",
            std::fs::read_to_string(&submitted)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Window commands go to the compositor integration for the active
    // window; foot opens with an ordinary, not screen-filling size.
    let mut window_hotkeys = running.config.clone();
    window_hotkeys.hotkeys.toggle_maximize_window = bind("Ctrl+Shift+Insert");
    window_hotkeys.hotkeys.minimize_window = bind("Ctrl+Shift+Home");
    running.configure(window_hotkeys, &config_path);
    let terminal_window = session.desktop.refresh().unwrap();
    running.chord(&ctrl_shift, PhysKey::Insert);
    wait_event(
        &running,
        &Event::Ui(okbs_engine::UiRequest::ToggleMaximizeWindow),
    );
    okbs_platform::WindowControl::toggle_maximize_active(&session.desktop).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let maximized = loop {
        let native = session.desktop.refresh().unwrap();
        if native.window == terminal_window.window
            && native.client[2] > terminal_window.client[2]
            && native.client[3] > terminal_window.client[3]
        {
            break native;
        }
        assert!(
            Instant::now() < deadline,
            "foot was not maximized from {terminal_window:?}: {native:?}"
        );
        std::thread::sleep(Duration::from_millis(30));
    };
    running.chord(&ctrl_shift, PhysKey::Insert);
    wait_event(
        &running,
        &Event::Ui(okbs_engine::UiRequest::ToggleMaximizeWindow),
    );
    okbs_platform::WindowControl::toggle_maximize_active(&session.desktop).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let native = session.desktop.refresh().unwrap();
        if native.window == terminal_window.window && native.client[2] < maximized.client[2] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "foot was not restored: {native:?}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    running.chord(&ctrl_shift, PhysKey::Home);
    wait_event(&running, &Event::Ui(okbs_engine::UiRequest::MinimizeWindow));
    okbs_platform::WindowControl::minimize_active(&session.desktop).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.desktop.refresh().unwrap().window == terminal_window.window {
        assert!(Instant::now() < deadline, "foot was not minimized");
        std::thread::sleep(Duration::from_millis(30));
    }
    eprintln!("foot maximize/restore/minimize hotkeys accepted");
    drop(running);
    drop(terminal);

    // Caps Lock and layout keys handled by the Linux input filter and the
    // engine, in a plain GTK field without application autocorrection.
    let editor = FieldWindow::open(&session, &workspace, &temp, "OKBS two-field acceptance");
    editor.activate(&session);
    let mut running = RunningEngine::new(&session);
    let field = || editor.first();
    let expect_field = |text: &str| editor.expect_first(text, &temp);
    let expect_field_stable = |text: &str| {
        expect_field(text);
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(field(), text, "GTK field changed after it settled");
    };
    let clear = || editor.clear_first();
    let mut options = running.config.clone();
    options.general.autoswitch = false;
    options.advanced.fix_accidental_capslock = false;
    running.configure(options.clone(), &config_path);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    // Baseline: the private session itself applies Caps Lock.
    clear();
    running.tap(PhysKey::CapsLock);
    running.type_text("abc");
    expect_field("ABC");
    running.tap(PhysKey::CapsLock);
    running.type_text("d");
    expect_field("ABCd");
    // «Отключить Caps Lock»: the key never reaches the session.
    options.advanced.disable_capslock = true;
    running.configure(options.clone(), &config_path);
    clear();
    running.tap(PhysKey::CapsLock);
    running.type_text("abc");
    expect_field_stable("abc");
    // Caps Lock as the layout switch key, without toggling capitals.
    options.advanced.disable_capslock = false;
    options.switching.switch_key = okbs_core::config::SwitchKey::CapsLock;
    running.configure(options.clone(), &config_path);
    clear();
    running.tap(PhysKey::CapsLock);
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.desktop.current().unwrap() != LayoutId(1) {
        assert!(Instant::now() < deadline, "Caps Lock did not switch layout");
        std::thread::sleep(Duration::from_millis(20));
    }
    running.type_text("f");
    expect_field("а");
    running.tap(PhysKey::CapsLock);
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.desktop.current().unwrap() != LayoutId(0) {
        assert!(
            Instant::now() < deadline,
            "Caps Lock did not switch layout back"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    running.type_text("f");
    expect_field("аf");
    // Scroll Lock works as Caps Lock.
    options.switching.switch_key = okbs_core::config::SwitchKey::None;
    options.advanced.scrolllock_as_capslock = true;
    running.configure(options.clone(), &config_path);
    clear();
    running.tap(PhysKey::ScrollLock);
    running.type_text("abc");
    expect_field("ABC");
    running.tap(PhysKey::ScrollLock);
    running.type_text("d");
    expect_field("ABCd");
    // Direct layout keys: left Shift selects Russian, right Shift English.
    options.advanced.scrolllock_as_capslock = false;
    options.switching.direct_keys.enabled = true;
    options.switching.direct_keys.ru = okbs_core::config::SwitchKey::LeftShift;
    options.switching.direct_keys.en = okbs_core::config::SwitchKey::RightShift;
    running.configure(options.clone(), &config_path);
    for (key, layout) in [
        (PhysKey::ShiftLeft, LayoutId(1)),
        (PhysKey::ShiftLeft, LayoutId(1)),
        (PhysKey::ShiftRight, LayoutId(0)),
    ] {
        running.tap(key);
        let deadline = Instant::now() + Duration::from_secs(5);
        while session.desktop.current().unwrap() != layout {
            assert!(
                Instant::now() < deadline,
                "{key:?} did not select {layout:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    // An accidental Caps Lock word is fixed and Caps Lock is turned off.
    options.switching.direct_keys.enabled = false;
    options.general.autoswitch = true;
    options.advanced.fix_accidental_capslock = true;
    running.configure(options.clone(), &config_path);
    clear();
    running.tap(PhysKey::CapsLock);
    running.type_text("Hello ");
    expect_field("Hello ");
    running.type_text("x");
    expect_field("Hello x");
    drop(running);
    drop(editor);
    eprintln!("GTK Caps Lock/switch key/Scroll Lock/direct keys/accidental Caps Lock accepted");
    eprintln!(
        "Native Wayland Writer selection/correction/history and foot correction-before-Enter/history accepted"
    );
}

#[test]
#[ignore = "requires private GNOME/KDE Wayland with pointer fixtures; set OKBS_TEST_APP=indicator in the device test script"]
fn real_floating_indicator_pointer_actions() {
    use crate::panels::{LinuxIndicator, Panels};
    use okbs_platform::{FloatingIndicator, IndicatorEvent, IndicatorLabels, IndicatorState};
    let mut session = Session::new();
    let panels = Panels::new(session.desktop.clone()).unwrap();
    let mut state = IndicatorState {
        visible: true,
        autohide: false,
        autohide_ms: 300,
        position: Some([300, 300]),
        locked: false,
    };
    let labels = IndicatorLabels {
        title: "OKBS indicator acceptance".into(),
        lock: "Lock".into(),
        settings: "Settings".into(),
        hide: "Hide".into(),
    };
    let mut indicator = LinuxIndicator::new(panels.clone(), &state, labels.clone()).unwrap();
    let size = 32;
    let icon: Vec<u8> = (0..size * size).flat_map(|_| [200, 40, 40, 255]).collect();
    indicator.set_icon(&icon, size, false).unwrap();
    // Events during `duration`; the GTK panels are pumped by `poll`.
    let events = |indicator: &mut LinuxIndicator, duration: Duration| {
        let deadline = Instant::now() + duration;
        let mut events = Vec::new();
        while Instant::now() < deadline {
            if let Some(event) = indicator.poll() {
                events.push(event);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        events
    };
    let geometry = |indicator: &mut LinuxIndicator| {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut previous = None;
        loop {
            let _ = events(indicator, Duration::from_millis(100));
            let current = panels.geometry("indicator").unwrap();
            if let Some(stable) = current
                && current == previous
            {
                break stable;
            }
            previous = current;
            assert!(Instant::now() < deadline, "indicator was not mapped");
        }
    };
    let center = |[x, y, width, height]: [i32; 4]| [x + width / 2, y + height / 2];
    let start = geometry(&mut indicator);
    // A double click asks for the settings window.
    session.pointer(Pointer::DoubleClick(center(start)));
    let seen = events(&mut indicator, Duration::from_secs(2));
    assert!(
        seen.contains(&IndicatorEvent::OpenSettings),
        "double click: {seen:?}"
    );
    // Dragging reports the new position, which the program saves.
    let delta = [120, 80];
    let from = center(start);
    session.pointer(Pointer::Drag(
        from,
        [from[0] + delta[0], from[1] + delta[1]],
    ));
    let seen = events(&mut indicator, Duration::from_secs(2));
    let moved = seen
        .iter()
        .rev()
        .find_map(|event| match event {
            IndicatorEvent::Moved(position) => Some(*position),
            _ => None,
        })
        .unwrap_or_else(|| panic!("drag reported no position: {seen:?}"));
    let expected = [start[0] + delta[0], start[1] + delta[1]];
    assert!(
        (moved[0] - expected[0]).abs() <= 4 && (moved[1] - expected[1]).abs() <= 4,
        "dragged to {moved:?}, expected about {expected:?}"
    );
    let dragged = geometry(&mut indicator);
    assert!(
        (dragged[0] - expected[0]).abs() <= 4 && (dragged[1] - expected[1]).abs() <= 4,
        "indicator at {dragged:?}, expected about {expected:?}"
    );
    // «Закрепить индикатор»: a drag no longer moves it.
    state.position = Some(moved);
    state.locked = true;
    indicator.configure(&state, labels.clone()).unwrap();
    let locked = geometry(&mut indicator);
    let from = center(locked);
    session.pointer(Pointer::Drag(from, [from[0] - 100, from[1] - 60]));
    let seen = events(&mut indicator, Duration::from_secs(2));
    assert!(
        !seen
            .iter()
            .any(|event| matches!(event, IndicatorEvent::Moved(p) if *p != moved)),
        "a locked indicator was moved: {seen:?}"
    );
    let after = geometry(&mut indicator);
    assert!(
        (after[0] - locked[0]).abs() <= 1 && (after[1] - locked[1]).abs() <= 1,
        "locked indicator moved from {locked:?} to {after:?}"
    );
    // Autohide shows it only for a while after a layout change.
    state.autohide = true;
    indicator.configure(&state, labels).unwrap();
    let _ = events(&mut indicator, Duration::from_millis(100));
    assert!(!panels.visible("indicator"), "autohide keeps it hidden");
    indicator.set_icon(&icon, size, true).unwrap();
    assert!(panels.visible("indicator"), "shown after a layout change");
    let _ = events(&mut indicator, Duration::from_millis(600));
    assert!(
        !panels.visible("indicator"),
        "hidden again after autohide_ms"
    );
    eprintln!("floating indicator double click/drag/lock/autohide accepted");
}
