//! Own Keyboard Switch (okbswitch): automatic RU/EN keyboard layout switcher.
// Release builds on Windows have no console window; `--help` attaches to the parent console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod app_linux;
#[cfg(windows)]
mod app_windows;
mod cli;
#[cfg(any(windows, target_os = "linux"))]
mod clipboard_history;
#[cfg(any(windows, target_os = "linux"))]
mod controller;
mod diagnose;
mod instance;
mod locale;
mod logging;
mod paths;
mod settings;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Cli;
use crossbeam_channel::bounded;
use okbs_core::config::{self, ConfigIssue, LogLevel};
use okbs_platform::DiagnosticStatus;
use paths::AppPaths;
use std::process::ExitCode;

/// How long `--restarting` waits for the previous instance to release the lock.
const RESTART_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

fn main() -> ExitCode {
    #[cfg(windows)]
    okbs_platform_windows::console::attach_parent_console();
    let cli = Cli::parse();
    #[cfg(windows)]
    let interactive = !(cli.paths || cli.diagnose || cli.licenses || cli.print_default_config);
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            tracing::error!("{err:#}");
            eprintln!("okbswitch: {err:#}");
            #[cfg(windows)]
            if interactive {
                okbs_platform_windows::console::show_startup_error(&format!("{err:#}"));
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    if cli.licenses {
        print!(
            "{}\n{}\n",
            include_str!("../../../NOTICE"),
            include_str!("../../../LICENSE")
        );
        print!("{}", include_str!("../../../data/LICENSES.md"));
        print!(
            "\n{}",
            include_str!("../../../data/hunspell/README_en_US.txt")
        );
        print!("\n{}", include_str!("../../../THIRD-PARTY-NOTICES.txt"));
        return Ok(ExitCode::SUCCESS);
    }
    if cli.print_default_config {
        print!("{}", config::to_toml_string(&config::Config::default())?);
        return Ok(ExitCode::SUCCESS);
    }

    let paths = AppPaths::resolve(cli.config)?;
    if cli.paths {
        print!("{paths}");
        return Ok(ExitCode::SUCCESS);
    }
    paths.prepare()?;
    // Own the destination before migrating/loading settings. Diagnostic runs
    // can inspect an active installation but never migrate profile data.
    let _lock = if cli.diagnose {
        None
    } else {
        let acquired = if cli.restarting {
            instance::acquire_waiting(&paths.lock_file, RESTART_WAIT)
        } else {
            instance::acquire(&paths.lock_file)
        }
        .with_context(|| format!("cannot open lock file {}", paths.lock_file.display()))?;
        match acquired {
            instance::Acquire::Acquired(lock) => Some(lock),
            instance::Acquire::AlreadyRunning => {
                eprintln!("okbswitch is already running");
                return Ok(ExitCode::SUCCESS);
            }
        }
    };
    // An installation that predates the self-contained layout keeps its
    // settings; the report goes to the log once it is open.
    let adopted = if cli.diagnose {
        Vec::new()
    } else {
        paths.adopt_user_profile_data()?
    };

    let loaded = config::load_or_create(&paths.config_file)
        .with_context(|| format!("cannot load configuration {}", paths.config_file.display()))?;

    if cli.diagnose {
        let report = diagnose::report(&paths, &loaded);
        print!("{report}");
        return Ok(if report.worst() == DiagnosticStatus::Error {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        });
    }

    // «Диагностика» in the settings window, or `--debug` for a single run.
    let level = if cli.debug {
        LogLevel::Debug.max(loaded.config.log.level)
    } else {
        loaded.config.log.effective_level()
    };
    let _log = logging::init(
        &paths.log_dir,
        level,
        loaded.config.log.keep_files,
        cli.debug,
    )?;
    tracing::info!(
        version = okbs_core::VERSION,
        os = std::env::consts::OS,
        log_dir = %paths.log_dir.display(),
        "starting {}",
        okbs_core::APP_NAME
    );
    for line in adopted {
        tracing::info!("{line}");
    }
    log_config_issues(&loaded.issues);

    if let Some(lock) = &_lock {
        tracing::debug!(path = %lock.path().display(), "instance lock acquired");
    }

    let read_only = loaded
        .issues
        .iter()
        .any(|i| matches!(i, ConfigIssue::NewerVersion { .. }));
    let settings = settings::Settings {
        config: loaded.config,
        path: paths.config_file.clone(),
        read_only,
    };
    let (stop_tx, stop_rx) = bounded::<()>(1);
    ctrlc::set_handler(move || {
        let _ = stop_tx.try_send(());
    })
    .context("cannot install the termination handler")?;

    #[cfg(windows)]
    app_windows::run(settings, &paths, cli.no_tray, cli.settings, &stop_rx)?;
    #[cfg(target_os = "linux")]
    app_linux::run(settings, cli.no_tray, cli.settings, &stop_rx)?;
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (settings, stop_rx);
        anyhow::bail!("this operating system is not supported");
    }
    tracing::info!("stopped");
    Ok(ExitCode::SUCCESS)
}

fn log_config_issues(issues: &[ConfigIssue]) {
    for issue in issues {
        if issue.is_error() {
            tracing::error!("{issue}");
        } else if issue.is_warning() {
            tracing::warn!("{issue}");
        } else {
            tracing::info!("{issue}");
        }
    }
}
