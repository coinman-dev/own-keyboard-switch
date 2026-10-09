//! Visible Writer and a real shell inside a native Wayland terminal.
use crate::{
    desktop::LinuxDesktop, input::LinuxSource, services::LinuxClipboard, session::Desktop,
    wayland_input_acceptance::KeyboardDriver,
};
use evdev::{AttributeSet, KeyCode, uinput::VirtualDevice};
use okbs_core::{
    PhysKey,
    config::{AutoReplaceItem, AutoReplaceTrigger, Config, HotkeyBinding},
};
use okbs_engine::{Backends, Command, EngineHandle, Event, Inputs, Processor};
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
    fn configure(&mut self, config: Config, path: &std::path::Path) {
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
    running.configure(changed, &config_path);
    session.desktop.clone().set(LayoutId(0)).unwrap();
    running.type_text("ghbdtn");
    running.chord(&[], PhysKey::Pause);
    expect("first", "привет");
    running.chord(&[], PhysKey::Pause);
    expect("first", "ghbdtn");
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
    drop(running);
    drop(terminal);
    eprintln!(
        "Native Wayland Writer selection/correction/history and foot correction-before-Enter/history accepted"
    );
}
