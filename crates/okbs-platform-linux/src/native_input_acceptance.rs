//! Visible Writer and a real shell inside a native Wayland terminal.
use crate::{
    desktop::LinuxDesktop, input::LinuxSource, services::LinuxClipboard, session::Desktop,
    wayland_input_acceptance::KeyboardDriver,
};
use evdev::{AttributeSet, KeyCode, uinput::VirtualDevice};
use okbs_core::{PhysKey, config::Config};
use okbs_engine::{Backends, Command, EngineHandle, Inputs, Processor};
use okbs_platform::{Clipboard, FocusInfo, KeyboardSource, LayoutId, LayoutManager, StopGuard};
use std::{
    path::PathBuf,
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
    _keyboard: Option<Process>,
}
impl Session {
    fn new() -> Self {
        let kde = crate::SessionInfo::detect().desktop == Desktop::Kde;
        let bridge = kde.then(|| crate::desktop::serve_kde_bridge().unwrap());
        let keyboard = kde.then(|| {
            Process(
                std::process::Command::new(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tmp/linux-kde-keyboard/keyboard-fixture"),
                )
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
            )
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
            _keyboard: keyboard,
        }
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
            config,
            Backends {
                injector: Box::new(injector),
                layouts: Box::new(session.desktop.clone()),
                clipboard: Some(Box::new(LinuxClipboard::new().unwrap())),
                sound: None,
                focus: Some(Box::new(session.desktop.clone())),
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
        let capture = source.start(sink).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        Self {
            engine,
            _capture: capture,
            _focus: focus,
            driver,
            hardware,
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
}

#[test]
#[ignore = "requires root/uinput, private GNOME/KDE Wayland, Writer/UNO and foot; set OKBS_TEST_APP=native in the device test script"]
fn real_wayland_writer_and_terminal_correct_paste_and_submit() {
    assert!(
        crate::permissions::is_root(),
        "run only in an isolated development VM"
    );
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
                    break;
                }
                if session.remote.is_none() && !clicked {
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
    drop(running);
    drop(terminal);
    eprintln!(
        "Native Wayland Writer selection/correction/history and foot correction-before-Enter/history accepted"
    );
}
