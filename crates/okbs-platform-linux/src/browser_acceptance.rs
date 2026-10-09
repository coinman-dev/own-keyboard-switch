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
    assert!(desktop.integration_ready());
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
    if let Some(remote) = &remote {
        let native = desktop.refresh().unwrap();
        remote
            .call::<_, _, ()>(
                "NotifyPointerMotionRelative",
                &(
                    f64::from(native.client[0] + native.client[2] / 2 - native.cursor[0]),
                    f64::from(native.client[1] + native.client[3] / 2 - native.cursor[1]),
                ),
            )
            .unwrap();
        remote
            .call::<_, _, ()>("NotifyPointerButton", &(272i32, true))
            .unwrap();
        remote
            .call::<_, _, ()>("NotifyPointerButton", &(272i32, false))
            .unwrap();
        remote
            .call::<_, _, ()>("NotifyKeyboardKeycode", &(42u32, true))
            .unwrap();
        remote
            .call::<_, _, ()>("NotifyKeyboardKeycode", &(42u32, false))
            .unwrap();
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
    drop(firefox);
    drop(server);
    if let Some(remote) = &remote {
        remote.call::<_, _, ()>("Stop", &()).unwrap();
    }
}
