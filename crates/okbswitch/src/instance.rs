//! Single-instance guard based on an exclusive file lock.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Held while this process is the running instance. Released on drop.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    path: PathBuf,
}

impl InstanceLock {
    /// Lock file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Outcome of [`acquire`].
#[derive(Debug)]
pub enum Acquire {
    /// This process is the only instance.
    Acquired(InstanceLock),
    /// Another instance holds the lock.
    AlreadyRunning,
}

/// Tries to become the single running instance.
pub fn acquire(path: &Path) -> io::Result<Acquire> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => {
            file.set_len(0)?;
            writeln!(file, "{}", std::process::id())?;
            file.flush()?;
            Ok(Acquire::Acquired(InstanceLock {
                _file: file,
                path: path.to_path_buf(),
            }))
        }
        Err(TryLockError::WouldBlock) => Ok(Acquire::AlreadyRunning),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

/// Like [`acquire`], but retries until `timeout` passes. Used when the program
/// restarts itself and the previous instance is still shutting down.
pub fn acquire_waiting(path: &Path, timeout: std::time::Duration) -> io::Result<Acquire> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match acquire(path)? {
            Acquire::Acquired(lock) => return Ok(Acquire::Acquired(lock)),
            Acquire::AlreadyRunning if std::time::Instant::now() >= deadline => {
                return Ok(Acquire::AlreadyRunning);
            }
            Acquire::AlreadyRunning => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_is_blocked_until_release() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("okbswitch.lock");
        let first = match acquire(&path).unwrap() {
            Acquire::Acquired(lock) => lock,
            Acquire::AlreadyRunning => panic!("first acquire must succeed"),
        };
        assert_eq!(first.path(), path);
        assert!(matches!(acquire(&path).unwrap(), Acquire::AlreadyRunning));
        drop(first);
        assert!(matches!(acquire(&path).unwrap(), Acquire::Acquired(_)));
    }

    #[test]
    fn waiting_gives_up_after_the_timeout_and_succeeds_once_released() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("okbswitch.lock");
        let held = match acquire(&path).unwrap() {
            Acquire::Acquired(lock) => lock,
            Acquire::AlreadyRunning => panic!("first acquire must succeed"),
        };
        let started = std::time::Instant::now();
        let result = acquire_waiting(&path, std::time::Duration::from_millis(250)).unwrap();
        assert!(matches!(result, Acquire::AlreadyRunning));
        assert!(started.elapsed() >= std::time::Duration::from_millis(250));
        drop(held);
        assert!(matches!(
            acquire_waiting(&path, std::time::Duration::from_secs(1)).unwrap(),
            Acquire::Acquired(_)
        ));
    }
}
