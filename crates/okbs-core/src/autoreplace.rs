//! Autoreplace («Автозамена»): abbreviations expanded while typing.

use crate::config::{AutoReplace, AutoReplaceItem};
use std::collections::HashMap;

/// An expansion ready to be typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion<'a> {
    /// Index of the entry in the configuration.
    pub index: usize,
    /// The entry.
    pub item: &'a AutoReplaceItem,
    /// Moves of the caret to the left after typing `item.to` («Запоминать позицию курсора»).
    pub caret_left: usize,
}

/// Lookup table built from the autoreplace settings.
#[derive(Debug, Clone, Default)]
pub struct AutoReplacer {
    items: Vec<AutoReplaceItem>,
    by_abbreviation: HashMap<String, usize>,
    also_in_other_layout: bool,
}

impl AutoReplacer {
    /// Builds the table; later duplicates of an abbreviation are ignored.
    pub fn new(settings: &AutoReplace) -> Self {
        let items: Vec<AutoReplaceItem> = if settings.enabled {
            settings.items.clone()
        } else {
            Vec::new()
        };
        let mut by_abbreviation = HashMap::new();
        for (i, item) in items.iter().enumerate() {
            let key = item.from.trim().to_lowercase();
            if !key.is_empty() {
                by_abbreviation.entry(key).or_insert(i);
            }
        }
        Self {
            items,
            by_abbreviation,
            also_in_other_layout: settings.also_in_other_layout,
        }
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.by_abbreviation.is_empty()
    }

    fn expansion(&self, index: usize) -> Expansion<'_> {
        let item = &self.items[index];
        let len = item.to.chars().count();
        let caret_left = usize::try_from(item.cursor_pos).map_or(0, |pos| len.saturating_sub(pos));
        Expansion {
            index,
            item,
            caret_left,
        }
    }

    /// Finds the entry for a finished word. `typed` is the word as it appears
    /// on screen; `other_layout` is the same keys in the other layout and is
    /// used when «Заменять при наборе в другой раскладке» is on.
    pub fn find(&self, typed: &str, other_layout: Option<&str>) -> Option<Expansion<'_>> {
        let lookup = |w: &str| self.by_abbreviation.get(&w.to_lowercase()).copied();
        lookup(typed)
            .or_else(|| {
                other_layout
                    .filter(|_| self.also_in_other_layout)
                    .and_then(lookup)
            })
            .map(|i| self.expansion(i))
    }

    /// Entries in configuration order, for the insert menu and the tray list.
    pub fn items(&self) -> &[AutoReplaceItem] {
        &self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(also: bool) -> AutoReplace {
        AutoReplace {
            also_in_other_layout: also,
            items: vec![
                AutoReplaceItem {
                    from: "снп".into(),
                    to: "С наилучшими пожеланиями".into(),
                    cursor_pos: -1,
                },
                AutoReplaceItem {
                    from: "тег".into(),
                    to: "<b></b>".into(),
                    cursor_pos: 3,
                },
                AutoReplaceItem {
                    from: "СНП".into(),
                    to: "duplicate".into(),
                    cursor_pos: -1,
                },
            ],
            ..AutoReplace::default()
        }
    }

    #[test]
    fn finds_abbreviations() {
        let r = AutoReplacer::new(&settings(true));
        let e = r.find("снп", Some("cyg")).unwrap();
        assert_eq!((e.index, e.caret_left), (0, 0));
        assert_eq!(r.find("СНП", None).unwrap().index, 0);
        assert_eq!(r.find("cyg", Some("снп")).unwrap().index, 0);
        let tag = r.find("тег", None).unwrap();
        assert_eq!(tag.caret_left, 4);
        assert!(r.find("нет", Some("ytn")).is_none());
    }

    #[test]
    fn other_layout_option_and_disabled() {
        let r = AutoReplacer::new(&settings(false));
        assert!(r.find("cyg", Some("снп")).is_none());
        let disabled = AutoReplace {
            enabled: false,
            ..settings(true)
        };
        assert!(AutoReplacer::new(&disabled).is_empty());
    }
}
