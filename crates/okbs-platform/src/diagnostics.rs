//! Environment report printed by `okbswitch --diagnose`.

use std::fmt;

/// Outcome of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticStatus {
    /// Informational.
    Info,
    /// Works.
    Ok,
    /// Works with limitations.
    Warning,
    /// Does not work.
    Error,
}

impl DiagnosticStatus {
    fn label(self) -> &'static str {
        match self {
            DiagnosticStatus::Info => "info",
            DiagnosticStatus::Ok => " ok ",
            DiagnosticStatus::Warning => "warn",
            DiagnosticStatus::Error => "FAIL",
        }
    }
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticItem {
    /// Outcome.
    pub status: DiagnosticStatus,
    /// What was checked.
    pub name: String,
    /// Details and hints.
    pub detail: String,
}

/// A list of checks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticReport {
    /// Checks in order.
    pub items: Vec<DiagnosticItem>,
}

impl DiagnosticReport {
    /// Adds a check.
    pub fn push(
        &mut self,
        status: DiagnosticStatus,
        name: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.items.push(DiagnosticItem {
            status,
            name: name.into(),
            detail: detail.into(),
        });
    }

    /// Adds an informational line.
    pub fn info(&mut self, name: impl Into<String>, detail: impl Into<String>) {
        self.push(DiagnosticStatus::Info, name, detail);
    }

    /// Appends all items of `other`.
    pub fn extend(&mut self, other: DiagnosticReport) {
        self.items.extend(other.items);
    }

    /// Worst status in the report.
    pub fn worst(&self) -> DiagnosticStatus {
        self.items
            .iter()
            .map(|i| i.status)
            .max()
            .unwrap_or(DiagnosticStatus::Info)
    }
}

impl fmt::Display for DiagnosticReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self
            .items
            .iter()
            .map(|i| i.name.chars().count())
            .max()
            .unwrap_or(0);
        for item in &self.items {
            writeln!(
                f,
                "[{}] {:<width$}  {}",
                item.status.label(),
                item.name,
                item.detail
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_and_display() {
        let mut r = DiagnosticReport::default();
        assert_eq!(r.worst(), DiagnosticStatus::Info);
        r.info("os", "linux");
        r.push(DiagnosticStatus::Warning, "tray", "no StatusNotifier host");
        r.push(DiagnosticStatus::Ok, "input", "readable");
        assert_eq!(r.worst(), DiagnosticStatus::Warning);
        let text = r.to_string();
        assert!(
            text.contains("[warn] tray   no StatusNotifier host"),
            "{text}"
        );
        assert_eq!(text.lines().count(), 3);
    }
}
