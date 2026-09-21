//! Mapping from physical keys to the characters a layout produces.

use crate::keys::PhysKey;

/// Characters produced by each physical key in one keyboard layout,
/// without and with `Shift`.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyMap {
    chars: Vec<[Option<char>; 2]>,
}

impl KeyMap {
    /// An empty map: no key produces a character.
    pub fn new() -> Self {
        Self {
            chars: vec![[None, None]; PhysKey::COUNT],
        }
    }

    /// Sets the characters for `key`.
    pub fn set(&mut self, key: PhysKey, normal: Option<char>, shifted: Option<char>) {
        self.chars[key.index()] = [normal, shifted];
    }

    /// Character produced by `key` with or without `Shift`.
    pub fn get(&self, key: PhysKey, shift: bool) -> Option<char> {
        self.chars[key.index()][usize::from(shift)]
    }

    /// Finds the key and shift state that produce `ch`, preferring the unshifted variant.
    pub fn find(&self, ch: char) -> Option<(PhysKey, bool)> {
        PhysKey::ALL.iter().find_map(|&key| {
            let [normal, shifted] = self.chars[key.index()];
            if normal == Some(ch) {
                Some((key, false))
            } else if shifted == Some(ch) {
                Some((key, true))
            } else {
                None
            }
        })
    }

    /// Iterates over keys that produce at least one character.
    pub fn iter(&self) -> impl Iterator<Item = (PhysKey, Option<char>, Option<char>)> + '_ {
        PhysKey::ALL.iter().filter_map(|&key| {
            let [normal, shifted] = self.chars[key.index()];
            (normal.is_some() || shifted.is_some()).then_some((key, normal, shifted))
        })
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for KeyMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.iter().map(|(k, n, s)| (k, (n, s))))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_find() {
        let mut map = KeyMap::new();
        map.set(PhysKey::KeyQ, Some('й'), Some('Й'));
        map.set(PhysKey::Digit2, Some('2'), Some('"'));
        assert_eq!(map.get(PhysKey::KeyQ, false), Some('й'));
        assert_eq!(map.get(PhysKey::KeyQ, true), Some('Й'));
        assert_eq!(map.get(PhysKey::KeyW, false), None);
        assert_eq!(map.find('"'), Some((PhysKey::Digit2, true)));
        assert_eq!(map.find('x'), None);
        assert_eq!(map.iter().count(), 2);
    }
}
