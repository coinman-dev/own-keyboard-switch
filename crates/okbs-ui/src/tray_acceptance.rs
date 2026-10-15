//! The real tray over StatusNotifierItem/dbusmenu in a private session bus,
//! read the way a desktop panel reads it.
// Test-only module: clippy does not see `cfg(all(test, ...))` as test code.
#![allow(clippy::unwrap_used)]
use crate::i18n::{Text, tr};
use crate::icon::{ICON_SIZE, IconState, render};
use crate::tray::{Tray, TrayCommand, TrayLayout, TrayState};
use okbs_core::Lang;
use okbs_core::config::{AutoReplace, AutoReplaceItem, General};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The panel side of the protocol: it only records registered items.
struct Watcher(Arc<Mutex<Vec<String>>>);
#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) {
        let sender = header.sender().map(ToString::to_string).unwrap_or_default();
        // An item may pass its bus name or only its object path.
        let item = if service.starts_with('/') {
            format!("{sender}{service}")
        } else {
            format!("{service}/StatusNotifierItem")
        };
        self.0.lock().unwrap().push(item);
    }
    fn register_status_notifier_host(&self, _service: &str) {}
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

/// StatusNotifierItem pixmaps are ARGB32 in network byte order.
fn argb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks(4)
        .flat_map(|pixel| [pixel[3], pixel[0], pixel[1], pixel[2]])
        .collect()
}

fn wait(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `(id, label, toggle-state)` of every dbusmenu entry, from gdbus text.
fn menu_entries(service: &str, path: &str) -> Vec<(i32, String, Option<i32>)> {
    let output = std::process::Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            service,
            "--object-path",
            path,
        ])
        .args([
            "--method",
            "com.canonical.dbusmenu.GetLayout",
            "--",
            "0",
            "-1",
            "[]",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    // Every node is printed as `(<id>, {properties}, [children])`.
    let mut entries = Vec::new();
    for node in text.split("<(").skip(1) {
        let Some((id, rest)) = node.split_once(',') else {
            continue;
        };
        let Ok(id) = id.trim().parse() else {
            continue;
        };
        let properties = rest.split_once('}').map_or(rest, |(head, _)| head);
        let label = properties
            .split_once("'label': <'")
            .and_then(|(_, tail)| tail.split_once("'>"))
            .map(|(label, _)| label.replace('_', ""))
            .unwrap_or_default();
        let toggle = properties
            .split_once("'toggle-state': <")
            .and_then(|(_, tail)| tail.split_once('>'))
            .and_then(|(value, _)| value.trim().parse().ok());
        entries.push((id, label, toggle));
    }
    entries
}

fn click(service: &str, path: &str, id: i32) {
    let status = std::process::Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            service,
            "--object-path",
            path,
        ])
        .args([
            "--method",
            "com.canonical.dbusmenu.Event",
            "--",
            &id.to_string(),
        ])
        .args(["clicked", "<0>", "0"])
        .output()
        .unwrap();
    assert!(status.status.success(), "{status:?}");
}

#[test]
#[ignore = "requires a private session bus (dbus-run-session); see tools/test-linux-x11.sh"]
fn real_status_notifier_tray_exposes_icon_menu_and_commands() {
    let registered = Arc::new(Mutex::new(Vec::new()));
    let _watcher = zbus::blocking::connection::Builder::session()
        .unwrap()
        .name("org.kde.StatusNotifierWatcher")
        .unwrap()
        .serve_at("/StatusNotifierWatcher", Watcher(registered.clone()))
        .unwrap()
        .build()
        .unwrap();
    let mut general = General::default();
    let english = IconState::for_layout(&general, Some("en-US"), Some(Lang::En), true, false);
    let layouts = [
        TrayLayout {
            name: "English (US)".into(),
            flag: None,
        },
        TrayLayout {
            name: "Russian".into(),
            flag: None,
        },
    ];
    let autoreplace = AutoReplace {
        items: vec![AutoReplaceItem {
            from: "sig".into(),
            to: "Best regards".into(),
            cursor_pos: -1,
        }],
        ..AutoReplace::default()
    };
    let state = TrayState {
        icon: english,
        sounds: true,
        layout: Some(0),
    };
    let mut tray = Tray::new(state, Lang::En, &autoreplace, &layouts).unwrap();
    wait("the tray did not register with the watcher", || {
        !registered.lock().unwrap().is_empty()
    });
    let item = registered.lock().unwrap()[0].clone();
    let (service, path) = item.split_at(item.find('/').unwrap());
    let bus = zbus::blocking::Connection::session().unwrap();
    let properties =
        zbus::blocking::Proxy::new(&bus, service, path, "org.kde.StatusNotifierItem").unwrap();
    let pixmap = |properties: &zbus::blocking::Proxy| -> Vec<(i32, i32, Vec<u8>)> {
        // Proxies cache properties; read the current value from the item.
        let value: zbus::zvariant::OwnedValue = zbus::blocking::Proxy::new(
            properties.connection(),
            properties.destination().to_owned(),
            properties.path().to_owned(),
            "org.freedesktop.DBus.Properties",
        )
        .unwrap()
        .call("Get", &("org.kde.StatusNotifierItem", "IconPixmap"))
        .unwrap();
        value.try_into().unwrap()
    };
    let expected =
        |icon: IconState| vec![(ICON_SIZE as i32, ICON_SIZE as i32, argb(&render(icon)))];
    assert_eq!(pixmap(&properties), expected(english), "text icon pixels");
    // «Сделать значок в виде флагов стран»: the Russian flag replaces the text.
    general.tray_flags = true;
    let russian = IconState::for_layout(&general, Some("ru-RU"), Some(Lang::Ru), true, false);
    assert!(russian.flag.is_some());
    tray.update(TrayState {
        icon: russian,
        sounds: false,
        layout: Some(1),
    })
    .unwrap();
    wait("the flag icon was not published", || {
        pixmap(&properties) == expected(russian)
    });
    assert_ne!(expected(russian), expected(english));
    let menu: zbus::zvariant::OwnedObjectPath = properties.get_property("Menu").unwrap();
    let entries = menu_entries(service, menu.as_str());
    let find = |label: &str| {
        entries
            .iter()
            .find(|entry| entry.1 == label)
            .unwrap_or_else(|| panic!("no menu entry {label:?} in {entries:?}"))
            .clone()
    };
    assert_eq!(find("English (US)").2, Some(0));
    assert_eq!(find("Russian").2, Some(1), "the active layout is checked");
    assert_eq!(find(tr(Text::MenuSounds, Lang::En)).2, Some(0));
    assert_eq!(find(tr(Text::MenuAutoswitch, Lang::En)).2, Some(1));
    // Activate is the host's primary click on the icon.
    let _: () = properties.call("Activate", &(0i32, 0i32)).unwrap();
    let mut received = None;
    wait("Activate did not reach the tray", || {
        received = tray.poll();
        received.is_some()
    });
    assert_eq!(received, Some(TrayCommand::OpenSettings));
    for (label, command) in [
        (
            tr(Text::MenuSettings, Lang::En).to_string(),
            TrayCommand::OpenSettings,
        ),
        ("English (US)".into(), TrayCommand::SelectLayout(0)),
        (
            "sig — Best regards".into(),
            TrayCommand::InsertAutoreplace(0),
        ),
        (
            tr(Text::MenuSounds, Lang::En).into(),
            TrayCommand::ToggleSounds,
        ),
        (tr(Text::MenuExit, Lang::En).into(), TrayCommand::Exit),
    ] {
        click(service, menu.as_str(), find(&label).0);
        let mut received = None;
        wait(
            &format!("menu entry {label:?} did not reach the tray"),
            || {
                received = tray.poll();
                received.is_some()
            },
        );
        assert_eq!(received, Some(command), "{label}");
    }
}

/// `org.gnome.Shell.Eval` of a JavaScript expression (the private test Shell
/// enables it with tools/linux-gnome-eval-fixture.js).
fn shell_eval(bus: &zbus::blocking::Connection, code: &str) -> serde_json::Value {
    let shell = zbus::blocking::Proxy::new(
        bus,
        "org.gnome.Shell",
        "/org/gnome/Shell",
        "org.gnome.Shell",
    )
    .unwrap();
    let (ok, result): (bool, String) = shell.call("Eval", &(code,)).unwrap();
    assert!(ok, "Shell.Eval failed: {result}");
    serde_json::from_str(&result).unwrap_or(serde_json::Value::Null)
}

/// The panel button of the tray item and its menu, as the Shell shows them.
const PANEL_ITEM: &str = r#"(() => {
    const entry = Object.entries(Main.panel.statusArea)
        .find(([key, item]) => key.startsWith('appindicator-') && item?._indicator?.id === 'okbswitch');
    if (!entry)
        return {keys: Object.keys(Main.panel.statusArea)};
    const button = entry[1];
    const box = actor => {
        const [x, y] = actor.get_transformed_position();
        const [width, height] = actor.get_transformed_size();
        return {x, y, width, height, mapped: actor.mapped};
    };
    const items = button.menu._getMenuItems().map(item => ({
        label: item.label?.text ?? null,
        ornament: item._ornament ?? null,
        box: box(item),
    }));
    // A pixmap without an overlay becomes the content of the icon actor.
    const content = button._icon?.content ?? null;
    const icon = content ? content.get_preferred_size().slice(1) : null;
    const changed = content !== globalThis.okbsTrayContent;
    globalThis.okbsTrayContent = content;
    return {button: box(button), icon, changed, open: button.menu.isOpen, items};
})()"#;

/// A real pointer of a Mutter RemoteDesktop session.
struct GnomePointer<'a> {
    bus: &'a zbus::blocking::Connection,
    session: zbus::blocking::Proxy<'a>,
}
impl<'a> GnomePointer<'a> {
    fn new(bus: &'a zbus::blocking::Connection) -> Self {
        let manager = zbus::blocking::Proxy::new(
            bus,
            "org.gnome.Mutter.RemoteDesktop",
            "/org/gnome/Mutter/RemoteDesktop",
            "org.gnome.Mutter.RemoteDesktop",
        )
        .unwrap();
        let path: zbus::zvariant::OwnedObjectPath = manager.call("CreateSession", &()).unwrap();
        let session = zbus::blocking::Proxy::new(
            bus,
            "org.gnome.Mutter.RemoteDesktop",
            path,
            "org.gnome.Mutter.RemoteDesktop.Session",
        )
        .unwrap();
        let _: () = session.call("Start", &()).unwrap();
        Self { bus, session }
    }
    /// Relative moves corrected by the position the Shell reports.
    fn move_to(&self, point: [f64; 2]) {
        for _ in 0..12 {
            let at = shell_eval(self.bus, "global.get_pointer().slice(0, 2)");
            let [x, y] = [at[0].as_f64().unwrap(), at[1].as_f64().unwrap()];
            if (x - point[0]).abs() < 2.0 && (y - point[1]).abs() < 2.0 {
                return;
            }
            let _: () = self
                .session
                .call("NotifyPointerMotionRelative", &(point[0] - x, point[1] - y))
                .unwrap();
            std::thread::sleep(Duration::from_millis(40));
        }
        panic!("the pointer did not reach {point:?}");
    }
    fn click(&self, point: [f64; 2], button: i32, count: usize) {
        self.move_to(point);
        for _ in 0..count {
            for pressed in [true, false] {
                let _: () = self
                    .session
                    .call("NotifyPointerButton", &(button, pressed))
                    .unwrap();
            }
            std::thread::sleep(Duration::from_millis(60));
        }
    }
}
impl Drop for GnomePointer<'_> {
    fn drop(&mut self) {
        let _ = self.session.call::<_, _, ()>("Stop", &());
    }
}

fn center(value: &serde_json::Value) -> [f64; 2] {
    let number = |key: &str| value[key].as_f64().unwrap();
    [
        number("x") + number("width") / 2.0,
        number("y") + number("height") / 2.0,
    ]
}

#[test]
#[ignore = "requires the private GNOME Shell with Ubuntu AppIndicators; see tools/test-linux-gnome.sh"]
fn real_gnome_panel_tray_shows_icon_menu_and_commands() {
    const LEFT: i32 = 272;
    const RIGHT: i32 = 273;
    let bus = zbus::blocking::Connection::session().unwrap();
    let general = General::default();
    let layouts = [
        TrayLayout {
            name: "English (US)".into(),
            flag: None,
        },
        TrayLayout {
            name: "Russian".into(),
            flag: None,
        },
    ];
    let state = TrayState {
        icon: IconState::for_layout(&general, Some("en-US"), Some(Lang::En), true, false),
        sounds: true,
        layout: Some(0),
    };
    let mut tray = Tray::new(state, Lang::En, &AutoReplace::default(), &layouts).unwrap();
    let mut item = serde_json::Value::Null;
    let deadline = Instant::now() + Duration::from_secs(10);
    // The Shell loads the pixmap and the menu asynchronously.
    while item["icon"].is_null() || item["items"].as_array().is_none_or(Vec::is_empty) {
        assert!(
            Instant::now() < deadline,
            "no okbswitch tray item in the panel: {item}"
        );
        std::thread::sleep(Duration::from_millis(100));
        item = shell_eval(&bus, PANEL_ITEM);
    }
    eprintln!("panel item: {item}");
    assert_eq!(item["button"]["mapped"], true, "{item}");
    assert_eq!(
        item["icon"],
        serde_json::json!([ICON_SIZE, ICON_SIZE]),
        "our pixmap"
    );
    // «Сделать значок в виде флагов стран»: the panel picks up the new pixmap.
    let flags = General {
        tray_flags: true,
        ..General::default()
    };
    tray.update(TrayState {
        icon: IconState::for_layout(&flags, Some("ru-RU"), Some(Lang::Ru), true, false),
        layout: Some(1),
        ..state
    })
    .unwrap();
    wait("the panel kept the old icon", || {
        shell_eval(&bus, PANEL_ITEM)["changed"] == true
    });
    let pointer = GnomePointer::new(&bus);
    let button = center(&item["button"]);
    let opened = |want: bool| {
        let mut item = serde_json::Value::Null;
        wait(
            &format!("the panel menu did not become open={want}"),
            || {
                item = shell_eval(&bus, PANEL_ITEM);
                item["open"] == want
            },
        );
        item
    };
    let entry = |item: &serde_json::Value, label: &str| {
        item["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["label"] == label)
            .unwrap_or_else(|| panic!("no menu entry {label:?} in {item}"))
            .clone()
    };
    let sounds = tr(Text::MenuSounds, Lang::En);
    // A right click opens the host's menu built from our dbusmenu.
    pointer.click(button, RIGHT, 1);
    let item = opened(true);
    let checked = entry(&item, sounds)["ornament"].clone();
    pointer.click(center(&entry(&item, sounds)["box"]), LEFT, 1);
    let mut received = None;
    wait("the panel menu entry did not reach the tray", || {
        received = tray.poll();
        received.is_some()
    });
    assert_eq!(received, Some(TrayCommand::ToggleSounds));
    opened(false);
    // The application applies the command; the Shell shows the new check mark.
    tray.update(TrayState {
        sounds: false,
        ..state
    })
    .unwrap();
    pointer.click(button, RIGHT, 1);
    let mut item = opened(true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while entry(&item, sounds)["ornament"] == checked {
        assert!(
            Instant::now() < deadline,
            "the panel kept the old check mark: {item}"
        );
        std::thread::sleep(Duration::from_millis(100));
        item = shell_eval(&bus, PANEL_ITEM);
    }
    eprintln!("after the update: {item}");
    pointer.click(
        center(&entry(&item, tr(Text::MenuSettings, Lang::En))["box"]),
        LEFT,
        1,
    );
    let mut received = None;
    wait("Settings did not reach the tray", || {
        received = tray.poll();
        received.is_some()
    });
    assert_eq!(received, Some(TrayCommand::OpenSettings));
    opened(false);
    // A double click activates the item: the settings, as on Windows.
    pointer.click(button, LEFT, 2);
    let mut received = None;
    wait("a double click did not reach the tray", || {
        received = tray.poll();
        received.is_some()
    });
    assert_eq!(received, Some(TrayCommand::OpenSettings));
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(
        shell_eval(&bus, PANEL_ITEM)["open"],
        false,
        "no menu after a double click"
    );
}
