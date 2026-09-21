//! Extra layout-rule matching, separate from the switching policy.
//!
//! A match is evidence, not an instruction to change the user's text.

use std::collections::BTreeMap;

const E: u8 = 0x10;
const A: u8 = 0x20;
const D: u8 = 0x40;

/// How a pattern is compared with one reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Contains,
    StartsWith,
    Equals,
}

/// The first matching built-in record, including its original source location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleMatch {
    pub source_line: u32,
    pub evaluation_priority: usize,
    pub mask: u8,
    pub match_mode: MatchMode,
}

impl RuleMatch {
    pub fn exception(self) -> bool {
        self.mask & E != 0
    }

    pub fn unconditional(self) -> bool {
        self.mask & A != 0
    }
}

#[derive(Debug)]
struct Record {
    matched: RuleMatch,
    pattern: String,
}

#[derive(Debug, Default)]
struct Node {
    edges: BTreeMap<u8, usize>,
    records: Vec<usize>,
}

/// Two byte tries: original case and lowercase input. Patterns are never folded.
#[derive(Debug)]
pub struct RuleDatabase {
    records: Vec<Record>,
    tries: [Vec<Node>; 2],
}

/// Exact input normalization of the studied matcher's default configuration.
/// Only a trailing run is removed; leading punctuation, commas and periods stay.
pub fn normalize(text: &str) -> &str {
    text.trim_end_matches(['!', '?', ' ', '\t', '\r', '\n'])
}

impl RuleDatabase {
    /// Validates the versioned, length-prefixed generated format.
    pub fn from_bytes(mut bytes: &[u8]) -> Result<Self, &'static str> {
        fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8], &'static str> {
            let (head, tail) = bytes.split_at_checked(len).ok_or("truncated rules")?;
            *bytes = tail;
            Ok(head)
        }
        fn u32le(bytes: &mut &[u8]) -> Result<u32, &'static str> {
            Ok(u32::from_le_bytes(
                take(bytes, 4)?.try_into().map_err(|_| "integer")?,
            ))
        }
        if take(&mut bytes, 8)? != b"OKBEXR01" {
            return Err("unknown rule format");
        }
        let count = u32le(&mut bytes)? as usize;
        if count > 100_000 || count > bytes.len() / 9 {
            return Err("invalid record count");
        }
        let mut records = Vec::with_capacity(count);
        let mut previous_line = 0;
        for _ in 0..count {
            let source_line = u32le(&mut bytes)?;
            if source_line <= previous_line {
                return Err("source lines out of order");
            }
            previous_line = source_line;
            let mask = take(&mut bytes, 1)?[0];
            if mask & 0x80 != 0 {
                return Err("user rules are not built-in records");
            }
            let match_mode = match mask & 11 {
                1 => MatchMode::Equals,
                2 => MatchMode::Contains,
                8 => MatchMode::StartsWith,
                _ => return Err("invalid match mode"),
            };
            let length = u32le(&mut bytes)? as usize;
            let pattern = std::str::from_utf8(take(&mut bytes, length)?)
                .map_err(|_| "pattern is not UTF-8")?
                .to_owned();
            records.push(Record {
                matched: RuleMatch {
                    source_line,
                    mask,
                    match_mode,
                    evaluation_priority: 0,
                },
                pattern,
            });
        }
        if !bytes.is_empty() {
            return Err("trailing rule data");
        }
        // Stable grouping is essential: A, E, other. Retain all duplicates.
        records.sort_by_key(|r| {
            if r.matched.mask & A != 0 {
                0
            } else if r.matched.mask & E != 0 {
                1
            } else {
                2
            }
        });
        let mut tries = [vec![Node::default()], vec![Node::default()]];
        for (priority, record) in records.iter_mut().enumerate() {
            record.matched.evaluation_priority = priority;
            let trie = &mut tries[usize::from(record.matched.mask & 4 != 0)];
            let mut node = 0;
            for byte in record.pattern.bytes() {
                let next = if let Some(&next) = trie[node].edges.get(&byte) {
                    next
                } else {
                    let next = trie.len();
                    trie.push(Node::default());
                    trie[node].edges.insert(byte, next);
                    next
                };
                node = next;
            }
            trie[node].records.push(priority);
        }
        Ok(Self { records, tries })
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// First match by evaluation priority, never by length or trie traversal order.
    pub fn find(&self, text: &str) -> Option<RuleMatch> {
        let original = normalize(text);
        let lower = original.to_lowercase();
        let mut best = None;
        for (trie, input) in self.tries.iter().zip([lower.as_str(), original]) {
            for start in input.char_indices().map(|(i, _)| i).chain([input.len()]) {
                let mut node = 0;
                let mut end = start;
                loop {
                    for &priority in &trie[node].records {
                        if best.is_some_and(|old| old <= priority) {
                            continue;
                        }
                        let mode = self.records[priority].matched.match_mode;
                        if mode == MatchMode::Contains
                            || (start == 0 && (mode == MatchMode::StartsWith || end == input.len()))
                        {
                            best = Some(priority);
                        }
                    }
                    let Some(next) = input
                        .as_bytes()
                        .get(end)
                        .and_then(|b| trie[node].edges.get(b))
                    else {
                        break;
                    };
                    node = *next;
                    end += 1;
                }
            }
        }
        best.map(|i| self.records[i].matched)
    }

    /// Simple reference implementation for checking the optimized index.
    pub fn find_reference(&self, text: &str) -> Option<RuleMatch> {
        let original = normalize(text);
        let lower = original.to_lowercase();
        self.records
            .iter()
            .find(|record| {
                let input = if record.matched.mask & 4 != 0 {
                    original
                } else {
                    &lower
                };
                match record.matched.match_mode {
                    MatchMode::Contains => input.contains(&record.pattern),
                    MatchMode::StartsWith => input.starts_with(&record.pattern),
                    MatchMode::Equals => input == record.pattern,
                }
            })
            .map(|r| r.matched)
    }
}

/// The recovered built-in layout-rule branch, before separate case correction.
/// It excludes imported user rules and prior-action suppression state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleVerdict {
    NoEvidence,
    Stay,
    Switch,
    /// Both readings look wrong; this branch does not choose a layout.
    Conflict,
}

pub fn resolve(
    current: Option<RuleMatch>,
    alternative: Option<RuleMatch>,
    minus_key: bool,
) -> RuleVerdict {
    let Some(current) = current else {
        return RuleVerdict::NoEvidence;
    };
    if current.exception() {
        return RuleVerdict::Stay;
    }
    // VA 0x41c6f5..0x41c743: D suppresses this branch on VK_SUBTRACT/OEM_MINUS.
    if minus_key && current.mask & D != 0 {
        return RuleVerdict::NoEvidence;
    }
    if !current.unconditional() {
        if alternative.is_some_and(RuleMatch::unconditional) {
            return RuleVerdict::Stay;
        }
        if alternative.is_some_and(|r| !r.exception()) {
            return RuleVerdict::Conflict;
        }
    }
    RuleVerdict::Switch
}

#[cfg(feature = "builtin-data")]
pub fn builtin() -> &'static RuleDatabase {
    static DB: std::sync::OnceLock<RuleDatabase> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/generated/extra.rules.z"
        ));
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(bytes, 4 * 1024 * 1024)
            .expect("embedded rule compression is valid");
        RuleDatabase::from_bytes(&raw).expect("embedded rule database is valid")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database(records: &[(u8, &str)]) -> RuleDatabase {
        let mut data = b"OKBEXR01".to_vec();
        data.extend((records.len() as u32).to_le_bytes());
        for (i, (mask, pattern)) in records.iter().enumerate() {
            data.extend((i as u32 + 2).to_le_bytes());
            data.push(*mask);
            data.extend((pattern.len() as u32).to_le_bytes());
            data.extend(pattern.as_bytes());
        }
        RuleDatabase::from_bytes(&data).expect("fixture")
    }

    #[test]
    fn modes_case_priority_and_normalization() {
        let db = database(&[
            (2, "bc"),
            (1 | E, "abc"),
            (8 | A, "ab"),
            (1 | 4, "UP"),
            (1, "MiX"),
        ]);
        assert_eq!(db.find("abc").unwrap().source_line, 4); // A before E
        assert_eq!(db.find("xbc").unwrap().source_line, 2);
        assert_eq!(db.find("UP!? \r\n").unwrap().source_line, 5);
        assert_eq!(db.find("up"), None);
        assert_eq!(db.find("MiX"), None); // patterns are not lowercased
        assert_eq!(db.find("abc."), db.find_reference("abc."));
        assert_eq!(normalize(" !ab!? \t"), " !ab");
        assert_eq!(normalize("word.,-"), "word.,-");
        let db = database(&[(2, "bc"), (2, "abcdef"), (2, "bc")]);
        assert_eq!(db.find("abcdef").unwrap().source_line, 2); // not longest/leftmost
        let db = database(&[(1, "word "), (8, "in ")]);
        assert_eq!(db.find("word "), None);
        assert_eq!(db.find("in progress").unwrap().source_line, 3);
    }

    #[test]
    fn result_flags_are_not_unconditional_actions() {
        let record = |mask| {
            Some(RuleMatch {
                source_line: 2,
                mask,
                evaluation_priority: 0,
                match_mode: MatchMode::Equals,
            })
        };
        assert_eq!(resolve(None, record(E | 1), false), RuleVerdict::NoEvidence);
        assert_eq!(resolve(record(E | 1), None, false), RuleVerdict::Stay);
        assert_eq!(resolve(record(1), None, false), RuleVerdict::Switch);
        assert_eq!(resolve(record(1), record(1), false), RuleVerdict::Conflict);
        assert_eq!(resolve(record(1), record(A | 1), false), RuleVerdict::Stay);
        assert_eq!(
            resolve(record(A | 1), record(1), false),
            RuleVerdict::Switch
        );
        assert_eq!(resolve(record(D | 1), None, true), RuleVerdict::NoEvidence);
        assert_eq!(resolve(record(D | 1), None, false), RuleVerdict::Switch);
    }

    #[test]
    fn rejects_corrupt_format() {
        assert!(RuleDatabase::from_bytes(b"OKBSPR02").is_err());
        assert!(RuleDatabase::from_bytes(b"OKBEXR01\xff\xff\xff\xff").is_err());
        assert!(RuleDatabase::from_bytes(b"OKBEXR01\0\0\0\0garbage").is_err());
    }

    #[test]
    #[cfg(feature = "builtin-data")]
    fn imported_records_and_index_match_reference() {
        let db = builtin();
        assert_eq!(db.len(), 37747);
        let by_line = |line| {
            db.records
                .iter()
                .find(|r| r.matched.source_line == line)
                .unwrap()
        };
        assert_eq!(by_line(220).pattern, "юучу ");
        assert_eq!(by_line(529).pattern, "by'n ");
        for line in [7939, 7940, 7941, 36472] {
            assert_eq!(by_line(line).matched.match_mode, MatchMode::Equals);
        }
        for record in db.records.iter().step_by(137) {
            for input in [
                record.pattern.clone(),
                record.pattern.to_uppercase(),
                format!("x{}!? ", record.pattern),
                format!("{}xyz", record.pattern),
            ] {
                assert_eq!(db.find(&input), db.find_reference(&input), "{input:?}");
            }
        }
    }
}
