//! Desktop files are embedded so the AppImage can install its own integration.
use crate::desktop::error;
use crate::session::{Desktop, GNOME_EXTENSION_UUID};
use okbs_platform::Result;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    Enabled,
    SessionRestart,
}

fn data_root() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| error("HOME is not set"))?;
    Ok(std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(home).join(".local/share")))
}

/// Install only the current desktop's component, preserving other extensions
/// and scripts. Enabling it never needs administrator authorization.
pub fn install_current(desktop: &Desktop) -> Result<Activation> {
    if crate::permissions::is_root() {
        return Err(error(
            "desktop integration must be installed as a regular user",
        ));
    }
    let data = data_root()?;
    write_components(
        &data,
        matches!(desktop, Desktop::Gnome),
        matches!(desktop, Desktop::Kde),
    )?;
    let bus = zbus::blocking::connection::Builder::session()
        .map_err(error)?
        .method_timeout(Duration::from_millis(500))
        .build()
        .map_err(error)?;
    match desktop {
        Desktop::Gnome => {
            let proxy = zbus::blocking::Proxy::new(
                &bus,
                "org.gnome.Shell",
                "/org/gnome/Shell",
                "org.gnome.Shell.Extensions",
            )
            .map_err(error)?;
            if !proxy
                .get_property::<bool>("UserExtensionsEnabled")
                .map_err(error)?
            {
                return Err(error("user extensions are disabled in GNOME"));
            }
            let enabled: bool = proxy
                .call("EnableExtension", &(GNOME_EXTENSION_UUID,))
                .map_err(error)?;
            if !enabled {
                queue_gnome_activation()?;
            }
            let deadline = Instant::now() + Duration::from_millis(1500);
            loop {
                let active = zbus::blocking::Proxy::new(
                    &bus,
                    "org.own_keyboard_switch.Gnome",
                    "/org/own_keyboard_switch/Gnome",
                    "org.own_keyboard_switch.Gnome",
                )
                .ok()
                .and_then(|service| service.call::<_, _, u32>("GetVersion", &()).ok())
                .is_some_and(|version| version >= crate::desktop::INTEGRATION_PROTOCOL_VERSION);
                if active {
                    return Ok(Activation::Enabled);
                }
                if Instant::now() >= deadline {
                    return Ok(Activation::SessionRestart);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Desktop::Kde => {
            enable_kde_at_login()?;
            let scripting = zbus::blocking::Proxy::new(
                &bus,
                "org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting",
            )
            .map_err(error)?;
            let loaded: bool = scripting
                .call("isScriptLoaded", &("okbswitch",))
                .map_err(error)?;
            if loaded {
                scripting
                    .call::<_, _, bool>("unloadScript", &("okbswitch",))
                    .map_err(error)?;
            }
            let path = data.join("kwin/scripts/okbswitch/contents/code/main.js");
            let id: i32 = scripting
                .call(
                    "loadScript",
                    &(path.to_string_lossy().as_ref(), "okbswitch"),
                )
                .map_err(error)?;
            if id < 0 {
                return Err(error("KWin did not load the integration script"));
            }
            let script = zbus::blocking::Proxy::new(
                &bus,
                "org.kde.KWin",
                format!("/Scripting/Script{id}"),
                "org.kde.kwin.Script",
            )
            .map_err(error)?;
            script.call::<_, _, ()>("run", &()).map_err(error)?;
            Ok(Activation::Enabled)
        }
        _ => Err(error("this desktop has no bundled integration")),
    }
}
fn queue_gnome_activation() -> Result<()> {
    use gtk::gio::{self, prelude::*};
    let schema = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.shell", true))
        .ok_or_else(|| error("GNOME settings schema is unavailable"))?;
    let settings = gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None);
    let mut enabled: Vec<String> = settings
        .strv("enabled-extensions")
        .iter()
        .map(|name| name.to_string())
        .collect();
    if !enabled.iter().any(|name| name == GNOME_EXTENSION_UUID) {
        enabled.push(GNOME_EXTENSION_UUID.into());
    }
    settings.delay();
    let result = (|| {
        settings
            .set_strv(
                "enabled-extensions",
                enabled
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .map_err(error)?;
        if schema.has_key("disabled-extensions") {
            let disabled: Vec<String> = settings
                .strv("disabled-extensions")
                .iter()
                .filter(|name| name.as_str() != GNOME_EXTENSION_UUID)
                .map(|name| name.to_string())
                .collect();
            settings
                .set_strv(
                    "disabled-extensions",
                    disabled
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .as_slice(),
                )
                .map_err(error)?;
        }
        Ok(())
    })();
    if result.is_err() {
        settings.revert();
        return result;
    }
    settings.apply();
    gio::Settings::sync();
    Ok(())
}
fn enable_kde_at_login() -> Result<()> {
    for program in ["kwriteconfig6", "kwriteconfig5"] {
        let mut child = match std::process::Command::new(program)
            .args([
                "--file",
                "kwinrc",
                "--group",
                "Plugins",
                "--key",
                "okbswitchEnabled",
                "true",
            ])
            .spawn()
        {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(error(e)),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().map_err(error)? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(error("KDE integration could not be enabled at login"))
                };
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error("KDE configuration command timed out"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    Err(error("KDE configuration utility is not installed"))
}

pub fn install() -> Result<()> {
    write_components(&data_root()?, true, true)
}
fn write_components(data: &std::path::Path, gnome_enabled: bool, kde_enabled: bool) -> Result<()> {
    let gnome = data.join("gnome-shell/extensions/okbswitch@own-keyboard-switch");
    let kde = data.join("kwin/scripts/okbswitch");
    if gnome_enabled {
        std::fs::create_dir_all(&gnome).map_err(error)?;
    }
    if kde_enabled {
        std::fs::create_dir_all(kde.join("contents/code")).map_err(error)?;
    }
    for (enabled, path, contents) in [
        (
            gnome_enabled,
            gnome.join("metadata.json"),
            include_str!("../../../packaging/linux/gnome/metadata.json"),
        ),
        (
            gnome_enabled,
            gnome.join("extension.js"),
            include_str!("../../../packaging/linux/gnome/extension.js"),
        ),
        (
            kde_enabled,
            kde.join("metadata.json"),
            include_str!("../../../packaging/linux/kde/metadata.json"),
        ),
        (
            kde_enabled,
            kde.join("contents/code/main.js"),
            include_str!("../../../packaging/linux/kde/contents/code/main.js"),
        ),
    ] {
        if enabled {
            std::fs::write(path, contents).map_err(error)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installing_one_desktop_preserves_other_components() {
        let scratch =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/linux-integrations");
        std::fs::create_dir_all(&scratch).unwrap();
        let temp = tempfile::TempDir::new_in(scratch).unwrap();
        let unrelated = temp
            .path()
            .join("gnome-shell/extensions/unrelated/extension.js");
        std::fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
        std::fs::write(&unrelated, "unrelated component").unwrap();
        write_components(temp.path(), true, false).unwrap();
        assert!(
            temp.path()
                .join("gnome-shell/extensions/okbswitch@own-keyboard-switch/extension.js")
                .is_file()
        );
        assert!(!temp.path().join("kwin/scripts/okbswitch").exists());
        write_components(temp.path(), false, true).unwrap();
        assert!(
            temp.path()
                .join("kwin/scripts/okbswitch/contents/code/main.js")
                .is_file()
        );
        assert_eq!(
            std::fs::read_to_string(unrelated).unwrap(),
            "unrelated component"
        );
    }
    #[test]
    #[ignore = "requires isolated GNOME Shell and settings; changes only the test session"]
    fn gnome_setup_reenables_the_component_and_preserves_other_settings() {
        use gtk::gio::prelude::*;
        use okbs_platform::LayoutManager;
        let bus = zbus::blocking::Connection::session().unwrap();
        let proxy = zbus::blocking::Proxy::new(
            &bus,
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.gnome.Shell.Extensions",
        )
        .unwrap();
        assert!(
            proxy
                .call::<_, _, bool>("DisableExtension", &(GNOME_EXTENSION_UUID,))
                .unwrap()
        );
        assert_eq!(
            install_current(&Desktop::Gnome).unwrap(),
            if std::env::var("OKBS_TEST_OLD_EXTENSION").as_deref() == Ok("1") {
                Activation::SessionRestart
            } else {
                Activation::Enabled
            }
        );
        assert!(
            crate::session::gnome_extension_installed(None),
            "the installed component must be found in XDG_DATA_HOME"
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(desktop) = crate::desktop::LinuxDesktop::connect() {
                assert_eq!(desktop.layouts().unwrap().len(), 2);
                let mut changed = desktop.clone();
                let mut config = okbs_core::config::Config::default();
                config.linux.layout_backend = okbs_core::config::LayoutBackend::GnomeExtension;
                if std::env::var("OKBS_TEST_OLD_EXTENSION").as_deref() == Ok("1") {
                    assert!(!desktop.integration_ready());
                    assert!(changed.prepare_layout(&config).is_err());
                } else {
                    changed
                        .apply_layout(changed.prepare_layout(&config).unwrap().unwrap())
                        .unwrap();
                    let original = desktop.current().unwrap();
                    changed
                        .set(okbs_platform::LayoutId(1 - original.0))
                        .unwrap();
                    assert_eq!(
                        desktop.current().unwrap(),
                        okbs_platform::LayoutId(1 - original.0)
                    );
                    changed.set(original).unwrap();
                }
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        let settings = gtk::gio::Settings::new("org.gnome.shell");
        let before = settings.boolean("disable-user-extensions");
        settings
            .set_strv("enabled-extensions", ["unrelated-enabled@fixture"])
            .unwrap();
        settings
            .set_strv(
                "disabled-extensions",
                [GNOME_EXTENSION_UUID, "unrelated-disabled@fixture"],
            )
            .unwrap();
        queue_gnome_activation().unwrap();
        let enabled: Vec<String> = settings
            .strv("enabled-extensions")
            .iter()
            .map(|name| name.to_string())
            .collect();
        let disabled: Vec<String> = settings
            .strv("disabled-extensions")
            .iter()
            .map(|name| name.to_string())
            .collect();
        assert!(
            enabled
                .iter()
                .any(|name| name == "unrelated-enabled@fixture")
        );
        assert!(enabled.iter().any(|name| name == GNOME_EXTENSION_UUID));
        assert_eq!(disabled, ["unrelated-disabled@fixture"]);
        assert_eq!(settings.boolean("disable-user-extensions"), before);
    }
    #[test]
    #[ignore = "requires isolated GNOME Wayland with two virtual monitors at different scales"]
    fn gnome_panel_is_clamped_on_a_second_scaled_monitor() {
        let bus = zbus::blocking::Connection::session().unwrap();
        let proxy = zbus::blocking::Proxy::new(
            &bus,
            "org.own_keyboard_switch.Gnome",
            "/org/own_keyboard_switch/Gnome",
            "org.own_keyboard_switch.Gnome",
        )
        .unwrap();
        let state: String = proxy.call("GetPanelGeometry", &("monitor-proof",)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&state).unwrap();
        let monitors = value["monitors"].as_array().unwrap();
        assert_eq!(monitors.len(), 2);
        let first = &monitors[0];
        let second = &monitors[1];
        let (x, y) = (second["x"].as_i64().unwrap(), second["y"].as_i64().unwrap());
        let (width, height) = (
            second["width"].as_i64().unwrap(),
            second["height"].as_i64().unwrap(),
        );
        assert_ne!(
            (x, y),
            (first["x"].as_i64().unwrap(), first["y"].as_i64().unwrap())
        );
        assert!(
            second["scale"].as_f64().unwrap() >= 1.5,
            "virtual monitor scaling did not activate: {monitors:?}"
        );
        let request = serde_json::json!({"id":"monitor-proof", "visible":true,
            "position":[x + width - 2, y + height - 2], "text":"isolated fixture", "opacity":1});
        proxy
            .call::<_, _, ()>("SetPanel", &(request.to_string(),))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let state: String = proxy.call("GetPanelGeometry", &("monitor-proof",)).unwrap();
            let value: serde_json::Value = serde_json::from_str(&state).unwrap();
            let panel = &value["panel"];
            if let (Some(px), Some(py), Some(pw), Some(ph)) = (
                panel["x"].as_i64(),
                panel["y"].as_i64(),
                panel["width"].as_i64(),
                panel["height"].as_i64(),
            ) && pw > 0
                && ph > 0
                && px >= x
                && py >= y
                && px + pw <= x + width
                && py + ph <= y + height
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "secondary panel escaped its monitor: {value}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        proxy
            .call::<_, _, ()>(
                "SetPanel",
                &(serde_json::json!({"id":"monitor-proof", "visible":false}).to_string(),),
            )
            .unwrap();
    }
    #[test]
    #[ignore = "requires isolated GNOME Wayland with two virtual monitors"]
    fn gnome_places_a_real_window_on_the_requested_secondary_monitor() {
        use gtk::prelude::*;
        gtk::init().unwrap();
        let bus = zbus::blocking::Connection::session().unwrap();
        let proxy = zbus::blocking::Proxy::new(
            &bus,
            "org.own_keyboard_switch.Gnome",
            "/org/own_keyboard_switch/Gnome",
            "org.own_keyboard_switch.Gnome",
        )
        .unwrap();
        let state: String = proxy.call("GetPanelGeometry", &("monitor-proof",)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&state).unwrap();
        let secondary = &value["monitors"][1];
        let point = [
            secondary["x"].as_i64().unwrap() as f32 + 80.0,
            secondary["y"].as_i64().unwrap() as f32 + 90.0,
        ];
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("OKBS GNOME secondary fixture");
        window.set_decorated(false);
        window.set_default_size(320, 200);
        window.add(&gtk::Label::new(Some("isolated test")));
        window.show_all();
        window.present();
        let desktop = crate::desktop::LinuxDesktop::connect().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            let state = desktop.refresh().unwrap();
            if state.pid == std::process::id() {
                break;
            }
            assert!(Instant::now() < deadline, "GTK fixture did not gain focus");
            std::thread::sleep(Duration::from_millis(10));
        }
        desktop
            .place_owned_window("OKBS GNOME secondary fixture", point)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            let state = desktop.refresh().unwrap();
            if state.pid == std::process::id()
                && state.frame[0] == point[0] as i32
                && state.frame[1] == point[1] as i32
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "GTK fixture stayed on its original monitor: {:?}",
                state.frame
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        window.close();
    }
    #[test]
    #[ignore = "requires isolated GNOME Wayland with two virtual monitors; removes only a test output"]
    fn gnome_repositions_a_panel_when_the_second_monitor_disappears() {
        let bus = zbus::blocking::Connection::session().unwrap();
        let proxy = zbus::blocking::Proxy::new(
            &bus,
            "org.own_keyboard_switch.Gnome",
            "/org/own_keyboard_switch/Gnome",
            "org.own_keyboard_switch.Gnome",
        )
        .unwrap();
        let current: String = proxy.call("GetPanelGeometry", &("hotplug-proof",)).unwrap();
        let current: serde_json::Value = serde_json::from_str(&current).unwrap();
        let second = &current["monitors"][1];
        let requested = [
            second["x"].as_i64().unwrap() + 80,
            second["y"].as_i64().unwrap() + 80,
        ];
        proxy
            .call::<_, _, ()>(
                "SetPanel",
                &(serde_json::json!({"id":"hotplug-proof", "visible":true,
            "position":requested, "text":"isolated fixture", "opacity":1})
                .to_string(),),
            )
            .unwrap();
        let status = std::process::Command::new("gdctl")
            .args(["set", "-L", "-M", "Meta-1", "--primary"])
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let current: String = proxy.call("GetPanelGeometry", &("hotplug-proof",)).unwrap();
            let value: serde_json::Value = serde_json::from_str(&current).unwrap();
            let monitors = value["monitors"].as_array().unwrap();
            if monitors.len() == 1 {
                let area = &monitors[0];
                let panel = &value["panel"];
                let inside = (|| {
                    Some(
                        panel["x"].as_i64()? >= area["x"].as_i64()?
                            && panel["y"].as_i64()? >= area["y"].as_i64()?
                            && panel["x"].as_i64()? + panel["width"].as_i64()?
                                <= area["x"].as_i64()? + area["width"].as_i64()?
                            && panel["y"].as_i64()? + panel["height"].as_i64()?
                                <= area["y"].as_i64()? + area["height"].as_i64()?,
                    )
                })();
                if inside == Some(true) {
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "panel remained outside the connected monitor: {value}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        proxy
            .call::<_, _, ()>(
                "SetPanel",
                &(serde_json::json!({"id":"hotplug-proof", "visible":false}).to_string(),),
            )
            .unwrap();
    }
    #[test]
    #[ignore = "requires isolated KDE Wayland and settings; installs only in its XDG directory"]
    fn kde_setup_loads_the_script_and_enables_it_at_login() {
        use okbs_platform::LayoutManager;
        let _bridge = crate::desktop::serve_kde_bridge().unwrap();
        let installed = data_root()
            .unwrap()
            .join("kwin/scripts/okbswitch/contents/code/main.js");
        assert!(
            !std::fs::read_to_string(&installed)
                .unwrap()
                .contains("protocol:5")
        );
        assert_eq!(install_current(&Desktop::Kde).unwrap(), Activation::Enabled);
        assert!(
            std::fs::read_to_string(&installed)
                .unwrap()
                .contains("protocol:5")
        );
        let desktop = crate::desktop::LinuxDesktop::connect().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !desktop.integration_ready() {
            assert!(
                Instant::now() < deadline,
                "newly installed KWin script did not connect"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut config = okbs_core::config::Config::default();
        config.linux.layout_backend = okbs_core::config::LayoutBackend::Kde;
        let mut changed = desktop.clone();
        changed
            .apply_layout(changed.prepare_layout(&config).unwrap().unwrap())
            .unwrap();
        let original = desktop.current().unwrap();
        changed
            .set(okbs_platform::LayoutId(1 - original.0))
            .unwrap();
        assert_eq!(
            desktop.current().unwrap(),
            okbs_platform::LayoutId(1 - original.0)
        );
        changed.set(original).unwrap();
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap()
            .join("kwinrc");
        assert!(
            std::fs::read_to_string(config)
                .unwrap()
                .contains("okbswitchEnabled=true")
        );
        let bus = zbus::blocking::Connection::session().unwrap();
        let proxy = zbus::blocking::Proxy::new(
            &bus,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .unwrap();
        assert!(
            proxy
                .call::<_, _, bool>("unloadScript", &("okbswitch",))
                .unwrap()
        );
    }
}
