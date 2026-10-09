//! Real native Wayland windows and AT-SPI fields in an isolated compositor.
use gtk::prelude::*;
use okbs_core::{
    Lang,
    config::{Config, UiLanguage},
    spell::Misspelling,
};
use okbs_platform_linux::{
    desktop::{LinuxDesktop, PlacedWindow},
    session::Desktop,
};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires isolated GNOME/KDE Wayland with two monitors and current integration"]
fn real_wayland_egui_popups_and_caret_use_the_requested_monitor() {
    gtk::init().unwrap();
    let kde = okbs_platform_linux::SessionInfo::detect().desktop == Desktop::Kde;
    // Headless Mutter starts without a keyboard. Keep an isolated virtual
    // keyboard session alive so GTK receives real wl_keyboard focus events.
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
    let _bridge = kde.then(|| okbs_platform_linux::desktop::serve_kde_bridge().unwrap());
    if kde {
        okbs_platform_linux::integration::install_current(&Desktop::Kde).unwrap();
    }
    let desktop = LinuxDesktop::connect().unwrap();
    assert!(
        desktop.integration_ready(),
        "native GUI acceptance needs the current compositor integration"
    );
    let pump = || {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let display = gtk::gdk::Display::default().unwrap();
    assert_eq!(
        display.n_monitors(),
        2,
        "test needs two real Wayland outputs"
    );
    let monitor = (0..display.n_monitors())
        .map(|i| display.monitor(i).unwrap().geometry())
        .max_by_key(|monitor| monitor.x())
        .unwrap();
    let area = [monitor.x(), monitor.y(), monitor.width(), monitor.height()];
    let wait_window = |title: &str, expected: Option<[i32; 2]>| -> PlacedWindow {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut previous = None;
        loop {
            pump();
            if let Some(window) = desktop.placed_window(title).unwrap() {
                let [x, y, w, h] = window.frame;
                if w > 0
                    && h > 0
                    && x >= area[0]
                    && y >= area[1]
                    && x + w <= area[0] + area[2]
                    && y + h <= area[1] + area[3]
                    && expected.is_none_or(|p| (x - p[0]).abs() <= 2 && (y - p[1]).abs() <= 2)
                {
                    return window;
                }
                previous = Some(window.frame);
            }
            assert!(
                Instant::now() < deadline,
                "{title}: actual {previous:?}, monitor {area:?}, expected {expected:?}"
            );
        }
    };
    let editor_title = "OKBS Wayland editor fixture";
    let editor = gtk::Window::new(gtk::WindowType::Toplevel);
    editor.set_title(editor_title);
    editor.set_decorated(false);
    editor.set_default_size(380, 200);
    let fields = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let first = gtk::Entry::new();
    first.set_text("synthetic fixture");
    let password = gtk::Entry::new();
    password.set_visibility(false);
    password.set_text("synthetic password");
    fields.add(&first);
    fields.add(&password);
    editor.add(&fields);
    let point = [area[0] + 40, area[1] + 60];
    desktop
        .place_owned_window(editor_title, point.map(|v| v as f32))
        .unwrap();
    editor.show_all();
    editor.present();
    first.grab_focus();
    first.set_position(4);
    let placed_editor = wait_window(editor_title, Some(point));
    // Earlier application tests leave a real compositor focus history. Put
    // this editor in the foreground explicitly, as a user would, instead of
    // relying on automatic focus when a new window appears on another output.
    desktop
        .command("ActivateWindow", placed_editor.window)
        .unwrap();
    if let Some(remote) = &remote {
        remote
            .call::<_, _, ()>("NotifyKeyboardKeycode", &(42u32, true))
            .unwrap();
        remote
            .call::<_, _, ()>("NotifyKeyboardKeycode", &(42u32, false))
            .unwrap();
    }
    first.grab_focus();
    let wait_focus = || {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            pump();
            let state = desktop.refresh().unwrap();
            if state.window == placed_editor.window && state.control != 0 {
                return state;
            }
            assert!(
                Instant::now() < deadline,
                "editor focus unavailable: expected {}, {state:?}; GTK active={} hasFocus={} isFocus={}",
                placed_editor.window,
                editor.is_active(),
                first.has_focus(),
                first.is_focus()
            );
        }
    };
    let focused = wait_focus();
    eprintln!(
        "native editor active={}, first focused={}, client={:?}",
        editor.is_active(),
        first.has_focus(),
        focused.client
    );
    let deadline = Instant::now() + Duration::from_secs(4);
    let caret = loop {
        pump();
        desktop.refresh().unwrap();
        if let Some(point) = desktop.caret_position(desktop.target()) {
            break point;
        }
        assert!(
            Instant::now() < deadline,
            "native Wayland caret unavailable"
        );
    };
    let [x, y, w, h] = placed_editor.frame;
    assert!(
        caret[0] >= x as f32
            && caret[0] <= (x + w) as f32
            && caret[1] >= y as f32
            && caret[1] <= (y + h) as f32,
        "caret {caret:?} outside editor {:?}",
        placed_editor.frame
    );
    let moved_point = [point[0] + 80, point[1] + 40];
    desktop
        .place_owned_window(editor_title, moved_point.map(|v| v as f32))
        .unwrap();
    wait_window(editor_title, Some(moved_point));
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        pump();
        desktop.refresh().unwrap();
        if desktop.caret_position(desktop.target()).is_some_and(|p| {
            (p[0] - caret[0] - 80.0).abs() <= 2.0 && (p[1] - caret[1] - 40.0).abs() <= 2.0
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "caret must follow the client area after moving its window"
        );
    }
    password.grab_focus();
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        pump();
        let state = desktop.refresh().unwrap();
        if state.control != focused.control && desktop.caret_position(desktop.target()).is_none() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "password must have another target and no caret geometry"
        );
    }
    first.grab_focus();
    wait_focus();
    let (events, _rx) = crossbeam_channel::unbounded();
    let settings = crate::window::SettingsWindow::spawn(events, Lang::En).unwrap();
    let list = settings.autoreplace_list();
    let title = "OKBS Wayland list fixture";
    let list_config = crate::autoreplace_list::AutoreplaceListConfig {
        labels: crate::autoreplace_list::AutoreplaceListLabels {
            title: title.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    desktop
        .place_owned_window(title, point.map(|v| v as f32))
        .unwrap();
    list.show(list_config.clone(), false, point.map(|v| v as f32));
    wait_window(title, Some(point));
    list.hide();
    desktop
        .command("ActivateWindow", placed_editor.window)
        .unwrap();
    wait_focus();
    let mut config = Config::default();
    config.general.ui_language = UiLanguage::En;
    let spell_title = crate::tr(crate::Text::SpellcheckWordTitle, Lang::En);
    let corner = [
        (area[0] + area[2] - 1) as f32,
        (area[1] + area[3] - 1) as f32,
    ];
    desktop.place_passive_window(spell_title, corner).unwrap();
    settings.show_text_passive(
        &config,
        crate::text_result::TextResult::spelling_popup("wrold".into(), Vec::new()),
        Some(corner),
        None,
    );
    wait_window(spell_title, None);
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        pump();
        if desktop.refresh().unwrap().window == placed_editor.window {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "passive popup stole editor focus"
        );
    }
    // The first mapped surface can still have egui's provisional height and
    // the scale of its original output. Wait for the measured viewport size.
    let settle = Instant::now() + Duration::from_millis(700);
    while Instant::now() < settle {
        pump();
    }
    let small = wait_window(spell_title, None);
    // egui measures and grows this viewport after it is already displayed.
    settings.show_text_passive(
        &config,
        crate::text_result::TextResult::spelling_popup(
            "wrold".into(),
            vec![Misspelling {
                range: 0..5,
                word: "wrold".into(),
                lang: Lang::En,
                suggestions: (0..8)
                    .map(|i| format!("synthetic suggestion {i}"))
                    .collect(),
            }],
        ),
        Some(corner),
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let grown = wait_window(spell_title, None);
        if grown.frame[3] > small.frame[3] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "spelling viewport must grow for additional suggestions: small {:?}, grown {:?}",
            small.frame,
            grown.frame
        );
    }
    let history = settings.clipboard_history();
    let history_title = "OKBS Wayland history fixture";
    desktop.place_owned_window(history_title, corner).unwrap();
    history.show(
        crate::clipboard_history::ClipboardHistoryConfig {
            entries: vec!["synthetic clipboard".into()],
            labels: crate::clipboard_history::ClipboardHistoryLabels {
                title: history_title.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        corner,
    );
    wait_window(history_title, None);
    // A fresh show request must reposition a previously created viewport.
    desktop
        .place_owned_window(title, point.map(|v| v as f32))
        .unwrap();
    list.show(list_config, false, point.map(|v| v as f32));
    wait_window(title, Some(point));
    history.hide();
    list.hide();
    drop(settings);
    editor.close();
    pump();
    if let Some(remote) = &remote {
        remote.call::<_, _, ()>("Stop", &()).unwrap();
    }
}
