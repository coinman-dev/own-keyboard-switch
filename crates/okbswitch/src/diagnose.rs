//! `--diagnose`: environment report.

use crate::instance::{self, Acquire};
use crate::paths::AppPaths;
use okbs_core::config::{ConfigIssue, LoadOutcome};
use okbs_platform::{DiagnosticReport, DiagnosticStatus};

/// Builds the full report: program, configuration and platform checks.
pub fn report(paths: &AppPaths, loaded: &LoadOutcome) -> DiagnosticReport {
    let mut r = DiagnosticReport::default();
    r.info(
        "version",
        format!("{} {}", okbs_core::APP_NAME, okbs_core::VERSION),
    );
    r.info("config file", paths.config_file.display().to_string());
    let warnings: Vec<&ConfigIssue> = loaded.issues.iter().filter(|i| i.is_warning()).collect();
    if warnings.is_empty() {
        r.push(DiagnosticStatus::Ok, "config", "valid");
    } else {
        for issue in warnings {
            let status = if issue.is_error() {
                DiagnosticStatus::Error
            } else {
                DiagnosticStatus::Warning
            };
            r.push(status, "config", issue.to_string());
        }
    }
    r.info("log directory", paths.log_dir.display().to_string());

    let started = std::time::Instant::now();
    let loaded_data = std::panic::catch_unwind(okbs_core::data::warm_up);
    match loaded_data {
        Ok(()) => r.push(
            DiagnosticStatus::Ok,
            "language data",
            format!(
                "RU/EN models and dictionaries loaded in {} ms",
                started.elapsed().as_millis()
            ),
        ),
        Err(_) => r.push(
            DiagnosticStatus::Error,
            "language data",
            "embedded language data is corrupted",
        ),
    }
    match instance::acquire(&paths.lock_file) {
        Ok(Acquire::Acquired(_)) => r.info("instance", "not running"),
        Ok(Acquire::AlreadyRunning) => r.info("instance", "running"),
        Err(err) => r.push(
            DiagnosticStatus::Warning,
            "instance",
            format!("cannot check lock file: {err}"),
        ),
    }

    #[cfg(target_os = "linux")]
    r.extend(okbs_platform_linux::diagnose(&loaded.config));
    #[cfg(windows)]
    r.extend(okbs_platform_windows::diagnose(&loaded.config));
    #[cfg(not(any(target_os = "linux", windows)))]
    r.push(
        DiagnosticStatus::Error,
        "platform",
        "this operating system is not supported",
    );
    r
}
