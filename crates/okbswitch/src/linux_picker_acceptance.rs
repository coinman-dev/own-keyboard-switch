//! Actual controller, real egui pickers and Linux injection in a private session.
use super::*;
use evdev::{AttributeSet, EventType, InputEvent as DeviceEvent, KeyCode, uinput::VirtualDevice};
use okbs_engine::{Backends, Inputs, Processor};
use okbs_platform::{Clipboard, FocusInfo, KeyboardSource};
use okbs_platform_linux::{
    desktop::LinuxDesktop, input::LinuxSource, services::LinuxClipboard, session::Desktop,
};
use std::{
    io::{BufRead, Write},
    process::{Child, Command as ProcessCommand, Stdio},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = ProcessCommand::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.0.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
enum Keyboard {
    Gnome(zbus_keyboard::Remote),
    Kde {
        _process: Process,
        input: std::process::ChildStdin,
        output: std::io::BufReader<std::process::ChildStdout>,
    },
}
// Use the already-available GTK/GIO bindings, so the application gains no new
// runtime dependency just to automate a private compositor in acceptance.
mod zbus_keyboard {
    pub struct Remote(pub std::process::Child);
    impl Drop for Remote {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    impl Remote {
        pub fn new(
            workspace: &std::path::Path,
            command: &std::path::Path,
            ready: &std::path::Path,
        ) -> Self {
            Self(
                std::process::Command::new("python3")
                    .arg(workspace.join("tools/linux-picker-keyboard-fixture.py"))
                    .args([command, ready])
                    .spawn()
                    .expect("private picker fixture operation failed"),
            )
        }
        pub fn click(&self, command: &std::path::Path, point: [i32; 2]) {
            std::fs::write(command, serde_json::json!({"click":point}).to_string())
                .expect("write private pointer action");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while command.exists() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}
fn tick(controller: &mut Controller, desktop: &LinuxDesktop) {
    desktop
        .refresh()
        .expect("private picker fixture operation failed");
    assert!(controller.step());
    std::thread::sleep(Duration::from_millis(10));
}
fn wait_inserted(
    controller: &mut Controller,
    desktop: &LinuxDesktop,
    what: &str,
    gate: &okbs_platform::autoreplace_gate::AutoReplaceGate,
    output_idle: &Path,
    mut condition: impl FnMut(&Controller) -> bool,
) {
    wait(controller, desktop, what, |c| {
        !gate.is_busy() && condition(c)
    });
    let completed = std::time::SystemTime::now();
    wait(controller, desktop, "private output queue drained", |_| {
        std::fs::metadata(output_idle)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t >= completed)
    });
}
fn wait(
    controller: &mut Controller,
    desktop: &LinuxDesktop,
    what: &str,
    mut condition: impl FnMut(&Controller) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tick(controller, desktop);
        if condition(controller) {
            break;
        }
        assert!(Instant::now() < deadline, "{what}: {:?}", desktop.cached());
    }
}
fn wait_editor(controller: &mut Controller, desktop: &LinuxDesktop, target: InputTarget) {
    let mut stable = None;
    wait(
        controller,
        desktop,
        "editable target stable after clipboard/UI startup",
        |_| {
            if desktop.target() != Some(target) {
                stable = None;
                return false;
            }
            stable.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(400)
        },
    );
}
#[test]
#[ignore = "requires isolated GNOME/KDE Wayland and owned uinput; use tools/test-linux-pickers.sh"]
fn real_linux_pickers_and_process_restart_preserve_editor_and_history() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if let Some(directory) = std::env::var_os("OKBS_PICKER_DIRECTORY") {
        phase(
            &workspace,
            &PathBuf::from(directory),
            &std::env::var("OKBS_PICKER_PHASE").expect("private picker fixture operation failed"),
        );
        return;
    }
    let scratch = workspace.join("tmp/linux-picker-tests");
    std::fs::create_dir_all(&scratch).expect("private picker fixture operation failed");
    let directory = tempfile::TempDir::new_in(scratch)
        .expect("private picker fixture operation failed")
        .keep();
    for stage in ["seed", "restore", "disabled"] {
        let log = directory.join(format!("{stage}-application.log"));
        let output = std::fs::File::create(&log).expect("private picker fixture operation failed");
        let mut child = Process(ProcessCommand::new(std::env::current_exe().expect("private picker fixture operation failed"))
            .args(["controller::linux_picker_acceptance::real_linux_pickers_and_process_restart_preserve_editor_and_history", "--exact", "--ignored", "--nocapture"])
            .env("OKBS_PICKER_DIRECTORY", &directory).env("OKBS_PICKER_PHASE", stage)
            .stdout(output.try_clone().expect("private picker fixture operation failed")).stderr(output).spawn().expect("private picker fixture operation failed"));
        let deadline = Instant::now() + Duration::from_secs(35);
        let status = loop {
            if let Some(status) = child
                .0
                .try_wait()
                .expect("private picker fixture operation failed")
            {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "picker phase {stage} did not exit: {}; scratch {}",
                std::fs::read_to_string(&log).expect("private picker fixture operation failed"),
                directory.display()
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            status.success(),
            "picker phase {stage}: {}; scratch {}",
            std::fs::read_to_string(&log).expect("private picker fixture operation failed"),
            directory.display()
        );
    }
}
fn phase(workspace: &Path, directory: &Path, stage: &str) {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .try_init();
    let kde = okbs_platform_linux::SessionInfo::detect().desktop == Desktop::Kde;
    let _bridge = kde.then(|| {
        okbs_platform_linux::desktop::serve_kde_bridge()
            .expect("private picker fixture operation failed")
    });
    let desktop = LinuxDesktop::connect().expect("private picker fixture operation failed");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !desktop.integration_ready() {
        assert!(Instant::now() < deadline);
        desktop
            .refresh()
            .expect("private picker fixture operation failed");
        std::thread::sleep(Duration::from_millis(20));
    }
    let state = directory.join(format!("{stage}-fields.json"));
    let field_command = directory.join(format!("{stage}-field-command"));
    let log = std::fs::File::create(directory.join(format!("{stage}-editor.log")))
        .expect("private picker fixture operation failed");
    let editor = Process(
        ProcessCommand::new("python3")
            .arg(workspace.join("tools/linux-field-fixture.py"))
            .args([&state, &field_command])
            .arg("--no-select-on-focus")
            .stdout(
                log.try_clone()
                    .expect("private picker fixture operation failed"),
            )
            .stderr(log)
            .spawn()
            .expect("private picker fixture operation failed"),
    );
    let path = directory.join("config.toml");
    let history_file = directory.join("history.txt");
    if stage == "seed" {
        let mut config = Config::default();
        config.general.ui_language = okbs_core::config::UiLanguage::En;
        config.general.passwords_to_english = false;
        config.advanced.clipboard_history = true;
        config.advanced.clipboard_history_persist = false;
        config.autoreplace.items = vec![okbs_core::config::AutoReplaceItem {
            from: "tag".into(),
            to: "<b></b>".into(),
            cursor_pos: 3,
        }];
        config
            .autoreplace
            .items
            .push(okbs_core::config::AutoReplaceItem {
                from: "second".into(),
                to: "<i></i>".into(),
                cursor_pos: 3,
            });
        okbs_core::config::save(&path, &config).expect("private picker fixture operation failed");
    }
    let mut config = okbs_core::config::load_or_create(&path)
        .expect("private picker fixture operation failed")
        .config;
    if stage != "seed" {
        assert_eq!(config.general.theme, okbs_core::config::Theme::Dark);
    }
    if stage == "disabled" {
        assert!(!config.advanced.clipboard_history_persist);
    }
    assert_eq!(config.autoreplace.items[0].cursor_pos, 3);
    if stage != "seed" {
        assert_eq!(
            config.hotkeys.show_clipboard_history.0,
            Some("Ctrl+F11".parse().expect("saved history shortcut"))
        );
        assert_eq!(
            config.hotkeys.show_autoreplace_menu.0,
            Some("Ctrl+F12".parse().expect("saved list shortcut"))
        );
        assert_eq!(
            config.hotkeys.toggle_autoreplace_list.0,
            Some("Ctrl+F10".parse().expect("saved floating list shortcut"))
        );
    }
    let mut keys = AttributeSet::new();
    for &key in okbs_core::PhysKey::ALL {
        keys.insert(KeyCode(key.evdev_code()));
    }
    let mut hardware = VirtualDevice::builder()
        .expect("create private keyboard builder")
        .name("OKBS picker fixture keyboard")
        .with_keys(&keys)
        .expect("set private keys")
        .build()
        .expect("create private keyboard");
    let deadline = Instant::now() + Duration::from_secs(3);
    let hardware_node = loop {
        if let Some(path) = hardware
            .enumerate_dev_nodes_blocking()
            .expect("find private keyboard node")
            .filter_map(Result::ok)
            .find(|p| p.exists())
        {
            break path;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    config.linux.devices = vec![hardware_node.to_string_lossy().into_owned()];
    config.hotkeys.show_clipboard_history =
        okbs_core::config::HotkeyBinding::some("Ctrl+F11".parse().expect("history shortcut"));
    config.hotkeys.show_autoreplace_menu =
        okbs_core::config::HotkeyBinding::some("Ctrl+F12".parse().expect("picker shortcut"));
    config.hotkeys.toggle_autoreplace_list =
        okbs_core::config::HotkeyBinding::some("Ctrl+F10".parse().expect("floating list shortcut"));
    let (mut source, injector) = LinuxSource::new(&config, desktop.clone())
        .expect("private picker fixture operation failed");
    let deadline = Instant::now() + Duration::from_secs(3);
    let node = loop {
        if let Some(node) = source
            .virtual_nodes()
            .expect("private picker fixture operation failed")
            .into_iter()
            .find(|p| p.exists())
        {
            break node;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    let output_ready = directory.join(format!("{stage}-output-ready"));
    let output_idle = output_ready.with_extension("idle");
    let output = Process(
        ProcessCommand::new("python3")
            .arg(workspace.join("tools/linux-wayland-output-fixture.py"))
            .args([&node, &output_ready])
            .stdout(Stdio::null())
            .stderr(
                std::fs::File::create(directory.join(format!("{stage}-output.log")))
                    .expect("private picker fixture operation failed"),
            )
            .spawn()
            .expect("private picker fixture operation failed"),
    );
    let keyboard_command = directory.join(format!("{stage}-key"));
    let keyboard_ready = directory.join(format!("{stage}-key-ready"));
    let mut keyboard = if kde {
        let mut process = Process(
            ProcessCommand::new(workspace.join("tmp/linux-kde-keyboard/keyboard-fixture"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("private picker fixture operation failed"),
        );
        let input = process
            .0
            .stdin
            .take()
            .expect("private picker fixture operation failed");
        let mut reader = std::io::BufReader::new(
            process
                .0
                .stdout
                .take()
                .expect("private picker fixture operation failed"),
        );
        let mut ready = String::new();
        reader
            .read_line(&mut ready)
            .expect("private picker fixture operation failed");
        assert_eq!(ready.trim(), "ready");
        Keyboard::Kde {
            _process: process,
            input,
            output: reader,
        }
    } else {
        Keyboard::Gnome(zbus_keyboard::Remote::new(
            workspace,
            &keyboard_command,
            &keyboard_ready,
        ))
    };
    let deadline = Instant::now() + Duration::from_secs(8);
    while !output_ready.exists() || (!kde && !keyboard_ready.exists()) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let native = desktop
            .refresh()
            .expect("private picker fixture operation failed");
        if native.pid == editor.0.id() && native.control != 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "GTK editor unavailable {native:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let original = desktop
        .input_target()
        .expect("private picker fixture operation failed")
        .expect("private picker fixture operation failed");
    let (input_sender, input) = unbounded();
    let (clipboard_sink, clipboard_events) = unbounded();
    let mut clipboard = LinuxClipboard::new().expect("private picker fixture operation failed");
    let clipboard_watch = clipboard
        .subscribe(clipboard_sink)
        .expect("private picker fixture operation failed");
    let mut processor = Processor::new(
        config.clone(),
        Backends {
            injector: Box::new(injector),
            layouts: Box::new(desktop.clone()),
            clipboard: Some(Box::new(
                LinuxClipboard::new().expect("private picker fixture operation failed"),
            )),
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
            focus: None,
            clipboard: Some(clipboard_events),
        },
    )
    .expect("private picker fixture operation failed");
    let capture = source
        .start(input_sender)
        .expect("capture only the picker fixture keyboard");
    let physical_tap = |hardware: &mut VirtualDevice,
                        modifiers: &[okbs_core::PhysKey],
                        key: okbs_core::PhysKey| {
        for modifier in modifiers {
            hardware
                .emit(&[DeviceEvent::new(EventType::KEY.0, modifier.evdev_code(), 1)])
                .expect("press private modifier");
        }
        hardware
            .emit(&[DeviceEvent::new(EventType::KEY.0, key.evdev_code(), 1)])
            .expect("press private key");
        // A physical key remains down across UI frames; this acceptance is
        // deliberately separate from fast-typing/stress validation.
        std::thread::sleep(Duration::from_millis(40));
        hardware
            .emit(&[DeviceEvent::new(EventType::KEY.0, key.evdev_code(), 0)])
            .expect("release private key");
        for modifier in modifiers.iter().rev() {
            hardware
                .emit(&[DeviceEvent::new(EventType::KEY.0, modifier.evdev_code(), 0)])
                .expect("release private modifier");
        }
    };
    let click = |keyboard: &mut Keyboard, point: [i32; 2]| match keyboard {
        Keyboard::Gnome(remote) => remote.click(&keyboard_command, point),
        Keyboard::Kde { input, output, .. } => {
            writeln!(input, "p {} {}", point[0], point[1]).expect("send private pointer action");
            input.flush().expect("flush pointer");
            let mut reply = String::new();
            output.read_line(&mut reply).expect("pointer reply");
            assert_eq!(reply.trim(), format!("p {} {}", point[0], point[1]));
        }
    };
    let panels = okbs_platform_linux::panels::Panels::new(desktop.clone())
        .expect("private picker fixture operation failed");
    let popup_desktop = desktop.clone();
    let focus_desktop = desktop.clone();
    let picker_desktop = desktop.clone();
    let gate = source.filter.gate.clone();
    let test_gate = gate.clone();
    let picker_config = config.autoreplace.clone();
    let source_filter = source.filter.clone();
    let theme = config.general.theme;
    let picker_panels = panels.clone();
    let mut controller = Controller::new(
        Settings {
            config,
            path: path.clone(),
            read_only: false,
        },
        engine,
        PlatformHooks {
            on_apply: Some(Box::new(move |config| {
                source_filter.configure(config);
                Ok(())
            })),
            history_file: Some(history_file.clone()),
            popup_focus: Some(Box::new(move |title| {
                let Some(window) = focus_desktop.placed_window(title)? else {
                    return Ok(false);
                };
                focus_desktop.command("ActivateWindow", window.window)?;
                Ok(true)
            })),
            cursor_position: Some(Box::new(|| [40.0, 120.0])),
            popup_placement: Some(Box::new(move |title, point| {
                popup_desktop.place_owned_window(title, point)
            })),
            autoreplace_ui: Some(Box::new(move |window| {
                Ok(Box::new(crate::linux_ui::LinuxAutoreplaceUi::new(
                    picker_panels,
                    picker_desktop,
                    window.autoreplace_list(),
                    gate,
                    picker_config,
                    autoreplace_labels(okbs_core::Lang::En),
                    theme,
                )))
            })),
            ..PlatformHooks::default()
        },
        None,
        false,
    );
    let newest = "synthetic saved history";
    if stage == "seed" {
        let mut applied = controller.settings.config.clone();
        applied.general.theme = okbs_core::config::Theme::Dark;
        applied.advanced.clipboard_history_persist = true;
        controller.handle_window_event(SettingsEvent::Apply(Box::new(applied)));
        clipboard
            .set_text("synthetic older\nline two\\tail")
            .expect("private picker fixture operation failed");
        wait(&mut controller, &desktop, "older capture", |c| {
            c.history_entries.entries().len() == 1
        });
        clipboard
            .set_text("synthetic second row")
            .expect("private second row clipboard");
        wait(&mut controller, &desktop, "second row captured", |c| {
            c.history_entries.entries().len() == 2
        });
        clipboard
            .set_text(newest)
            .expect("private picker fixture operation failed");
        wait(&mut controller, &desktop, "newest capture", |c| {
            c.history_entries
                .entries()
                .first()
                .is_some_and(|s| s == newest)
        });
    } else if stage == "restore" {
        assert_eq!(
            controller.history_entries.entries(),
            [
                newest,
                "synthetic second row",
                "synthetic older\nline two\\tail"
            ]
        );
    } else {
        assert!(controller.history_entries.entries().is_empty());
        assert!(!history_file.exists());
        controller.shutdown();
        drop(capture);
        drop(clipboard_watch);
        drop(keyboard);
        drop(output);
        drop(editor);
        return;
    }
    wait_editor(&mut controller, &desktop, original);
    physical_tap(
        &mut hardware,
        &[okbs_core::PhysKey::ControlLeft],
        okbs_core::PhysKey::F11,
    );
    let title = controller.history_config().labels.title;
    wait(&mut controller, &desktop, "history visible/focused", |c| {
        c.history
            .as_ref()
            .expect("private picker fixture operation failed")
            .is_visible()
            && desktop.cached().title == title
    });
    assert_eq!(controller.history_target, Some(original));
    for _ in 0..25 {
        tick(&mut controller, &desktop);
    }
    physical_tap(&mut hardware, &[], okbs_core::PhysKey::ArrowDown);
    for _ in 0..20 {
        tick(&mut controller, &desktop);
    }
    physical_tap(&mut hardware, &[], okbs_core::PhysKey::Enter);
    wait_inserted(
        &mut controller,
        &desktop,
        "history inserted in original GTK field",
        &test_gate,
        &output_idle,
        |_| {
            std::fs::read(&state)
                .ok()
                .and_then(|s| serde_state(&s))
                .is_some_and(|s| s.0 == "synthetic second row" && s.1.is_empty())
        },
    );
    assert_eq!(
        desktop
            .input_target()
            .expect("private picker fixture operation failed"),
        Some(original)
    );
    // Reopening and cancelling must not insert another entry or leave the
    // keyboard in a hidden viewport after a genuine history selection.
    assert!(controller.engine.send(Command::ShowClipboardHistory));
    let history_title = controller.history_config().labels.title;
    wait(
        &mut controller,
        &desktop,
        "history reopened with keyboard focus",
        |_| desktop.cached().title == history_title,
    );
    for _ in 0..25 {
        tick(&mut controller, &desktop);
    }
    physical_tap(&mut hardware, &[], okbs_core::PhysKey::Escape);
    wait(&mut controller, &desktop, "history cancelled", |c| {
        !c.history
            .as_ref()
            .expect("private picker fixture operation failed")
            .is_visible()
    });
    wait(
        &mut controller,
        &desktop,
        "editor restored after Escape",
        |_| desktop.target() == Some(original),
    );
    // Select another actual row with the pointer, then use the same picker
    // confirmation path. Coordinates are relative to its native client.
    assert!(controller.engine.send(Command::ShowClipboardHistory));
    wait(&mut controller, &desktop, "mouse history picker", |_| {
        desktop.cached().title == history_title
    });
    for _ in 0..25 {
        tick(&mut controller, &desktop);
    }
    let [x, y, _, _] = desktop.cached().client;
    click(&mut keyboard, [x + 75, y + 12]);
    for _ in 0..15 {
        tick(&mut controller, &desktop);
    }
    physical_tap(&mut hardware, &[], okbs_core::PhysKey::Enter);
    let expected_base = "synthetic second rowsynthetic saved history";
    wait_inserted(
        &mut controller,
        &desktop,
        "pointer-selected history row",
        &test_gate,
        &output_idle,
        |_| {
            std::fs::read(&state)
                .ok()
                .and_then(|s| serde_state(&s))
                .is_some_and(|s| s.0 == expected_base)
        },
    );
    std::fs::write(&field_command, "first").expect("private picker fixture operation failed");
    wait_editor(&mut controller, &desktop, original);
    physical_tap(
        &mut hardware,
        &[okbs_core::PhysKey::ControlLeft],
        okbs_core::PhysKey::F12,
    );
    let title = autoreplace_labels(okbs_core::Lang::En).title;
    wait(
        &mut controller,
        &desktop,
        "autoreplace picker focused",
        |_| desktop.cached().title == title,
    );
    for _ in 0..25 {
        tick(&mut controller, &desktop);
    }
    physical_tap(&mut hardware, &[], okbs_core::PhysKey::ArrowDown);
    for _ in 0..20 {
        tick(&mut controller, &desktop);
    }
    hardware
        .emit(&[DeviceEvent::new(
            EventType::KEY.0,
            okbs_core::PhysKey::Enter.evdev_code(),
            1,
        )])
        .expect("hold picker confirmation during insertion");
    wait_inserted(
        &mut controller,
        &desktop,
        "autoreplace chosen in editor",
        &test_gate,
        &output_idle,
        |_| {
            std::fs::read(&state)
                .ok()
                .and_then(|s| serde_state(&s))
                .is_some_and(|s| s.0 == format!("{expected_base}<i></i>") && s.1.is_empty())
        },
    );
    hardware
        .emit(&[DeviceEvent::new(
            EventType::KEY.0,
            okbs_core::PhysKey::Enter.evdev_code(),
            0,
        )])
        .expect("release held picker confirmation");
    let released = std::time::SystemTime::now();
    wait(
        &mut controller,
        &desktop,
        "held confirmation release delivered",
        |_| {
            std::fs::metadata(&output_idle)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t >= released)
        },
    );
    // The passive panel is a different native implementation from the egui
    // picker. A genuine pointer click must keep keyboard focus in the editor.
    std::fs::write(&field_command, "first").expect("move fixture caret to end");
    wait(&mut controller, &desktop, "fixture caret at end", |_| {
        !field_command.exists()
    });
    physical_tap(
        &mut hardware,
        &[okbs_core::PhysKey::ControlLeft],
        okbs_core::PhysKey::F10,
    );
    wait(&mut controller, &desktop, "floating list mapped", |_| {
        panels.row_center("list", 1).ok().flatten().is_some()
    });
    assert_eq!(
        desktop.target(),
        Some(original),
        "passive list stole editor focus"
    );
    let point = panels
        .row_center("list", 1)
        .expect("floating row geometry")
        .expect("second native row");
    click(&mut keyboard, point);
    let expected_floating = format!("{expected_base}<i></i><i></i>");
    wait_inserted(
        &mut controller,
        &desktop,
        "second floating row inserted",
        &test_gate,
        &output_idle,
        |_| {
            std::fs::read(&state)
                .ok()
                .and_then(|s| serde_state(&s))
                .is_some_and(|s| s.0 == expected_floating && s.1.is_empty())
        },
    );
    assert!(
        panels.visible("list"),
        "floating list must remain available after selection"
    );
    wait_editor(&mut controller, &desktop, original);
    let before_enter: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state).expect("synthetic field snapshot"))
            .expect("synthetic activation counters");
    assert_eq!(
        before_enter["activations"]["first"].as_u64(),
        Some(0),
        "picker confirmation leaked into editor"
    );
    physical_tap(&mut hardware, &[], okbs_core::PhysKey::Enter);
    wait(
        &mut controller,
        &desktop,
        "next Enter reaches editor after pointer selection",
        |_| {
            std::fs::read(&state)
                .ok()
                .and_then(|s| serde_json::from_slice::<serde_json::Value>(&s).ok())
                .is_some_and(|s| {
                    s["activations"]["first"].as_u64() == Some(1)
                        && s["activations"]["second"].as_u64() == Some(0)
                })
        },
    );
    std::fs::write(&field_command, "second").expect("switch fixture field");
    wait(&mut controller, &desktop, "second editor focused", |_| {
        desktop.target().is_some_and(|target| {
            target.window == original.window && target.control != original.control
        })
    });
    let second_target = desktop.target().expect("second editor target");
    click(
        &mut keyboard,
        panels
            .row_center("list", 0)
            .expect("floating first row geometry")
            .expect("first native row"),
    );
    wait_inserted(
        &mut controller,
        &desktop,
        "floating insertion follows field switch",
        &test_gate,
        &output_idle,
        |_| {
            std::fs::read(&state)
                .ok()
                .and_then(|s| serde_state(&s))
                .is_some_and(|s| s.0 == expected_floating && s.1 == "<b></b>")
        },
    );
    wait_editor(&mut controller, &desktop, second_target);
    let mut disabled_autoreplace = controller.settings.config.clone();
    disabled_autoreplace.autoreplace.enabled = false;
    controller.handle_window_event(SettingsEvent::Apply(Box::new(disabled_autoreplace)));
    for _ in 0..25 {
        tick(&mut controller, &desktop);
    }
    click(
        &mut keyboard,
        panels
            .row_center("list", 0)
            .expect("disabled panel geometry")
            .expect("disabled panel row"),
    );
    for _ in 0..30 {
        tick(&mut controller, &desktop);
    }
    assert_eq!(
        serde_state(&std::fs::read(&state).expect("fixture state")).expect("synthetic fields"),
        (expected_floating.clone(), "<b></b>".into()),
        "disabled floating panel inserted text"
    );
    let mut enabled = controller.settings.config.clone();
    enabled.autoreplace.enabled = true;
    controller.handle_window_event(SettingsEvent::Apply(Box::new(enabled)));
    for _ in 0..25 {
        tick(&mut controller, &desktop);
    }
    std::fs::write(&field_command, "password").expect("focus private password field");
    wait(&mut controller, &desktop, "password role confirmed", |_| {
        matches!(desktop.is_password_field(), Ok(Some(true)))
    });
    click(
        &mut keyboard,
        panels
            .row_center("list", 0)
            .expect("password panel geometry")
            .expect("native row"),
    );
    for _ in 0..50 {
        tick(&mut controller, &desktop);
    }
    assert_eq!(
        serde_state(&std::fs::read(&state).expect("fixture state")).expect("synthetic fields"),
        (expected_floating.clone(), "<b></b>".into()),
        "floating panel inserted in a password field"
    );
    physical_tap(
        &mut hardware,
        &[okbs_core::PhysKey::ControlLeft],
        okbs_core::PhysKey::F10,
    );
    wait(
        &mut controller,
        &desktop,
        "floating list hidden by shortcut",
        |_| !panels.visible("list"),
    );
    std::fs::write(&field_command, "first").expect("return to original fixture field");
    wait(
        &mut controller,
        &desktop,
        "original editor restored",
        |_| desktop.target() == Some(original),
    );
    if stage == "restore" {
        assert!(
            controller
                .engine
                .send(Command::AutoreplaceList { toggle: false })
        );
        let title = autoreplace_labels(okbs_core::Lang::En).title;
        wait(&mut controller, &desktop, "autoreplace reopened", |_| {
            desktop.cached().title == title
        });
        for _ in 0..25 {
            tick(&mut controller, &desktop);
        }
        physical_tap(&mut hardware, &[], okbs_core::PhysKey::Escape);
        wait(
            &mut controller,
            &desktop,
            "autoreplace Escape returned to editor",
            |_| desktop.target() == Some(original),
        );
        assert_eq!(
            serde_state(&std::fs::read(&state).expect("private picker fixture operation failed"))
                .expect("private picker fixture operation failed")
                .0,
            expected_floating
        );
        let mut disabled = controller.settings.config.clone();
        disabled.advanced.clipboard_history_persist = false;
        controller.handle_window_event(SettingsEvent::Apply(Box::new(disabled)));
        assert!(!history_file.exists());
    }
    controller.shutdown();
    drop(capture);
    drop(clipboard_watch);
    drop(keyboard);
    drop(output);
    drop(editor);
}
fn serde_state(bytes: &[u8]) -> Option<(String, String)> {
    // The observation process reports only fields it created for this fixture.
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    Some((
        value["first"].as_str()?.to_string(),
        value["second"].as_str()?.to_string(),
    ))
}
