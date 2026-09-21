//! Log files with daily rotation.
//!
//! The typed text itself is never logged; only events and decisions are.

use anyhow::{Context, Result};
use okbs_core::config::LogLevel;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

/// Set once by [`init`], so «Диагностика» can change the verbosity of the
/// running program instead of asking for a restart.
static LEVEL: OnceLock<reload::Handle<LevelFilter, tracing_subscriber::Registry>> = OnceLock::new();

/// `--debug` wins over the configuration for the whole run.
static FORCED: AtomicBool = AtomicBool::new(false);

/// Flushes pending log records when dropped.
#[derive(Debug)]
pub struct LogGuard {
    _worker: WorkerGuard,
}

fn filter(level: LogLevel) -> LevelFilter {
    match level {
        LogLevel::Error => LevelFilter::ERROR,
        LogLevel::Warn => LevelFilter::WARN,
        LogLevel::Info => LevelFilter::INFO,
        LogLevel::Debug => LevelFilter::DEBUG,
        LogLevel::Trace => LevelFilter::TRACE,
    }
}

/// Applies a new verbosity to the running program. Ignored while `--debug`
/// asked for the detailed log on the command line.
pub fn set_level(level: LogLevel) {
    if FORCED.load(Ordering::SeqCst) {
        return;
    }
    if let Some(handle) = LEVEL.get()
        && let Err(err) = handle.modify(|current| *current = filter(level))
    {
        tracing::warn!("cannot change the log level: {err}");
    }
}

/// Installs the global subscriber: a daily log file in `dir`, plus the
/// terminal when `console` is set.
pub fn init(dir: &Path, level: LogLevel, keep_files: u32, console: bool) -> Result<LogGuard> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("cannot create log directory {}", dir.display()))?;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(okbs_core::APP_ID)
        .filename_suffix("log")
        .max_log_files(keep_files.max(1) as usize)
        .build(dir)
        .with_context(|| format!("cannot open log file in {}", dir.display()))?;
    let (writer, worker) = tracing_appender::non_blocking(appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_thread_names(true)
        .with_target(true);
    let console_layer = console.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(false)
    });

    // One reloadable filter in front of both layers: «Диагностика» then
    // changes the whole subscriber with a single call.
    let (level_layer, handle) = reload::Layer::new(filter(level));
    tracing_subscriber::registry()
        .with(level_layer)
        .with(file_layer)
        .with(console_layer)
        .try_init()
        .context("logging is already initialized")?;
    FORCED.store(console, Ordering::SeqCst);
    let _ = LEVEL.set(handle);
    Ok(LogGuard { _worker: worker })
}
