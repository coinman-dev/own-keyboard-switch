//! `okbswitch --diagnose` checks for Windows.

use crate::layouts;
use okbs_core::config::Config;
use okbs_platform::{DiagnosticReport, DiagnosticStatus};

/// Collects environment facts relevant to the program.
pub fn diagnose(config: &Config) -> DiagnosticReport {
    let mut r = DiagnosticReport::default();
    r.info("os", format!("Windows ({})", std::env::consts::ARCH));

    let elevated = crate::elevation::is_elevated();
    let elevation_status = if config.general.run_elevated && !elevated {
        DiagnosticStatus::Warning
    } else {
        DiagnosticStatus::Info
    };
    r.push(
        elevation_status,
        "elevated",
        if elevated {
            "yes: works in administrator windows"
        } else {
            "no: administrator windows are not affected (see «Запускать с правами Администратора»)"
        },
    );
    // The `Run` key cannot raise the rights, so an elevated start at logon uses
    // a scheduled task instead. Report which registration is in place.
    match (
        okbs_platform::Autostart::is_enabled(&crate::WinAutostart),
        okbs_platform::Autostart::elevated_at_login(&crate::WinAutostart),
    ) {
        (Ok(false), _) => r.info("autostart", "off"),
        (Ok(true), Ok(true)) => r.info("autostart", "logon task, with administrator rights"),
        (Ok(true), _) => {
            let status = if config.general.run_elevated {
                DiagnosticStatus::Warning
            } else {
                DiagnosticStatus::Info
            };
            r.push(
                status,
                "autostart",
                "Run registry key, without administrator rights",
            );
        }
        (Err(err), _) => r.push(DiagnosticStatus::Warning, "autostart", err.to_string()),
    }

    let installed = layouts::installed_layouts();
    let list: Vec<String> = installed
        .iter()
        .map(|l| format!("{} [{:08X}]", l.name, l.id.0))
        .collect();
    r.info("layouts", list.join(", "));
    for lang in config.general.language_pair {
        let found = installed.iter().any(|l| l.lang == Some(lang));
        r.push(
            if found {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Error
            },
            format!("layout {lang}"),
            if found {
                "installed"
            } else {
                "not installed: add it in Settings → Time & language"
            },
        );
    }
    if let Some(current) = layouts::foreground_layout() {
        r.info("foreground layout", layouts::describe(current).name);
    }
    for info in installed.iter().filter(|l| l.lang.is_some()) {
        let map = layouts::keymap_for(info.id);
        let sample: String = [
            okbs_core::PhysKey::KeyQ,
            okbs_core::PhysKey::KeyW,
            okbs_core::PhysKey::KeyE,
            okbs_core::PhysKey::Comma,
        ]
        .iter()
        .filter_map(|&k| map.get(k, false))
        .collect();
        r.info(format!("keys {}", info.name), format!("Q W E , → {sample}"));
    }
    r
}
