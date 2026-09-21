//! Remembered clipboard texts («Следить за буфером обмена»).
//!
//! The newest text comes first and repeats of the newest one are dropped, so
//! copying the same text twice does not fill the list. With «Сохранять историю
//! буфера обмена после перезагрузки» the texts are written to a plain file in
//! the state directory; the texts are never written to the log.

use std::path::{Path, PathBuf};

/// Escapes one entry into a single line: a text may contain line breaks.
fn encode(text: &str) -> String {
    text.replace('\\', r"\\").replace('\n', r"\n")
}

/// Reverses [`encode`]. An unknown escape keeps its own characters.
fn decode(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The remembered texts, newest first.
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<String>,
    capacity: usize,
    file: Option<PathBuf>,
}

impl History {
    /// An empty history of at most `capacity` texts, saved to `file`.
    pub fn new(file: Option<PathBuf>, capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity: capacity.max(1),
            file,
        }
    }

    /// Texts, newest first.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Applies a new `clipboard.history_size`, dropping the oldest texts.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        self.entries.truncate(self.capacity);
    }

    /// Deletes the saved texts, e.g. when «Сохранять историю буфера обмена
    /// после перезагрузки» was switched off. The texts in memory are kept, and
    /// the file is written again if the option comes back.
    pub fn remove_file(&self) -> std::io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        match std::fs::remove_file(path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    /// Adds `text` in front. Returns whether the list changed.
    pub fn push(&mut self, text: String) -> bool {
        if text.is_empty() || self.entries.first().is_some_and(|first| *first == text) {
            return false;
        }
        self.entries.retain(|entry| *entry != text);
        self.entries.insert(0, text);
        self.entries.truncate(self.capacity);
        true
    }

    /// «Очистить».
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Reads the saved texts, replacing what is in memory. A missing file
    /// leaves the current texts alone: there is simply nothing to restore.
    pub fn load(&mut self) {
        let Some(path) = self.file.clone() else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                self.entries = contents
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(decode)
                    .take(self.capacity)
                    .collect();
                tracing::debug!(
                    entries = self.entries.len(),
                    "clipboard history restored from {}",
                    path.display()
                );
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!("cannot read {}: {err}", path.display()),
        }
    }

    /// Writes the texts. Does nothing when no file was configured.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut contents = String::new();
        for entry in &self.entries {
            contents.push_str(&encode(entry));
            contents.push('\n');
        }
        write_private(path, &contents)
    }
}

/// Writes the file so that other users of the machine cannot read it: the
/// history may hold texts the user copied out of private documents.
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())
    }
    // On Windows the state directory under the user profile is already
    // restricted to this account.
    #[cfg(not(unix))]
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_repeats_and_within_the_capacity() {
        let mut history = History::new(None, 3);
        assert!(history.push("one".into()));
        assert!(history.push("two".into()));
        assert!(
            !history.push("two".into()),
            "the newest text is not repeated"
        );
        assert!(
            history.push("one".into()),
            "an older text moves to the front"
        );
        assert_eq!(history.entries(), ["one", "two"]);
        assert!(history.push("three".into()));
        assert!(history.push("four".into()));
        assert_eq!(history.entries(), ["four", "three", "one"]);
        assert!(!history.push(String::new()));
        history.set_capacity(1);
        assert_eq!(history.entries(), ["four"]);
        history.clear();
        assert!(history.entries().is_empty());
    }

    #[test]
    fn multiline_texts_survive_a_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("clipboard-history.txt");
        let mut history = History::new(Some(path.clone()), 30);
        for text in [
            "plain",
            "two\nlines",
            r"back\slash",
            "escape\\nlooking",
            "  spaces  ",
        ] {
            assert!(history.push(text.into()));
        }
        history.save().unwrap();
        let mut restored = History::new(Some(path), 30);
        restored.load();
        assert_eq!(restored.entries(), history.entries());
        assert!(restored.entries().contains(&"two\nlines".to_string()));
        assert!(restored.entries().contains(&r"back\slash".to_string()));
    }

    #[test]
    fn loading_keeps_the_capacity_and_ignores_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clipboard-history.txt");
        let mut history = History::new(Some(path.clone()), 30);
        for index in 0..10 {
            history.push(format!("text {index}"));
        }
        history.save().unwrap();
        let mut small = History::new(Some(path), 4);
        small.load();
        assert_eq!(small.entries(), ["text 9", "text 8", "text 7", "text 6"]);

        // Nothing to restore on the first run: the texts already in memory stay.
        let mut missing = History::new(Some(dir.path().join("absent.txt")), 4);
        missing.push("kept".into());
        missing.load();
        assert_eq!(missing.entries(), ["kept"]);
    }

    #[test]
    fn without_a_file_nothing_is_written_or_removed() {
        let mut history = History::new(None, 5);
        history.push("text".into());
        history.save().unwrap();
        history.remove_file().unwrap();
        assert_eq!(history.entries(), ["text"]);
    }

    #[test]
    fn switching_the_option_off_deletes_the_saved_texts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clipboard-history.txt");
        let mut history = History::new(Some(path.clone()), 5);
        history.push("remembered".into());
        history.save().unwrap();
        assert!(path.exists());
        history.remove_file().unwrap();
        assert!(!path.exists());
        // Removing again is not an error, and the texts in memory are kept.
        history.remove_file().unwrap();
        assert_eq!(history.entries(), ["remembered"]);
        history.save().unwrap();
        assert!(path.exists(), "turning the option back on writes again");
    }
}
