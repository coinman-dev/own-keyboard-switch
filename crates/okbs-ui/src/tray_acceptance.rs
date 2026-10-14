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
