use regex::Regex;
use std::collections::HashMap;

use super::EntryType;

pub(crate) type Entry = (EntryType, String);

#[derive(Debug, Clone)]
pub struct Keychain {
    patterns: Vec<String>,
    entries: HashMap<EntryType, String>,
}

impl Keychain {
    #[must_use]
    pub fn new(patterns: Vec<String>) -> Keychain {
        Keychain {
            patterns,
            entries: HashMap::new(),
        }
    }

    pub fn update(&mut self, entry: Entry) {
        self.entries.insert(entry.0, entry.1);
    }

    pub(crate) fn extend_patterns(&mut self, keys: &Keychain) {
        self.patterns.extend(keys.patterns.clone());
    }

    pub(crate) fn extend_entries(&mut self, keys: &Keychain) {
        self.entries.extend(keys.entries.clone());
    }

    pub(crate) fn extend_if_not_contained(&mut self, keys: &Keychain) {
        for (key, value) in &keys.entries {
            if !self.entries.contains_key(key) {
                self.entries.insert(key.clone(), value.clone());
            }
        }
    }

    #[allow(clippy::must_use_candidate)]
    pub fn get_patterns(&self) -> &Vec<String> {
        &self.patterns
    }

    /// # Panics
    ///
    /// Will panic if the regex cannot be compiled.
    #[allow(clippy::must_use_candidate)]
    pub fn matching_pattern_regexes(&self) -> Vec<(Regex, bool)> {
        if self.patterns.is_empty() {
            return Vec::new();
        }

        self.patterns
            .iter()
            .filter_map(|pattern| {
                let contains_wildcard =
                    pattern.contains('*') || pattern.contains('?') || pattern.contains('!');
                if !contains_wildcard {
                    return None;
                }

                let mut pattern = pattern
                    .replace('.', r"\.")
                    .replace('*', ".*")
                    .replace('?', ".");

                let is_negated = pattern.starts_with('!');
                if is_negated {
                    pattern.remove(0);
                }

                pattern = format!("^{pattern}$");
                Some((Regex::new(&pattern).unwrap(), is_negated))
            })
            .collect()
    }

    #[allow(clippy::must_use_candidate)]
    pub fn get(&self, entry: &EntryType) -> Option<String> {
        self.entries.get(entry).cloned()
    }

    #[allow(clippy::must_use_candidate)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[allow(clippy::module_name_repetitions)]
pub trait KeysVecExt {
    /// Apply the name entry to the keysname entry if the keysname entry is empty.
    #[must_use]
    fn apply_name_to_empty_keysname(&self) -> Self;

    /// Merges the keyss with the same entries into one keys.
    #[must_use]
    fn merge_same_keyss(&self) -> Self;

    /// Spreads the keyss with multiple patterns into multiple keyss with one pattern.
    #[must_use]
    fn spread(&self) -> Self;

    /// Apply patterns entries to non-pattern keyss and remove the pattern keyss.
    #[must_use]
    fn apply_patterns(&self) -> Self;
}

impl KeysVecExt for Vec<Keychain> {
    fn apply_name_to_empty_keysname(&self) -> Self {
        let mut keyss = self.clone();

        for keys in &mut keyss {
            if keys.get(&EntryType::Host).is_none() {
                let name = keys.patterns.first().unwrap().clone();
                keys.update((EntryType::Host, name));
            }
        }

        keyss
    }

    fn merge_same_keyss(&self) -> Self {
        let mut keyss = self.clone();

        for i in (0..keyss.len()).rev() {
            let (left, right) = keyss.split_at_mut(i); // Split into left and right parts

            let current_keys = &right[0];

            for j in (0..i).rev() {
                let target_keys = &mut left[j];

                if current_keys.entries != target_keys.entries {
                    continue;
                }

                if current_keys
                    .entries
                    .values()
                    .any(|value| value.contains("%h"))
                {
                    continue;
                }

                target_keys.extend_patterns(current_keys);
                target_keys.extend_entries(current_keys);
                keyss.remove(i);
                break;
            }
        }

        keyss
    }

    fn spread(&self) -> Vec<Keychain> {
        let mut keyss = Vec::new();

        for keys in self {
            let patterns = keys.get_patterns();
            if patterns.is_empty() {
                keyss.push(keys.clone());
                continue;
            }

            for pattern in patterns {
                let mut new_keys = keys.clone();
                new_keys.patterns = vec![pattern.clone()];
                keyss.push(new_keys);
            }
        }

        keyss
    }

    /// Apply patterns entries to non-pattern keyss and remove the pattern keyss.
    ///
    /// You might want to call [`KeysVecExt::merge_same_keyss`] after this.
    fn apply_patterns(&self) -> Self {
        let mut keyss = self.spread();
        let mut pattern_indexes = Vec::new();

        for i in 0..keyss.len() {
            let matching_pattern_regexes = keyss[i].matching_pattern_regexes();
            if matching_pattern_regexes.is_empty() {
                continue;
            }

            pattern_indexes.push(i);

            for j in 0..keyss.len() {
                if i == j {
                    continue;
                }

                if !keyss[j].matching_pattern_regexes().is_empty() {
                    continue;
                }

                for (regex, is_negated) in &matching_pattern_regexes {
                    if regex.is_match(&keyss[j].patterns[0]) == *is_negated {
                        continue;
                    }

                    let keys = keyss[i].clone();
                    keyss[j].extend_if_not_contained(&keys);
                    break;
                }
            }
        }

        for i in pattern_indexes.into_iter().rev() {
            keyss.remove(i);
        }

        keyss
    }
}

