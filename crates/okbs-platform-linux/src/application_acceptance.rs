//! Real Qt widgets and the distribution's KWrite editor in isolated Wayland.
use crate::{desktop::LinuxDesktop, session::Desktop};
use okbs_platform::FocusInfo;
use std::{
    collections::{HashSet, VecDeque},
    hash::{Hash, Hasher},
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

fn focused_text_capability(
    bus: &zbus::blocking::Connection,
    pid: u32,
    control: u64,
) -> Option<bool> {
    type Object = (String, zbus::zvariant::OwnedObjectPath);
    let service =
        zbus::blocking::Proxy::new(bus, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus").ok()?;
    let address: String = service.call("GetAddress", &()).ok()?;
    let access = zbus::blocking::connection::Builder::address(address.as_str())
        .ok()?
        .method_timeout(Duration::from_millis(100))
        .build()
        .ok()?;
    let root = zbus::blocking::Proxy::new(
        &access,
        "org.a11y.atspi.Registry",
        "/org/a11y/atspi/accessible/root",
        "org.a11y.atspi.Accessible",
    )
    .ok()?;
    let applications: Vec<Object> = root.call("GetChildren", &()).ok()?;
    let owners = zbus::blocking::fdo::DBusProxy::new(&access).ok()?;
    let mut queue = VecDeque::new();
    for object in applications {
        if owners
            .get_connection_unix_process_id(object.0.as_str().try_into().ok()?)
            .ok()
            == Some(pid)
        {
            queue.push_back(object);
        }
    }
    let mut visited = HashSet::new();
    while let Some(object) = queue.pop_front() {
        if visited.len() >= 512 {
            break;
        }
        if !visited.insert(object.clone()) {
            continue;
        }
        let accessible: zbus::blocking::Proxy<'_> = zbus::blocking::proxy::Builder::new(&access)
            .destination(object.0.as_str())
            .ok()?
            .path(object.1.as_str())
            .ok()?
            .interface("org.a11y.atspi.Accessible")
            .ok()?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .ok()?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        object.hash(&mut hash);
        if hash.finish().max(1) == control {
            let interfaces: Vec<String> = accessible.call("GetInterfaces", &()).ok()?;
            let children: Vec<Object> = accessible.call("GetChildren", &()).ok()?;
            eprintln!(
                "KWrite focused node interfaces={interfaces:?}, children={}",
                children.len()
            );
            if interfaces
                .iter()
                .any(|interface| interface == "org.a11y.atspi.Text")
            {
                return Some(true);
            }
            // A toolkit may put the editor behind a focused container. Check
            // its descendants too before accepting the no-caret fallback.
            let mut descendants: VecDeque<Object> = children
                .into_iter()
                .map(|(name, path)| {
                    (
                        if name.is_empty() {
                            object.0.clone()
                        } else {
                            name
                        },
                        path,
                    )
                })
                .collect();
            let mut checked = HashSet::new();
            while let Some(child) = descendants.pop_front() {
                if checked.len() >= 64 {
                    return None;
                }
                if !checked.insert(child.clone()) {
                    continue;
                }
                let proxy: zbus::blocking::Proxy<'_> = zbus::blocking::proxy::Builder::new(&access)
                    .destination(child.0.as_str())
                    .ok()?
                    .path(child.1.as_str())
                    .ok()?
                    .interface("org.a11y.atspi.Accessible")
                    .ok()?
                    .cache_properties(zbus::proxy::CacheProperties::No)
                    .build()
                    .ok()?;
                let interfaces: Vec<String> = proxy.call("GetInterfaces", &()).ok()?;
                if interfaces
                    .iter()
                    .any(|interface| interface == "org.a11y.atspi.Text")
                {
                    let text: zbus::blocking::Proxy<'_> =
                        zbus::blocking::proxy::Builder::new(&access)
                            .destination(child.0.as_str())
                            .ok()?
                            .path(child.1.as_str())
                            .ok()?
                            .interface("org.a11y.atspi.Text")
                            .ok()?
                            .cache_properties(zbus::proxy::CacheProperties::No)
                            .build()
                            .ok()?;
                    if text
                        .get_property::<i32>("CaretOffset")
                        .is_ok_and(|offset| offset >= 0)
                    {
                        return Some(true);
                    }
                }
                let children: Vec<Object> = proxy.call("GetChildren", &()).ok()?;
                descendants.extend(children.into_iter().map(|(name, path)| {
                    (
                        if name.is_empty() {
                            child.0.clone()
                        } else {
                            name
                        },
                        path,
                    )
                }));
            }
            return Some(false);
        }
        if let Ok(children) = accessible.call::<_, _, Vec<Object>>("GetChildren", &()) {
            queue.extend(children.into_iter().map(|(name, path)| {
                (
                    if name.is_empty() {
                        object.0.clone()
                    } else {
                        name
                    },
                    path,
                )
            }));
        }
    }
    None
}

#[test]
#[ignore = "requires isolated Wayland, current integration, KWrite and PyQt6; opens only test documents"]
fn real_qt_fields_and_kwrite_preserve_caret_password_and_window_identity() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = workspace.join("tmp/linux-applications");
    std::fs::create_dir_all(&scratch).unwrap();
    let temporary = tempfile::TempDir::new_in(scratch).unwrap();
    let temp = temporary.keep();
    let kde = crate::SessionInfo::detect().desktop == Desktop::Kde;
    let _bridge = kde.then(|| crate::desktop::serve_kde_bridge().unwrap());
    if kde {
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
    let desktop = LinuxDesktop::connect().unwrap();
    assert!(desktop.integration_ready());
    let state = temp.join("qt-state.json");
    let command = temp.join("qt-command.txt");
    let frameless = std::env::var("OKBS_TEST_QT_FRAMELESS").is_ok_and(|v| v == "1");
    let qt = Process(
        Command::new("python3")
            .arg(workspace.join("tools/linux-qt-field-fixture.py"))
            .args([&state, &command])
            .args(frameless.then_some("--frameless"))
            .env("QT_QPA_PLATFORM", "wayland")
            .env("QT_LINUX_ACCESSIBILITY_ALWAYS_ON", "1")
            .env("LANGUAGE", "en")
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(temp.join("qt.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    let pid = qt.0.id();
    let title = "OKBS Qt fields fixture";
    desktop
        .place_fixture_window(pid, title, [1300, 100])
        .unwrap();
    desktop
        .place_fixture_window(pid, "OKBS Qt second fixture", [1300, 140])
        .unwrap();
    let wait_window = |title: &str| {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(window) = desktop.fixture_window(pid, title).unwrap() {
                return window;
            }
            assert!(Instant::now() < deadline, "Qt window {title} not mapped");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let first_window = wait_window(title);
    desktop
        .command("ActivateWindow", first_window.window)
        .unwrap();
    std::fs::write(&command, "first").unwrap();
    let read = |expected: &str, window: u64| {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let native = desktop.refresh().unwrap();
            let value = std::fs::read(&state)
                .ok()
                .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
            if native.window == window
                && native.pid == pid
                && native.control != 0
                && value.as_ref().is_some_and(|v| v["focus"] == expected)
                && desktop.is_password_field().unwrap() == Some(expected == "password")
            {
                return (native, value.unwrap());
            }
            assert!(
                Instant::now() < deadline,
                "Qt focus {expected} unavailable: {native:?}, fixture {value:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let wait_caret = |expected: &str, window: u64| {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let (native, value) = read(expected, window);
            if let Some(caret) = desktop.caret_position(desktop.target()) {
                let expected = [
                    native.client[0] as f32
                        + value["caret"][0].as_f64().unwrap() as f32 * native.client[2] as f32
                            / value["window_size"][0].as_f64().unwrap() as f32,
                    native.client[1] as f32
                        + value["caret"][1].as_f64().unwrap() as f32 * native.client[3] as f32
                            / value["window_size"][1].as_f64().unwrap() as f32,
                ];
                if (caret[0] - expected[0]).abs() <= 4.0 && (caret[1] - expected[1]).abs() <= 4.0 {
                    return (native, caret);
                }
                assert!(
                    Instant::now() < deadline,
                    "Qt caret {caret:?}, expected {expected:?}, native {native:?}, geometry {value:?}"
                );
            } else {
                let sx = native.client[2] as f64 / value["window_size"][0].as_f64().unwrap();
                let sy = native.client[3] as f64 / value["window_size"][1].as_f64().unwrap();
                if !frameless && (sx - sy).abs() > 0.02 {
                    let fallback = desktop.popup_position(desktop.target()).unwrap();
                    let [x, y, w, h] = native.client;
                    assert!(
                        fallback[0] >= x as f32
                            && fallback[0] < (x + w) as f32
                            && fallback[1] >= y as f32
                            && fallback[1] < (y + h) as f32
                    );
                    return (native, fallback);
                }
                assert!(
                    Instant::now() < deadline,
                    "Qt caret missing: {native:?}, geometry {value:?}"
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let (first, caret) = wait_caret("first", first_window.window);
    let had_caret = desktop.caret_position(desktop.target()).is_some();
    let target = desktop.target().unwrap();
    std::fs::write(&command, "move-caret").unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let (_, next) = wait_caret("first", first_window.window);
        let geometry = std::fs::read(&state)
            .ok()
            .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
        if !had_caret
            && geometry
                .as_ref()
                .is_some_and(|v| v["caret"][0].as_f64().unwrap_or(0.0) > 60.0)
            || had_caret && (next[0] - caret[0]).abs() > 10.0
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Qt caret did not move inside its document"
        );
    }
    desktop.place_fixture_window(pid, title, [40, 100]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let (_, next) = wait_caret("first", first_window.window);
        if (next[0] - caret[0]).abs() > 500.0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Qt caret did not follow its window to the first monitor"
        );
    }
    std::fs::write(&command, "password").unwrap();
    let (password, _) = read("password", first_window.window);
    assert_ne!(password.control, first.control);
    assert!(desktop.caret_position(desktop.target()).is_none());
    assert!(desktop.caret_position(Some(target)).is_none());
    assert!(desktop.popup_position(Some(target)).is_none());
    std::fs::write(&command, "show-second").unwrap();
    let second_window = wait_window("OKBS Qt second fixture");
    desktop
        .command("ActivateWindow", second_window.window)
        .unwrap();
    std::fs::write(&command, "second").unwrap();
    let (second, _) = wait_caret("second", second_window.window);
    assert_ne!(second.control, first.control);
    assert!(desktop.caret_position(Some(target)).is_none());

    let document = temp.join("okbs-synthetic-document.txt");
    std::fs::write(&document, "Github b\nplan b\nwindows b linux\n").unwrap();
    let kwrite = Process(
        Command::new("kwrite")
            .args(["--line", "2", "--column", "4"])
            .arg(&document)
            .env("QT_QPA_PLATFORM", "wayland")
            .env("QT_LINUX_ACCESSIBILITY_ALWAYS_ON", "1")
            .env("LANGUAGE", "en")
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(temp.join("kwrite.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let native = desktop.refresh().unwrap();
        if native.pid == kwrite.0.id() && native.control != 0 {
            if let Some(point) = desktop.caret_position(desktop.target()) {
                let [x, y, w, h] = native.client;
                assert!(
                    point[0] >= x as f32
                        && point[0] <= (x + w) as f32
                        && point[1] >= y as f32
                        && point[1] <= (y + h) as f32,
                    "KWrite caret {point:?} outside client {:?}",
                    native.client
                );
                assert_eq!(desktop.is_password_field().unwrap(), Some(false));
                break;
            }
            let capability = focused_text_capability(&bus, native.pid, native.control);
            if capability == Some(false) {
                // KTextEditor can expose the focused editor without a Text
                // interface. Confirm this explicitly instead of inventing a
                // caret; the application then uses its active-window fallback.
                assert_eq!(desktop.is_password_field().unwrap(), Some(false));
                assert!(desktop.caret_position(Some(target)).is_none());
                let point = desktop.popup_position(desktop.target()).unwrap();
                let [x, y, w, h] = native.client;
                assert!(
                    point[0] >= x as f32
                        && point[0] < (x + w) as f32
                        && point[1] >= y as f32
                        && point[1] < (y + h) as f32,
                    "KWrite fallback {point:?} must stay in its active client {:?}, cursor {:?}",
                    native.client,
                    native.cursor
                );
                eprintln!(
                    "KWrite active-window fallback verified: focused document has no AT-SPI Text interface"
                );
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "KWrite caret unavailable: {native:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read_to_string(document).unwrap(),
        "Github b\nplan b\nwindows b linux\n"
    );
    drop(kwrite);
    drop(qt);
    if let Some(remote) = &remote {
        remote.call::<_, _, ()>("Stop", &()).unwrap();
    }
}
