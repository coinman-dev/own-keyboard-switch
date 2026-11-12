//! Release versions (`0.6.0-beta`, tags `v0.6.0-beta`) and their order, for
//! the update check.

use std::cmp::Ordering;

/// A parsed `major.minor.patch[-pre]` version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    numbers: [u64; 3],
    /// Pre-release identifiers, e.g. `["beta", "2"]`; empty for a release.
    pre: Vec<String>,
}

impl Version {
    /// Parses `0.6.0-beta` or a tag `v0.6.0-beta`; build metadata is ignored.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((_, "")) => return None,
            Some((core, pre)) => (core, pre),
            None => (text, ""),
        };
        let mut parts = core.split('.');
        let mut numbers = [0; 3];
        for number in &mut numbers {
            *number = parts.next()?.parse().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }
        let pre: Vec<String> = if pre.is_empty() {
            Vec::new()
        } else {
            pre.split('.').map(str::to_owned).collect()
        };
        if pre.iter().any(|part| part.is_empty()) {
            return None;
        }
        Some(Self { numbers, pre })
    }

    /// A pre-release such as `0.6.0-beta`.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    /// Semantic Versioning precedence: a release follows its pre-releases,
    /// numeric identifiers compare as numbers and before text ones.
    fn cmp(&self, other: &Self) -> Ordering {
        self.numbers.cmp(&other.numbers).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&other.pre) {
                        let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(a), Ok(b)) => a.cmp(&b),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if order != Ordering::Equal {
                            return order;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn parses_tags_and_rejects_garbage() {
        assert_eq!(v("v0.6.0-beta"), v("0.6.0-beta"));
        assert!(v("0.6.0-beta").is_prerelease());
        assert!(!v("1.0.0").is_prerelease());
        for bad in ["", "v1", "1.2", "1.2.3.4", "1.x.3", "1.2.3-", "1.2.3-a..b"] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn follows_semantic_versioning_precedence() {
        let ordered = [
            "0.5.0-beta",
            "0.6.0-alpha",
            "0.6.0-beta",
            "0.6.0-beta.2",
            "0.6.0-beta.10",
            "0.6.0-rc.1",
            "0.6.0",
            "0.6.1-beta",
            "0.10.0",
            "1.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(v("1.0.0+build.5").cmp(&v("1.0.0")), Ordering::Equal);
    }
}
