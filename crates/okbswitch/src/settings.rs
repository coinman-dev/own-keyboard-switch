//! The configuration file owned by the running application.

use okbs_core::config::{self, Config};
use std::path::PathBuf;

/// Configuration and the file it is saved to.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Current configuration.
    pub config: Config,
    /// `config.toml` path.
    pub path: PathBuf,
    /// Do not overwrite a file written by a newer version.
    pub read_only: bool,
}

impl Settings {
    /// Replaces the whole configuration and saves it.
    pub fn replace(&mut self, config: Config) {
        self.update(|c| *c = config);
    }

    /// Applies `change` and saves the file when the configuration changed.
    pub fn update(&mut self, change: impl FnOnce(&mut Config)) {
        let before = self.config.clone();
        change(&mut self.config);
        if self.config == before || self.read_only {
            return;
        }
        if let Err(err) = config::save(&self.path, &self.config) {
            tracing::warn!("cannot save settings: {err}");
        }
    }
}
