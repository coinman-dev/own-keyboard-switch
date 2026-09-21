//! Detection of the graphical session and choice of the layout backend.

use okbs_core::config::LayoutBackend;
use std::path::Path;

/// Display server protocol of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    /// Wayland compositor.
    Wayland,
    /// X11 server.
    X11,
    /// Text console.
    Tty,
    /// Could not be determined.
    Unknown,
}

/// Desktop environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Desktop {
    /// GNOME Shell (including the Ubuntu session).
    Gnome,
    /// KDE Plasma.
    Kde,
    /// Another desktop, with its `XDG_CURRENT_DESKTOP` value.
    Other(String),
    /// No desktop information.
    Unknown,
}

/// Facts about the current session gathered from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// Protocol.
    pub session_type: SessionType,
    /// Desktop environment.
    pub desktop: Desktop,
    /// `WAYLAND_DISPLAY`.
    pub wayland_display: Option<String>,
    /// `DISPLAY`.
    pub x_display: Option<String>,
    /// `DBUS_SESSION_BUS_ADDRESS` is set.
    pub has_session_bus: bool,
    /// Running inside Windows Subsystem for Linux.
    pub wsl: bool,
}

impl SessionInfo {
    /// Reads the real process environment.
    pub fn detect() -> Self {
        let osrelease = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        Self::from_env(|name| std::env::var(name).ok(), &osrelease)
    }

    /// Builds the info from an environment lookup (for tests).
    pub fn from_env(get: impl Fn(&str) -> Option<String>, kernel_release: &str) -> Self {
        let non_empty = |name: &str| get(name).filter(|v| !v.trim().is_empty());
        let wayland_display = non_empty("WAYLAND_DISPLAY");
        let x_display = non_empty("DISPLAY");

        let session_type = match non_empty("XDG_SESSION_TYPE")
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("wayland") => SessionType::Wayland,
            Some("x11") => SessionType::X11,
            Some("tty") => SessionType::Tty,
            _ if wayland_display.is_some() => SessionType::Wayland,
            _ if x_display.is_some() => SessionType::X11,
            _ => SessionType::Unknown,
        };

        let current = non_empty("XDG_CURRENT_DESKTOP")
            .or_else(|| non_empty("XDG_SESSION_DESKTOP"))
            .or_else(|| non_empty("DESKTOP_SESSION"));
        let desktop = match current {
            Some(value) => {
                let lower = value.to_ascii_lowercase();
                let parts: Vec<&str> = lower.split([':', ';']).collect();
                if parts.iter().any(|p| *p == "kde" || p.contains("plasma")) {
                    Desktop::Kde
                } else if parts.iter().any(|p| p.contains("gnome") || *p == "ubuntu") {
                    Desktop::Gnome
                } else {
                    Desktop::Other(value)
                }
            }
            None if non_empty("KDE_FULL_SESSION").is_some() => Desktop::Kde,
            None if non_empty("GNOME_SETUP_DISPLAY").is_some() => Desktop::Gnome,
            None => Desktop::Unknown,
        };

        let release = kernel_release.to_ascii_lowercase();
        Self {
            session_type,
            desktop,
            wayland_display,
            x_display,
            has_session_bus: non_empty("DBUS_SESSION_BUS_ADDRESS").is_some(),
            wsl: non_empty("WSL_DISTRO_NAME").is_some()
                || release.contains("microsoft")
                || release.contains("wsl"),
        }
    }
}

/// Picks the concrete layout backend for `setting`.
///
/// `Auto` resolves to KDE D-Bus on Plasma (X11 and Wayland), the GNOME Shell
/// extension or its fallback on GNOME, XKB on other X11 sessions and the
/// internal counter elsewhere. X11 is never chosen for a Wayland session,
/// because XWayland only covers X11 clients.
pub fn resolve_layout_backend(
    setting: LayoutBackend,
    session: &SessionInfo,
    gnome_extension_available: bool,
) -> LayoutBackend {
    if setting != LayoutBackend::Auto {
        return setting;
    }
    match (&session.desktop, session.session_type) {
        (Desktop::Kde, _) => LayoutBackend::Kde,
        (Desktop::Gnome, _) if gnome_extension_available => LayoutBackend::GnomeExtension,
        (Desktop::Gnome, _) => LayoutBackend::GnomeFallback,
        (_, SessionType::X11) => LayoutBackend::X11,
        _ => LayoutBackend::Internal,
    }
}

/// UUID of the bundled GNOME Shell extension.
pub const GNOME_EXTENSION_UUID: &str = "okbswitch@own-keyboard-switch";

/// Whether the bundled GNOME Shell extension is installed for the user or system-wide.
pub fn gnome_extension_installed(home: Option<&Path>) -> bool {
    let user = home.map(|h| {
        h.join(".local/share/gnome-shell/extensions")
            .join(GNOME_EXTENSION_UUID)
            .join("metadata.json")
    });
    let system = Path::new("/usr/share/gnome-shell/extensions")
        .join(GNOME_EXTENSION_UUID)
        .join("metadata.json");
    user.is_some_and(|p| p.exists()) || system.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn session(vars: &[(&str, &str)], release: &str) -> SessionInfo {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        SessionInfo::from_env(|k| map.get(k).cloned(), release)
    }

    #[test]
    fn ubuntu_gnome_wayland() {
        let s = session(
            &[
                ("XDG_SESSION_TYPE", "wayland"),
                ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
                ("WAYLAND_DISPLAY", "wayland-0"),
                ("DISPLAY", ":0"),
            ],
            "6.17.0-5-generic",
        );
        assert_eq!(s.session_type, SessionType::Wayland);
        assert_eq!(s.desktop, Desktop::Gnome);
        assert!(!s.wsl);
        assert_eq!(
            resolve_layout_backend(LayoutBackend::Auto, &s, false),
            LayoutBackend::GnomeFallback
        );
        assert_eq!(
            resolve_layout_backend(LayoutBackend::Auto, &s, true),
            LayoutBackend::GnomeExtension
        );
    }

    #[test]
    fn kubuntu_wayland_and_x11() {
        for kind in ["wayland", "x11"] {
            let s = session(
                &[("XDG_SESSION_TYPE", kind), ("XDG_CURRENT_DESKTOP", "KDE")],
                "",
            );
            assert_eq!(s.desktop, Desktop::Kde);
            assert_eq!(
                resolve_layout_backend(LayoutBackend::Auto, &s, true),
                LayoutBackend::Kde
            );
        }
    }

    #[test]
    fn xfce_x11() {
        let s = session(
            &[
                ("XDG_SESSION_TYPE", "x11"),
                ("XDG_CURRENT_DESKTOP", "XFCE"),
                ("DISPLAY", ":0"),
            ],
            "",
        );
        assert_eq!(s.desktop, Desktop::Other("XFCE".to_string()));
        assert_eq!(
            resolve_layout_backend(LayoutBackend::Auto, &s, false),
            LayoutBackend::X11
        );
    }

    #[test]
    fn sway_wayland_uses_internal() {
        let s = session(
            &[
                ("XDG_CURRENT_DESKTOP", "sway"),
                ("WAYLAND_DISPLAY", "wayland-1"),
                ("DISPLAY", ":0"),
            ],
            "",
        );
        assert_eq!(s.session_type, SessionType::Wayland);
        assert_eq!(
            resolve_layout_backend(LayoutBackend::Auto, &s, false),
            LayoutBackend::Internal
        );
    }

    #[test]
    fn explicit_setting_wins() {
        let s = session(&[("XDG_CURRENT_DESKTOP", "KDE")], "");
        assert_eq!(
            resolve_layout_backend(LayoutBackend::X11, &s, false),
            LayoutBackend::X11
        );
    }

    #[test]
    fn wsl_detection() {
        let s = session(
            &[("WAYLAND_DISPLAY", "wayland-0")],
            "6.18.33.2-microsoft-standard-WSL2",
        );
        assert!(s.wsl);
        assert_eq!(s.desktop, Desktop::Unknown);
        let s = session(&[], "");
        assert_eq!(s.session_type, SessionType::Unknown);
    }
}
