//! Log files with daily rotation.
//!
//! The typed text itself is never logged; only events and decisions are.

use anyhow::{Context, Result};
use okbs_core::config::LogLevel;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

/// Set once by [`init`], so «Диагностика» can change the verbosity of the
/// running program instead of asking for a restart.
static LEVEL: OnceLock<reload::Handle<LevelFilter, tracing_subscriber::Registry>> = OnceLock::new();
static OUTPUT: OnceLock<Arc<FileOutput>> = OnceLock::new();

/// `--debug` wins over the configuration for the whole run.
static FORCED: AtomicBool = AtomicBool::new(false);

/// Flushes pending log records when dropped.
#[derive(Debug)]
pub struct LogGuard {
    output: Arc<FileOutput>,
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        self.output.disable();
    }
}

struct FileOutput {
    dir: PathBuf,
    keep_files: usize,
    writer: Arc<RwLock<Option<NonBlocking>>>,
    worker: Mutex<Option<WorkerGuard>>,
}

impl std::fmt::Debug for FileOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileOutput")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl FileOutput {
    fn enable(&self) -> Result<()> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| anyhow::anyhow!("log worker lock poisoned"))?;
        if worker.is_some() {
            return Ok(());
        }
        let appender = Builder::new()
            .rotation(Rotation::DAILY)
            .filename_prefix(okbs_core::APP_ID)
            .filename_suffix("log")
            .max_log_files(self.keep_files)
            .build(&self.dir)
            .with_context(|| format!("cannot open log file in {}", self.dir.display()))?;
        let (new_writer, new_worker) = tracing_appender::non_blocking(appender);
        *self
            .writer
            .write()
            .map_err(|_| anyhow::anyhow!("log writer lock poisoned"))? = Some(new_writer);
        *worker = Some(new_worker);
        Ok(())
    }

    fn disable(&self) {
        if let Ok(mut writer) = self.writer.write() {
            *writer = None;
        }
        if let Ok(mut worker) = self.worker.lock() {
            worker.take();
        }
    }
}

#[derive(Clone)]
struct DynamicMakeWriter(Arc<RwLock<Option<NonBlocking>>>);

enum DynamicWriter {
    File(NonBlocking),
    Sink(io::Sink),
}

impl Write for DynamicWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::File(writer) => writer.write(buf),
            Self::Sink(writer) => writer.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::File(writer) => writer.flush(),
            Self::Sink(writer) => writer.flush(),
        }
    }
}

impl<'a> MakeWriter<'a> for DynamicMakeWriter {
    type Writer = DynamicWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.0
            .read()
            .ok()
            .and_then(|writer| writer.clone())
            .map_or_else(|| DynamicWriter::Sink(io::sink()), DynamicWriter::File)
    }
}

fn filter(level: Option<LogLevel>) -> LevelFilter {
    match level {
        None => LevelFilter::OFF,
        Some(LogLevel::Error) => LevelFilter::ERROR,
        Some(LogLevel::Warn) => LevelFilter::WARN,
        Some(LogLevel::Info) => LevelFilter::INFO,
        Some(LogLevel::Debug) => LevelFilter::DEBUG,
        Some(LogLevel::Trace) => LevelFilter::TRACE,
    }
}

/// Applies a new verbosity to the running program. Ignored while `--debug`
/// asked for the detailed log on the command line.
pub fn set_level(enabled: bool, level: LogLevel) {
    if FORCED.load(Ordering::SeqCst) {
        return;
    }
    let Some(handle) = LEVEL.get() else { return };
    let Some(output) = OUTPUT.get() else { return };
    if enabled {
        if let Err(err) = output.enable() {
            eprintln!("okbswitch: cannot enable logging: {err:#}");
            return;
        }
        if let Err(err) = handle.modify(|current| *current = filter(Some(level))) {
            eprintln!("okbswitch: cannot change the log level: {err}");
        }
    } else {
        if let Err(err) = handle.modify(|current| *current = LevelFilter::OFF) {
            eprintln!("okbswitch: cannot disable logging: {err}");
            return;
        }
        output.disable();
    }
}

/// Installs the global subscriber: a daily log file in `dir`, plus the
/// terminal when `console` is set.
pub fn init(
    dir: &Path,
    enabled: bool,
    level: LogLevel,
    keep_files: u32,
    console: bool,
) -> Result<LogGuard> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("cannot create log directory {}", dir.display()))?;
    let writer = Arc::new(RwLock::new(None));
    let output = Arc::new(FileOutput {
        dir: dir.to_path_buf(),
        keep_files: keep_files.max(1) as usize,
        writer: writer.clone(),
        worker: Mutex::new(None),
    });
    if enabled || console {
        output.enable()?;
    }

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(DynamicMakeWriter(writer))
        .with_ansi(false)
        .with_thread_names(true)
        .with_target(true);
    let console_layer = console.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(io::stderr)
            .with_target(false)
    });

    // One reloadable filter in front of both layers: «Диагностика» then
    // changes the whole subscriber with a single call.
    let (level_layer, handle) = reload::Layer::new(filter((enabled || console).then_some(level)));
    tracing_subscriber::registry()
        .with(level_layer)
        .with(file_layer)
        .with(console_layer)
        .try_init()
        .context("logging is already initialized")?;
    FORCED.store(console, Ordering::SeqCst);
    let _ = LEVEL.set(handle);
    let _ = OUTPUT.set(output.clone());
    Ok(LogGuard { output })
}
