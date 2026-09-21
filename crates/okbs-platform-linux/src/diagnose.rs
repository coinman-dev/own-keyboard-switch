//! `okbswitch --diagnose` checks for Linux.

use crate::access::{self, DeviceAccess};
use crate::session::{self, Desktop, SessionInfo, SessionType};
use okbs_core::config::{Config, LayoutBackend};
use okbs_platform::{DiagnosticReport, DiagnosticStatus};

/// Collects environment facts relevant to the program.
pub fn diagnose(config: &Config) -> DiagnosticReport {
    let mut r = DiagnosticReport::default();
    let s = SessionInfo::detect();

    let session_name = match s.session_type {
        SessionType::Wayland => "Wayland",
        SessionType::X11 => "X11",
        SessionType::Tty => "tty",
        SessionType::Unknown => "unknown",
    };
    let session_status = match s.session_type {
        SessionType::Wayland | SessionType::X11 => DiagnosticStatus::Ok,
        _ => DiagnosticStatus::Warning,
    };
    r.push(session_status, "session", session_name);

    let (desktop_status, desktop_name) = match &s.desktop {
        Desktop::Gnome => (DiagnosticStatus::Ok, "GNOME".to_string()),
        Desktop::Kde => (DiagnosticStatus::Ok, "KDE Plasma".to_string()),
        Desktop::Other(name) => (DiagnosticStatus::Warning, format!("{name} (not tested)")),
        Desktop::Unknown => (DiagnosticStatus::Warning, "unknown".to_string()),
    };
    r.push(desktop_status, "desktop", desktop_name);

    if s.wsl {
        r.push(
            DiagnosticStatus::Warning,
            "wsl",
            "running inside WSL: no /dev/input, keyboard capture cannot work here",
        );
    }
    r.push(
        if s.has_session_bus {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Warning
        },
        "session bus",
        if s.has_session_bus {
            "DBUS_SESSION_BUS_ADDRESS set"
        } else {
            "DBUS_SESSION_BUS_ADDRESS not set"
        },
    );

    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let extension = session::gnome_extension_installed(home.as_deref());
    let backend = session::resolve_layout_backend(config.linux.layout_backend, &s, extension);
    let backend_name = match backend {
        LayoutBackend::Auto => "auto",
        LayoutBackend::Kde => "kde (org.kde.keyboard D-Bus)",
        LayoutBackend::X11 => "x11 (XKB extension)",
        LayoutBackend::GnomeExtension => "gnome-extension (bundled GNOME Shell extension)",
        LayoutBackend::GnomeFallback => "gnome-fallback (gsettings + switch shortcut)",
        LayoutBackend::Internal => "internal (layout state is tracked by the program)",
    };
    let backend_status = match backend {
        LayoutBackend::Kde | LayoutBackend::X11 | LayoutBackend::GnomeExtension => {
            DiagnosticStatus::Ok
        }
        _ => DiagnosticStatus::Warning,
    };
    r.push(backend_status, "layout backend", backend_name);
    if s.desktop == Desktop::Gnome {
        r.push(
            if extension {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Warning
            },
            "gnome extension",
            if extension {
                "installed".to_string()
            } else {
                format!(
                    "{} not installed: program exclusions and password detection are limited",
                    session::GNOME_EXTENSION_UUID
                )
            },
        );
    }

    let scan = access::scan_input_devices();
    if !scan.input_dir_exists {
        r.push(
            DiagnosticStatus::Error,
            "input devices",
            "/dev/input does not exist",
        );
    } else if scan.keyboards.is_empty() {
        let hint = if scan.denied > 0 {
            format!(
                "{} of {} event devices are not readable; install the package udev rule or add the user to the `input` group",
                scan.denied, scan.event_nodes
            )
        } else {
            format!("{} event devices, none is a keyboard", scan.event_nodes)
        };
        r.push(DiagnosticStatus::Error, "input devices", hint);
    } else {
        let names: Vec<String> = scan
            .keyboards
            .iter()
            .map(|k| format!("{} ({})", k.name, k.path.display()))
            .collect();
        let status = if scan.denied > 0 {
            DiagnosticStatus::Warning
        } else {
            DiagnosticStatus::Ok
        };
        r.push(status, "keyboards", names.join(", "));
    }

    let (uinput_status, uinput_detail) = match access::uinput_access() {
        DeviceAccess::Granted => (DiagnosticStatus::Ok, "writable"),
        DeviceAccess::Denied => (
            DiagnosticStatus::Error,
            "permission denied; install the package udev rule or add the user to the `input` group",
        ),
        DeviceAccess::Missing => (
            DiagnosticStatus::Error,
            "missing; load the `uinput` kernel module",
        ),
        DeviceAccess::Failed => (DiagnosticStatus::Error, "cannot be opened"),
    };
    r.push(uinput_status, "/dev/uinput", uinput_detail);

    match access::udev_rule() {
        Some(path) => r.push(DiagnosticStatus::Ok, "udev rule", path),
        None => r.info("udev rule", "not installed"),
    }
    if let Some(member) = access::in_input_group() {
        r.info(
            "input group",
            if member { "member" } else { "not a member" },
        );
    }
    r
}
