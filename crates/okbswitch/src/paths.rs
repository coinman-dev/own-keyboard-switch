//! Writable program files stay in the installation directory: data/ and Logs/.
//! Profile directories are read only as migration sources, never as a fallback.

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use okbs_core::APP_ID;
use std::{
    fmt, fs,
    path::{Component, Path, PathBuf},
};

const CONFIG_FILE: &str = "config.toml";
pub const HISTORY_FILE: &str = "clipboard-history.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub state_dir: PathBuf,
    pub log_dir: PathBuf,
    pub lock_file: PathBuf,
    custom_config: bool,
}

impl AppPaths {
    /// Pure path resolution, so --paths does not create or migrate any files.
    pub fn resolve(config_override: Option<PathBuf>) -> Result<Self> {
        let exe = std::env::current_exe().context("cannot locate the program")?;
        let directory = exe.parent().context("the program has no directory")?;
        Self::in_directory(directory, config_override)
    }

    fn in_directory(directory: &Path, config_override: Option<PathBuf>) -> Result<Self> {
        let state_dir = directory.join("data");
        let custom_config = config_override.is_some();
        let config_file = match config_override {
            None => state_dir.join(CONFIG_FILE),
            Some(path) => {
                let full = if path.is_absolute() {
                    path
                } else {
                    directory.join(path)
                };
                if !full.starts_with(directory)
                    || full.components().any(|c| c == Component::ParentDir)
                {
                    bail!(
                        "--config must name a file inside the program directory: {}",
                        directory.display()
                    );
                }
                full
            }
        };
        Ok(Self {
            config_file,
            log_dir: directory.join("Logs"),
            lock_file: state_dir.join(format!("{APP_ID}.lock")),
            state_dir,
            custom_config,
        })
    }

    pub fn prepare(&self) -> Result<()> {
        for directory in [&self.state_dir, &self.log_dir] {
            fs::create_dir_all(directory).with_context(|| format!("cannot create {}; install to a writable directory or repair the installation permissions", directory.display()))?;
            // A unique temporary file cannot overwrite an existing user's file.
            tempfile::NamedTempFile::new_in(directory).with_context(|| {
                format!(
                    "cannot write to {}; choose a writable installation directory",
                    directory.display()
                )
            })?;
        }
        if let Some(parent) = self.config_file.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Called only after the new installation's lock is held. A custom config
    /// or diagnostic request must never import/remove a user's profile files.
    pub fn adopt_user_profile_data(&self) -> Result<Vec<String>> {
        if self.custom_config {
            return Ok(Vec::new());
        }
        let base = BaseDirs::new().context("cannot locate old profile data")?;
        let config = base.config_dir().join(APP_ID);
        let state = base
            .state_dir()
            .unwrap_or_else(|| base.data_local_dir())
            .join(APP_ID);
        let mut locks = vec![state.join(format!("{APP_ID}.lock"))];
        if let Some(runtime) = base.runtime_dir() {
            let lock = runtime.join(format!("{APP_ID}.lock"));
            if !locks.contains(&lock) {
                locks.push(lock);
            }
        }
        self.adopt_from(&config, &state, &locks)
    }

    fn adopt_from(
        &self,
        config: &Path,
        state: &Path,
        legacy_locks: &[PathBuf],
    ) -> Result<Vec<String>> {
        let mut migrated = Vec::new();
        // Do not migrate data while an older version still holds its lock.
        let mut guards = Vec::new();
        for lock in legacy_locks.iter().filter(|lock| lock.is_file()) {
            let file = fs::OpenOptions::new().read(true).write(true).open(lock)?;
            file.try_lock()
                .context("an older version is running; close it before migrating settings")?;
            guards.push(file);
        }
        for (source, target) in [
            (config.join(CONFIG_FILE), self.config_file.clone()),
            (state.join(HISTORY_FILE), self.state_dir.join(HISTORY_FILE)),
        ] {
            if source.is_file() && !target.exists() {
                transfer(&source, &target)?;
                migrated.push(format!(
                    "migrated {} to {}",
                    source.display(),
                    target.display()
                ));
            }
        }
        // Preserve conflicting configs, logs and backups instead of replacing
        // current settings or recursively deleting old profile directories.
        for (source, label) in [(config, "config"), (state, "state")] {
            // A portable copy may itself live inside an old profile directory.
            // Never traverse/archive the current installation into itself.
            if source.is_dir() && !self.state_dir.starts_with(source) {
                archive_directory(
                    source,
                    &self.state_dir.join("legacy-profile").join(label),
                    legacy_locks,
                    &mut migrated,
                )?;
            }
        }
        drop(guards);
        for lock in legacy_locks.iter().filter(|lock| lock.is_file()) {
            fs::remove_file(lock)?;
        }
        // remove_dir only succeeds for empty directories. Unknown links stay intact.
        for source in [config, state] {
            let _ = fs::remove_dir(source);
        }
        Ok(migrated)
    }
}

fn transfer(source: &Path, target: &Path) -> Result<()> {
    let parent = target.parent().context("destination has no parent")?;
    fs::create_dir_all(parent)?;
    let mut input = fs::File::open(source)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let copied = std::io::copy(&mut input, &mut temporary)?;
    if copied != input.metadata()?.len() {
        bail!("source changed during migration: {}", source.display());
    }
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(target)
        .with_context(|| format!("cannot preserve {}", target.display()))?;
    // The original is removed only after a complete durable copy is in place.
    drop(input);
    fs::remove_file(source)?;
    Ok(())
}

fn archive_directory(
    source: &Path,
    target: &Path,
    locks: &[PathBuf],
    messages: &mut Vec<String>,
) -> Result<()> {
    for item in fs::read_dir(source)? {
        let item = item?;
        let from = item.path();
        if locks.contains(&from) {
            continue;
        }
        let kind = item.file_type()?;
        let destination = target.join(item.file_name());
        if kind.is_dir() {
            archive_directory(&from, &destination, locks, messages)?;
            let _ = fs::remove_dir(&from);
        } else if kind.is_file() {
            let mut unique = destination.clone();
            let mut number = 1;
            while unique.exists() {
                unique = target.join(format!("{}.{}", item.file_name().to_string_lossy(), number));
                number += 1;
            }
            transfer(&from, &unique)?;
            messages.push(format!(
                "preserved {} in {}",
                from.display(),
                unique.display()
            ));
        } else {
            messages.push(format!(
                "left unsupported profile entry intact: {}",
                from.display()
            ));
        }
    }
    Ok(())
}

impl fmt::Display for AppPaths {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "config: {}", self.config_file.display())?;
        writeln!(f, "state:  {}", self.state_dir.display())?;
        writeln!(f, "logs:   {}", self.log_dir.display())?;
        writeln!(f, "lock:   {}", self.lock_file.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolving_paths_is_read_only_and_stays_inside_installation() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::in_directory(directory.path(), None).unwrap();
        assert_eq!(paths.config_file, directory.path().join("data/config.toml"));
        assert_eq!(paths.log_dir, directory.path().join("Logs"));
        assert!(!paths.state_dir.exists());
        assert!(
            AppPaths::in_directory(directory.path(), Some("../elsewhere.toml".into())).is_err()
        );
        let external = tempfile::tempdir().unwrap();
        assert!(
            AppPaths::in_directory(directory.path(), Some(external.path().join("config.toml")))
                .is_err()
        );
        paths.prepare().unwrap();
        assert!(paths.state_dir.is_dir() && paths.log_dir.is_dir());
        assert_eq!(fs::read_dir(&paths.state_dir).unwrap().count(), 0);
    }

    #[test]
    fn unwritable_installation_does_not_fall_back_to_a_profile() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("data"), "not a directory").unwrap();
        let paths = AppPaths::in_directory(directory.path(), None).unwrap();
        assert!(paths.prepare().is_err());
        assert_eq!(paths.config_file, directory.path().join("data/config.toml"));
    }

    #[test]
    fn migration_preserves_conflicts_backups_and_history_independently() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("old-config");
        let state = directory.path().join("old-state");
        fs::create_dir_all(&profile).unwrap();
        fs::create_dir_all(state.join("logs")).unwrap();
        fs::write(profile.join(CONFIG_FILE), "old config").unwrap();
        fs::write(profile.join("config.toml.invalid"), "backup").unwrap();
        fs::write(state.join(HISTORY_FILE), "history").unwrap();
        fs::write(state.join("logs/old.log"), "old log").unwrap();
        let paths = AppPaths::in_directory(&directory.path().join("program"), None).unwrap();
        paths.prepare().unwrap();
        fs::write(&paths.config_file, "current config").unwrap();
        paths.adopt_from(&profile, &state, &[]).unwrap();
        assert_eq!(
            fs::read_to_string(&paths.config_file).unwrap(),
            "current config"
        );
        assert_eq!(
            fs::read_to_string(paths.state_dir.join(HISTORY_FILE)).unwrap(),
            "history"
        );
        assert_eq!(
            fs::read_to_string(paths.state_dir.join("legacy-profile/config/config.toml")).unwrap(),
            "old config"
        );
        assert_eq!(
            fs::read_to_string(
                paths
                    .state_dir
                    .join("legacy-profile/config/config.toml.invalid")
            )
            .unwrap(),
            "backup"
        );
        assert_eq!(
            fs::read_to_string(paths.state_dir.join("legacy-profile/state/logs/old.log")).unwrap(),
            "old log"
        );
        assert!(!profile.exists() && !state.exists());
        assert!(paths.adopt_from(&profile, &state, &[]).unwrap().is_empty());
    }

    #[test]
    fn an_old_running_instance_prevents_migration() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("old");
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join(CONFIG_FILE), "settings").unwrap();
        let lock = fs::File::create(profile.join("okbswitch.lock")).unwrap();
        lock.try_lock().unwrap();
        let paths = AppPaths::in_directory(&directory.path().join("program"), None).unwrap();
        paths.prepare().unwrap();
        let legacy_lock = profile.join("okbswitch.lock");
        assert!(
            paths
                .adopt_from(&profile, &profile, std::slice::from_ref(&legacy_lock))
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(profile.join(CONFIG_FILE)).unwrap(),
            "settings"
        );
        assert!(!paths.config_file.exists());
        drop(lock);
        paths
            .adopt_from(&profile, &profile, &[legacy_lock])
            .unwrap();
        assert_eq!(fs::read_to_string(&paths.config_file).unwrap(), "settings");
    }

    #[test]
    fn interrupted_copy_never_overwrites_an_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, "source data").unwrap();
        fs::write(&target, "existing data").unwrap();
        assert!(transfer(&source, &target).is_err());
        assert_eq!(fs::read_to_string(&source).unwrap(), "source data");
        assert_eq!(fs::read_to_string(&target).unwrap(), "existing data");
    }

    #[test]
    fn a_portable_copy_inside_the_old_profile_is_not_archived_into_itself() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(CONFIG_FILE), "old settings").unwrap();
        fs::write(directory.path().join("okbswitch.exe"), "program").unwrap();
        let paths = AppPaths::in_directory(directory.path(), None).unwrap();
        paths.prepare().unwrap();
        paths
            .adopt_from(directory.path(), directory.path(), &[])
            .unwrap();
        assert_eq!(
            fs::read_to_string(&paths.config_file).unwrap(),
            "old settings"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("okbswitch.exe")).unwrap(),
            "program"
        );
        assert!(!paths.state_dir.join("legacy-profile").exists());
    }

    #[test]
    fn a_runtime_directory_lock_also_prevents_migration() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config");
        let state = directory.path().join("state");
        let runtime = directory.path().join("runtime");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        fs::write(config.join(CONFIG_FILE), "settings").unwrap();
        let lock_path = runtime.join("okbswitch.lock");
        let lock = fs::File::create(&lock_path).unwrap();
        lock.try_lock().unwrap();
        let paths = AppPaths::in_directory(&directory.path().join("program"), None).unwrap();
        paths.prepare().unwrap();
        assert!(
            paths
                .adopt_from(
                    &config,
                    &state,
                    &[state.join("okbswitch.lock"), lock_path.clone()]
                )
                .is_err()
        );
        assert!(config.join(CONFIG_FILE).exists());
        drop(lock);
        paths
            .adopt_from(&config, &state, &[state.join("okbswitch.lock"), lock_path])
            .unwrap();
        assert_eq!(fs::read_to_string(&paths.config_file).unwrap(), "settings");
    }
}
