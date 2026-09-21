//! Reading and writing `config.toml`.
//!
//! Loading never fails because of the file contents. A syntax error resets the
//! configuration to defaults; an invalid value resets only its own key; a
//! malformed element of a list is dropped. In both cases the original file is
//! kept as `config.toml.invalid-<unix time>` and a corrected file is written.

use super::{CURRENT_VERSION, Config};
use serde::Deserialize;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// I/O failure while reading or writing the configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file exists but cannot be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// File path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file or its backup cannot be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// File path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The configuration cannot be serialized (a bug).
    #[error("cannot serialize configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Something noteworthy that happened while loading the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigIssue {
    /// The file did not exist and was created with defaults.
    CreatedDefault {
        /// Created file.
        path: PathBuf,
    },
    /// The file is not valid TOML or not UTF-8; defaults are used.
    SyntaxError {
        /// Parser message.
        message: String,
    },
    /// A value has the wrong type or format and was replaced by its default.
    InvalidValue {
        /// Dotted key path, e.g. `hotkeys.paste_plain` or `rules[2]`.
        key: String,
        /// Reason.
        message: String,
    },
    /// A key is not known to this version and is ignored.
    UnknownKey {
        /// Dotted key path.
        key: String,
    },
    /// A value was out of range or inconsistent and was adjusted.
    Adjusted {
        /// Dotted key path.
        key: String,
        /// What was changed.
        message: String,
    },
    /// The file was written by a newer version; it is read but never rewritten.
    NewerVersion {
        /// Version in the file.
        found: u32,
        /// Version supported by this build.
        supported: u32,
    },
    /// Several actions share one hotkey.
    DuplicateHotkey {
        /// The shared hotkey.
        hotkey: String,
        /// Config keys of the actions.
        actions: Vec<&'static str>,
    },
    /// The invalid original file was copied here.
    BackupCreated {
        /// Backup path.
        path: PathBuf,
    },
    /// The file was upgraded from an older schema version.
    Migrated {
        /// Version in the file.
        from: u32,
        /// Current version.
        to: u32,
    },
    /// A corrected configuration was written.
    Rewritten {
        /// Config path.
        path: PathBuf,
    },
}

impl ConfigIssue {
    /// Issues that made the loader discard part of the user's file.
    pub fn is_error(&self) -> bool {
        matches!(
            self,
            ConfigIssue::SyntaxError { .. } | ConfigIssue::InvalidValue { .. }
        )
    }

    /// Issues worth a warning (errors included).
    pub fn is_warning(&self) -> bool {
        !matches!(
            self,
            ConfigIssue::CreatedDefault { .. }
                | ConfigIssue::Rewritten { .. }
                | ConfigIssue::Migrated { .. }
        )
    }
}

impl fmt::Display for ConfigIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigIssue::CreatedDefault { path } => {
                write!(f, "created default configuration at {}", path.display())
            }
            ConfigIssue::SyntaxError { message } => {
                write!(
                    f,
                    "configuration file is invalid, defaults are used: {message}"
                )
            }
            ConfigIssue::InvalidValue { key, message } => {
                write!(f, "invalid value of `{key}`, default is used: {message}")
            }
            ConfigIssue::UnknownKey { key } => write!(f, "unknown key `{key}` is ignored"),
            ConfigIssue::Adjusted { key, message } => write!(f, "`{key}` adjusted: {message}"),
            ConfigIssue::NewerVersion { found, supported } => write!(
                f,
                "configuration version {found} is newer than supported version {supported}; \
                 the file will not be overwritten automatically"
            ),
            ConfigIssue::DuplicateHotkey { hotkey, actions } => {
                write!(
                    f,
                    "hotkey `{hotkey}` is assigned to several actions: {}",
                    actions.join(", ")
                )
            }
            ConfigIssue::Migrated { from, to } => {
                write!(f, "configuration upgraded from version {from} to {to}")
            }
            ConfigIssue::BackupCreated { path } => {
                write!(f, "original configuration saved to {}", path.display())
            }
            ConfigIssue::Rewritten { path } => {
                write!(f, "corrected configuration written to {}", path.display())
            }
        }
    }
}

/// Result of loading: the effective configuration plus everything noteworthy.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadOutcome {
    /// Effective configuration.
    pub config: Config,
    /// Issues in the order they were found.
    pub issues: Vec<ConfigIssue>,
}

const HEADER: &str = "\
# Own Keyboard Switch (okbswitch) — настройки.
# Файл изменяется окном «Настройки». Ручные правки вступают в силу после перезапуска программы.
# Пустая строка у горячей клавиши означает, что комбинация не назначена.

";

/// Serializes the configuration with the explanatory header.
pub fn to_toml_string(config: &Config) -> Result<String, ConfigError> {
    Ok(format!("{HEADER}{}", toml::to_string_pretty(config)?))
}

/// Parses configuration text. Never fails; problems are reported as issues.
pub fn from_toml_str(text: &str) -> LoadOutcome {
    let mut issues = Vec::new();
    let mut user = match text.parse::<Table>() {
        Ok(table) => table,
        Err(err) => {
            issues.push(ConfigIssue::SyntaxError {
                message: err.to_string().trim().to_string(),
            });
            return LoadOutcome {
                config: Config::default(),
                issues,
            };
        }
    };
    migrate(&mut user, &mut issues);
    let mut config = deserialize_lenient(user, &mut issues);
    issues.extend(config.sanitize());
    LoadOutcome { config, issues }
}

/// Loads the configuration from `path`, creating it with defaults if missing.
///
/// Returns an error only for I/O failures. When the file contained invalid
/// data, it is backed up and rewritten with the effective configuration
/// (unless it was written by a newer version).
pub fn load_or_create(path: &Path) -> Result<LoadOutcome, ConfigError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let config = Config::default();
            save(path, &config)?;
            return Ok(LoadOutcome {
                config,
                issues: vec![ConfigIssue::CreatedDefault {
                    path: path.to_path_buf(),
                }],
            });
        }
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    let mut outcome = match String::from_utf8(bytes) {
        Ok(text) => from_toml_str(text.strip_prefix('\u{feff}').unwrap_or(&text)),
        Err(err) => LoadOutcome {
            config: Config::default(),
            issues: vec![ConfigIssue::SyntaxError {
                message: format!("file is not valid UTF-8: {err}"),
            }],
        },
    };

    let broken = outcome.issues.iter().any(ConfigIssue::is_error);
    let newer = outcome
        .issues
        .iter()
        .any(|i| matches!(i, ConfigIssue::NewerVersion { .. }));
    let migrated = outcome
        .issues
        .iter()
        .any(|i| matches!(i, ConfigIssue::Migrated { .. }));
    if migrated && !broken {
        save(path, &outcome.config)?;
        outcome.issues.push(ConfigIssue::Rewritten {
            path: path.to_path_buf(),
        });
    }
    if broken && !newer {
        let backup = backup_path(path);
        fs::copy(path, &backup).map_err(|source| ConfigError::Write {
            path: backup.clone(),
            source,
        })?;
        outcome
            .issues
            .push(ConfigIssue::BackupCreated { path: backup });
        save(path, &outcome.config)?;
        outcome.issues.push(ConfigIssue::Rewritten {
            path: path.to_path_buf(),
        });
    }
    Ok(outcome)
}

/// Writes the configuration atomically (temporary file + rename).
pub fn save(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let text = to_toml_string(config)?;
    let write_err = |source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(write_err)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(write_err)
}

fn backup_path(path: &Path) -> PathBuf {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let base = path.as_os_str().to_owned();
    let mut n = 0u32;
    loop {
        let mut candidate = base.clone();
        if n == 0 {
            candidate.push(format!(".invalid-{secs}"));
        } else {
            candidate.push(format!(".invalid-{secs}-{n}"));
        }
        let candidate = PathBuf::from(candidate);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Upgrades `version 1 → 2` and so on. `MIGRATIONS[i]` converts version `i + 1`.
// Version 3 adds ui_language = "system". Existing explicit "ru"/"en"
// preferences need no conversion and must be preserved.
const MIGRATIONS: &[fn(&mut Table)] = &[migrate_1_to_2];

/// Version 2 converts one-letter words: the old default `min_word_len = 2` becomes 1.
fn migrate_1_to_2(table: &mut Table) {
    let path = [
        "troubleshooting".to_string(),
        "detector".to_string(),
        "min_word_len".to_string(),
    ];
    if matches!(get_at(table, &path), Some(Value::Integer(2))) {
        set_at(table, &path, Value::Integer(1));
    }
}

fn migrate(table: &mut Table, issues: &mut Vec<ConfigIssue>) {
    let found = match table.get("version") {
        None => CURRENT_VERSION,
        Some(Value::Integer(v)) if *v >= 1 => u32::try_from(*v).unwrap_or(u32::MAX),
        Some(other) => {
            issues.push(ConfigIssue::InvalidValue {
                key: "version".to_string(),
                message: format!("expected a positive integer, found {}", other.type_str()),
            });
            CURRENT_VERSION
        }
    };
    if found > CURRENT_VERSION {
        issues.push(ConfigIssue::NewerVersion {
            found,
            supported: CURRENT_VERSION,
        });
    } else if found < CURRENT_VERSION {
        for step in found..CURRENT_VERSION {
            if let Some(migration) = MIGRATIONS.get((step - 1) as usize) {
                migration(table);
            }
        }
        issues.push(ConfigIssue::Migrated {
            from: found,
            to: CURRENT_VERSION,
        });
    }
    table.insert(
        "version".to_string(),
        Value::Integer(i64::from(CURRENT_VERSION)),
    );
}

fn deserialize_lenient(user: Table, issues: &mut Vec<ConfigIssue>) -> Config {
    let mut unknown = Vec::new();
    if let Ok(config) =
        serde_ignored::deserialize(Value::Table(user.clone()), |p| unknown.push(p.to_string()))
    {
        issues.extend(
            unknown
                .into_iter()
                .map(|key| ConfigIssue::UnknownKey { key }),
        );
        return config;
    }

    let mut root = match Value::try_from(Config::default()) {
        Ok(Value::Table(table)) => table,
        _ => Table::new(),
    };
    apply_lenient(&mut root, &[], user, issues);

    let mut unknown = Vec::new();
    match serde_ignored::deserialize::<_, _, Config>(Value::Table(root), |p| {
        unknown.push(p.to_string())
    }) {
        Ok(config) => {
            issues.extend(
                unknown
                    .into_iter()
                    .map(|key| ConfigIssue::UnknownKey { key }),
            );
            config
        }
        Err(err) => {
            issues.push(ConfigIssue::SyntaxError {
                message: err.to_string().trim().to_string(),
            });
            Config::default()
        }
    }
}

fn check(root: &Table) -> Result<(), String> {
    Config::deserialize(Value::Table(root.clone()))
        .map(drop)
        .map_err(|e| e.to_string().trim().to_string())
}

fn apply_lenient(root: &mut Table, path: &[String], user: Table, issues: &mut Vec<ConfigIssue>) {
    for (key, value) in user {
        let mut key_path = path.to_vec();
        key_path.push(key);
        let mut candidate = root.clone();
        set_at(&mut candidate, &key_path, value.clone());
        let message = match check(&candidate) {
            Ok(()) => {
                *root = candidate;
                continue;
            }
            Err(message) => message,
        };
        match value {
            Value::Table(sub) if matches!(get_at(root, &key_path), Some(Value::Table(_))) => {
                apply_lenient(root, &key_path, sub, issues);
            }
            Value::Array(items) => {
                let mut kept = Vec::new();
                let mut dropped = Vec::new();
                for (index, item) in items.into_iter().enumerate() {
                    let mut attempt = kept.clone();
                    attempt.push(item);
                    let mut candidate = root.clone();
                    set_at(&mut candidate, &key_path, Value::Array(attempt.clone()));
                    match check(&candidate) {
                        Ok(()) => kept = attempt,
                        Err(message) => dropped.push(ConfigIssue::InvalidValue {
                            key: format!("{}[{index}]", key_path.join(".")),
                            message,
                        }),
                    }
                }
                let mut candidate = root.clone();
                set_at(&mut candidate, &key_path, Value::Array(kept));
                if check(&candidate).is_ok() {
                    *root = candidate;
                    issues.extend(dropped);
                } else {
                    issues.push(ConfigIssue::InvalidValue {
                        key: key_path.join("."),
                        message,
                    });
                }
            }
            _ => issues.push(ConfigIssue::InvalidValue {
                key: key_path.join("."),
                message,
            }),
        }
    }
}

fn get_at<'a>(table: &'a Table, path: &[String]) -> Option<&'a Value> {
    let (last, parents) = path.split_last()?;
    let mut current = table;
    for key in parents {
        current = current.get(key)?.as_table()?;
    }
    current.get(last)
}

fn set_at(table: &mut Table, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = table;
    for key in parents {
        let entry = current
            .entry(key.clone())
            .or_insert_with(|| Value::Table(Table::new()));
        if !entry.is_table() {
            *entry = Value::Table(Table::new());
        }
        let Value::Table(next) = entry else {
            return;
        };
        current = next;
    }
    current.insert(last.clone(), value);
}
