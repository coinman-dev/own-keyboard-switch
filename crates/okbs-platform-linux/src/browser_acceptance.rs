//! Browser accessibility acceptance against the official Firefox binary.
use crate::{desktop::LinuxDesktop, session::Desktop};
use okbs_platform::FocusInfo;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn check_real_input(
    workspace: &std::path::Path,
    state: &std::path::Path,
    command: &std::path::Path,
    desktop: &LinuxDesktop,
    remote: Option<&zbus::zvariant::OwnedObjectPath>,
    bus: &zbus::blocking::Connection,
) {
    use crate::{input::LinuxSource, services::LinuxClipboard};
    use evdev::{AttributeSet, KeyCode, uinput::VirtualDevice};
    use okbs_core::{PhysKey, config::Config};
    use okbs_engine::{Backends, Command as EngineCommand, EngineHandle, Event, Inputs, Processor};
    use okbs_platform::{Clipboard, KeyboardSource, LayoutId, LayoutManager};

    let page = || -> serde_json::Value {
        std::fs::read(state)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(serde_json::Value::Null)
    };
    let prepare = |field: &str, token: u32| {
        let previous = desktop.cached().control;
        let changing = page()["focus"] != field;
        let temporary = command.with_extension("new");
        std::fs::write(
            &temporary,
            serde_json::json!({"field":field,"text":"","offset":0,"token":token}).to_string(),
        )
        .unwrap();
        std::fs::rename(temporary, command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            desktop.refresh().unwrap();
            let value = page();
            if value["acknowledgement"] == token
                && value["documentFocus"] == true
                && value["visibility"] == "visible"
                && value["focus"] == field
                && desktop.cached().control != 0
                && (!changing || desktop.cached().control != previous)
                && desktop.is_password_field().unwrap() == Some(field == "password")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "synthetic field preparation failed: {value:?}"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let expect_text = |field: &str, expected: &str| {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let value = page();
            if value["values"][field] == expected {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Firefox {field}: expected {expected:?}, page {value:?}; scratch {}",
                state.display()
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    prepare("first", 1);
    let mut keys = AttributeSet::new();
    for &key in PhysKey::ALL {
        keys.insert(KeyCode(key.evdev_code()));
    }
    let mut hardware = VirtualDevice::builder()
        .unwrap()
        .name("OKBS browser fixture keyboard")
        .with_keys(&keys)
        .unwrap()
        .build()
        .unwrap();
    let path = crate::acceptance::virtual_device_node(&mut hardware);
    let mut config = Config::default();
    config.linux.devices = vec![path.to_string_lossy().into_owned()];
    config.general.passwords_to_english = false;
    config.switching.inject_key_delay_ms = 4;
    config.spellcheck.check_typed_words = true;
    let (mut source, injector) = LinuxSource::new(&config, desktop.clone()).unwrap();
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
            "test output keyboard has no node"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let driver = crate::wayland_input_acceptance::KeyboardDriver::start(
        output,
        remote.map(|path| (path.clone(), bus.clone())),
    );
    desktop.clone().set(LayoutId(1)).unwrap();
    let mut clipboard = LinuxClipboard::new().unwrap();
    let saved_clipboard = "synthetic clipboard preserved";
    clipboard.set_text(saved_clipboard).unwrap();
    let (sink, input) = crossbeam_channel::unbounded();
    let (focus_sink, focus_input) = crossbeam_channel::unbounded();
    let focus_watch = FocusInfo::subscribe(&mut desktop.clone(), focus_sink).unwrap();
    let mut processor = Processor::new(
        config,
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
    // Source startup enables menu queries and adds a native virtual keyboard.
    // Wait for a settled editable target/clipboard before the first test key;
    // a transient control during initialization must not be mistaken for a
    // ready browser field by this test-only delivery bridge.
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut stable = None;
    let mut previous = None;
    loop {
        desktop.refresh().unwrap();
        let target = desktop.input_target().unwrap();
        let ready = target.is_some_and(|target| target.control != 0)
            && clipboard.text().unwrap().as_deref() == Some(saved_clipboard)
            && desktop.current().unwrap() == LayoutId(1);
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
            "Firefox input target did not settle at device startup: {:?}",
            desktop.cached()
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    driver.type_text(&mut hardware, "Github b linux windows plan b ");
    expect_text("first", "Github и linux windows plan b ");
    let target = desktop.input_target().unwrap().unwrap();
    driver.type_text(&mut hardware, "wrold ");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if engine
            .events()
            .recv_timeout(Duration::from_millis(100))
            .is_ok_and(
                |event| matches!(event, Event::CheckSpelling { text, .. } if text == "wrold"),
            )
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Firefox typed-word spelling snapshot unavailable"
        );
    }
    assert!(engine.send(EngineCommand::ReplaceTypedSpelling {
        request_id: 101,
        target,
        original: "wrold".into(),
        corrected: "world".into()
    }));
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(Event::SpellingReplacementFinished {
            request_id: 101,
            success,
        }) = engine.events().recv_timeout(Duration::from_millis(100))
        {
            assert!(success, "Firefox valid spelling replacement failed");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Firefox spelling replacement acknowledgement missing"
        );
    }
    expect_text("first", "Github и linux windows plan b world ");
    prepare("second", 2);
    let deadline = Instant::now() + Duration::from_secs(8);
    while desktop
        .input_target()
        .unwrap()
        .is_none_or(|current| current.control == target.control)
    {
        assert!(
            Instant::now() < deadline,
            "Firefox textarea target stayed stale"
        );
        desktop.refresh().unwrap();
    }
    assert!(engine.send(EngineCommand::ReplaceTypedSpelling {
        request_id: 102,
        target,
        original: "wrold".into(),
        corrected: "world".into()
    }));
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(Event::SpellingReplacementFinished {
            request_id: 102,
            success,
        }) = engine.events().recv_timeout(Duration::from_millis(100))
        {
            assert!(!success, "Firefox stale popup changed another field");
            break;
        }
        assert!(Instant::now() < deadline);
    }
    expect_text("first", "Github и linux windows plan b world ");
    expect_text("second", "");
    if let Some(remote) = remote {
        let shell = zbus::blocking::Proxy::new(
            bus,
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.gnome.Shell",
        )
        .unwrap();
        let remote_session = zbus::blocking::Proxy::new(
            bus,
            "org.gnome.Mutter.RemoteDesktop",
            remote.as_str(),
            "org.gnome.Mutter.RemoteDesktop.Session",
        )
        .unwrap();
        for overview in [true, false] {
            if overview {
                shell.set_property("OverviewActive", true).unwrap();
            } else {
                // Alt+F2 opens Shell's real modal Run dialog without enabling Eval.
                for (key, pressed) in [(56u32, true), (60, true), (60, false), (56, false)] {
                    remote_session
                        .call::<_, _, ()>("NotifyKeyboardKeycode", &(key, pressed))
                        .unwrap();
                }
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while desktop.refresh().unwrap().window != 0 {
                assert!(
                    Instant::now() < deadline,
                    "Shell keyboard grab retained a background input target"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(desktop.input_target().unwrap().is_none());
            assert!(desktop.popup_position(Some(target)).is_none());
            assert!(engine.send(EngineCommand::InsertText {
                text: "must not reach background Firefox".into(),
                target: Some(target)
            }));
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if engine
                    .events()
                    .recv_timeout(Duration::from_millis(100))
                    .is_ok_and(|event| matches!(event, Event::Error(_)))
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "Shell input grab did not reject background insertion"
                );
            }
            expect_text("first", "Github и linux windows plan b world ");
            assert_eq!(clipboard.text().unwrap().as_deref(), Some(saved_clipboard));
            if overview {
                shell.set_property("OverviewActive", false).unwrap();
            } else {
                for pressed in [true, false] {
                    remote_session
                        .call::<_, _, ()>("NotifyKeyboardKeycode", &(1u32, pressed))
                        .unwrap();
                }
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while desktop.refresh().unwrap().window == 0 {
                assert!(
                    Instant::now() < deadline,
                    "Shell did not restore the application input target"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    for (field, token) in [("second", 3), ("editable", 4)] {
        prepare(field, token);
        let payload = "история буфера\nsecond synthetic line";
        assert!(engine.send(EngineCommand::InsertText {
            text: payload.into(),
            target: desktop.input_target().unwrap()
        }));
        expect_text(field, payload);
        let deadline = Instant::now() + Duration::from_secs(5);
        while clipboard.text().unwrap().as_deref() != Some(saved_clipboard) {
            assert!(
                Instant::now() < deadline,
                "Firefox {field} paste did not restore synthetic clipboard: {:?}",
                clipboard.text().unwrap()
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    prepare("password", 5);
    assert!(engine.send(EngineCommand::InsertText {
        text: "must not be inserted".into(),
        target: desktop.input_target().unwrap()
    }));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if engine
            .events()
            .recv_timeout(Duration::from_millis(100))
            .is_ok_and(|event| matches!(event, Event::Error(_)))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "password insertion was not rejected"
        );
    }
    expect_text("password", "");
    assert_eq!(clipboard.text().unwrap().as_deref(), Some(saved_clipboard));
    engine.shutdown();
    drop(guard);
    drop(focus_watch);
    drop(driver);
    eprintln!(
        "Firefox Wayland input acceptance passed: layout correction, plan b, spelling, stale target, multiline history paste, clipboard restoration, password rejection ({})",
        workspace.display()
    );
}
fn send(path: &std::path::Path, field: &str, offset: i32) {
    let temporary = path.with_extension("new");
    std::fs::write(
        &temporary,
        serde_json::json!({"field":field,"offset":offset}).to_string(),
    )
    .unwrap();
    std::fs::rename(temporary, path).unwrap();
}

#[test]
#[ignore = "requires isolated native Wayland and official Firefox in tmp/linux-browser; opens only a local test page"]
fn real_firefox_fields_password_and_editable_document_have_distinct_targets() {
    browser_fields(false);
}

#[test]
#[ignore = "requires root/uinput, isolated GNOME/KDE Wayland and official Firefox; use tools/test-linux-browser-input.sh"]
fn real_firefox_engine_corrects_and_pastes_without_crossing_fields() {
    browser_fields(true);
}

fn browser_fields(check_input: bool) {
    if check_input {
        assert!(
            crate::permissions::is_root(),
            "run device acceptance as root in an isolated development VM"
        );
        assert!(matches!(
            crate::SessionInfo::detect().desktop,
            Desktop::Gnome | Desktop::Kde
        ));
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = workspace.join("tmp/linux-browser-tests");
    std::fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::TempDir::new_in(scratch).unwrap().keep();
    let state = temp.join("state.json");
    let command = temp.join("command.json");
    let ready = temp.join("port.txt");
    let server = Process(
        Command::new("python3")
            .arg(workspace.join("tools/linux-browser-field-fixture.py"))
            .args([&state, &command, &ready])
            .args(check_input.then_some("--report-synthetic-values"))
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(temp.join("server.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let port = loop {
        if let Ok(value) = std::fs::read_to_string(&ready) {
            break value.trim().to_string();
        }
        assert!(
            Instant::now() < deadline,
            "local browser fixture did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let kde = crate::SessionInfo::detect().desktop == Desktop::Kde;
    let mut bridge = kde.then(|| crate::desktop::serve_kde_bridge().unwrap());
    if kde && !check_input {
        crate::integration::install_current(&Desktop::Kde).unwrap();
    }
    let bus = zbus::blocking::Connection::session().unwrap();
    let remote_path: Option<zbus::zvariant::OwnedObjectPath> = if kde {
        None
    } else {
        let remote = zbus::blocking::Proxy::new(
            &bus,
            "org.gnome.Mutter.RemoteDesktop",
            "/org/gnome/Mutter/RemoteDesktop",
            "org.gnome.Mutter.RemoteDesktop",
        )
        .unwrap();
        Some(remote.call("CreateSession", &()).unwrap())
    };
    let remote = remote_path.as_ref().map(|path| {
        zbus::blocking::Proxy::new(
            &bus,
            "org.gnome.Mutter.RemoteDesktop",
            path.as_str(),
            "org.gnome.Mutter.RemoteDesktop.Session",
        )
        .unwrap()
    });
    if let Some(remote) = &remote {
        remote.call::<_, _, ()>("Start", &()).unwrap();
        remote
            .call::<_, _, ()>("NotifyKeyboardKeycode", &(42u32, true))
            .unwrap();
        remote
            .call::<_, _, ()>("NotifyKeyboardKeycode", &(42u32, false))
            .unwrap();
    }
    let profile = temp.join("profile");
    std::fs::create_dir(&profile).unwrap();
    let automation_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let automation_port = automation_listener.local_addr().unwrap().port();
    drop(automation_listener);
    std::fs::write(
        profile.join("user.js"),
        r#"
user_pref("accessibility.force_disabled", -1);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.newtabpage.enabled", false);
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.enabled", false);
user_pref("app.update.auto", false);
"#,
    )
    .unwrap();
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(profile.join("user.js"))
            .unwrap(),
        "user_pref(\"marionette.port\", {automation_port});"
    )
    .unwrap();
    let binary = std::fs::read_to_string(workspace.join("tmp/linux-browser/current.txt")).unwrap();
    let firefox = Process(
        Command::new(binary.trim())
            .args(["--no-remote", "--new-instance", "--marionette", "--profile"])
            .arg(&profile)
            .arg(format!("http://127.0.0.1:{port}/"))
            .env("MOZ_ENABLE_WAYLAND", "1")
            .env("GDK_BACKEND", "wayland")
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(temp.join("firefox.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    let desktop = LinuxDesktop::connect().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !desktop.integration_ready() {
        assert!(
            Instant::now() < deadline,
            "desktop integration did not connect"
        );
        desktop.refresh().unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }
    if kde && check_input {
        // Real session-start loading, then reconnect after losing the owner.
        // Do not reload the script: that would hide a broken startup lifecycle.
        drop(bridge.take());
        std::thread::sleep(Duration::from_millis(700));
        assert!(!desktop.integration_ready());
        bridge = Some(crate::desktop::serve_kde_bridge().unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !desktop.integration_ready() {
            assert!(
                Instant::now() < deadline,
                "KWin script did not reconnect after the application's restart"
            );
            desktop.refresh().unwrap();
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    let pid = firefox.0.id();
    let deadline = Instant::now() + Duration::from_secs(20);
    let window = loop {
        let native = desktop.refresh().unwrap();
        if native.pid == pid && native.title.contains("OKBS browser acceptance") {
            break native.window;
        }
        assert!(
            Instant::now() < deadline,
            "Firefox local page unavailable: {native:?}; logs {}",
            temp.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    desktop.command("ActivateWindow", window).unwrap();
    let automation = Command::new("python3")
        .arg(workspace.join("tools/linux-firefox-session.py"))
        .arg(automation_port.to_string())
        .output()
        .unwrap();
    assert!(
        automation.status.success(),
        "Firefox automation setup: {}",
        String::from_utf8_lossy(&automation.stderr)
    );
    if remote.is_some() {
        let shell = zbus::blocking::Proxy::new(
            &bus,
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.gnome.Shell",
        )
        .unwrap();
        shell.set_property("OverviewActive", false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while desktop.refresh().unwrap().window != window {
            assert!(
                Instant::now() < deadline,
                "GNOME did not expose the active Firefox window after leaving Overview"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    send(&command, "first", 4);
    if let Some(remote) = &remote {
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(100));
            let page = std::fs::read(&state)
                .ok()
                .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
            if page.as_ref().is_some_and(|v| v["documentFocus"] == true) {
                break;
            }
            remote
                .call::<_, _, ()>("NotifyKeyboardKeycode", &(15u32, true))
                .unwrap();
            remote
                .call::<_, _, ()>("NotifyKeyboardKeycode", &(15u32, false))
                .unwrap();
        }
        send(&command, "first", 4);
    }
    let read = |field: &str, previous: Option<u64>| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let native = desktop.refresh().unwrap();
            let page = std::fs::read(&state)
                .ok()
                .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
            if native.pid == pid
                && native.window == window
                && native.control != 0
                && previous.is_none_or(|id| native.control != id)
                && page
                    .as_ref()
                    .is_some_and(|v| v["focus"] == field && v["documentFocus"] == true)
                && desktop.is_password_field().unwrap() == Some(field == "password")
            {
                return native;
            }
            assert!(
                Instant::now() < deadline,
                "Firefox field {field} unavailable: {native:?}, page {page:?}; logs {}",
                temp.display()
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let first = read("first", None);
    let target = desktop.target().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let caret = loop {
        let native = read("first", None);
        if let Some(point) = desktop.caret_position(desktop.target()) {
            let [x, y, w, h] = native.client;
            assert!(
                point[0] >= x as f32
                    && point[0] <= (x + w) as f32
                    && point[1] >= y as f32
                    && point[1] <= (y + h) as f32,
                "Firefox caret {point:?} outside {:?}",
                native.client
            );
            break point;
        }
        assert!(
            Instant::now() < deadline,
            "Firefox caret unavailable: {native:?}"
        );
    };
    send(&command, "first", 12);
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        read("first", None);
        if desktop
            .caret_position(desktop.target())
            .is_some_and(|p| (p[0] - caret[0]).abs() > 10.0)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Firefox caret did not follow selection offset"
        );
    }
    send(&command, "password", 4);
    let password = read("password", Some(first.control));
    assert_ne!(first.control, password.control);
    assert!(desktop.caret_position(desktop.target()).is_none());
    assert!(desktop.popup_position(Some(target)).is_none());
    send(&command, "second", 4);
    let second = read("second", Some(password.control));
    assert_ne!(first.control, second.control);
    assert_ne!(password.control, second.control);
    assert!(desktop.caret_position(Some(target)).is_none());
    send(&command, "editable", 4);
    let editable = read("editable", Some(second.control));
    assert_ne!(editable.control, second.control);
    assert_ne!(editable.control, first.control);
    let point = desktop.popup_position(desktop.target()).unwrap();
    let [x, y, w, h] = editable.client;
    assert!(
        point[0] >= x as f32
            && point[0] <= (x + w) as f32
            && point[1] >= y as f32
            && point[1] <= (y + h) as f32
    );
    if check_input {
        // Headless GNOME can start in Overview. DOM focus alone does not
        // prove the compositor is delivering keys to the application.
        if !kde {
            let shell = zbus::blocking::Proxy::new(
                &bus,
                "org.gnome.Shell",
                "/org/gnome/Shell",
                "org.gnome.Shell",
            )
            .unwrap();
            shell.set_property("OverviewActive", false).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            desktop.command("ActivateWindow", window).unwrap();
        }
        check_real_input(
            &workspace,
            &state,
            &command,
            &desktop,
            remote_path.as_ref(),
            &bus,
        );
    }
    drop(firefox);
    drop(server);
    drop(bridge);
    if let Some(remote) = &remote {
        remote.call::<_, _, ()>("Stop", &()).unwrap();
    }
}
