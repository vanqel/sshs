use super::key_parser_error::KeyParseError;
use super::key_parser_error::KeyUnknownEntryError;
use super::{EntryType, Keychain};
use crate::keystore::keychain::Entry;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug)]
pub struct KeyParser {
    ignore_unknown_entries: bool,
}

impl Default for KeyParser {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyParser {
    #[must_use]
    pub fn new() -> KeyParser {
        KeyParser {
            ignore_unknown_entries: true,
        }
    }

    /// # Errors
    ///
    /// Will return `Err` if the SSH configuration cannot be parsed.
    pub fn parse_file<P>(&self, path: P) -> Result<Vec<Keychain>, KeyParseError>
    where
        P: AsRef<Path>,
    {
        let mut reader = BufReader::new(File::open(path)?);
        self.parse(&mut reader)
    }

    /// # Errors
    ///
    /// Will return `Err` if the SSH configuration cannot be parsed.
    pub fn parse(&self, reader: &mut impl BufRead) -> Result<Vec<Keychain>, KeyParseError> {
        let (global_keychain, mut keychains) = self.parse_raw(reader)?;

        if !global_keychain.is_empty() {
            for Keychain in &mut keychains {
                Keychain.extend_if_not_contained(&global_keychain);
            }
        }

        Ok(keychains)
    }

    fn parse_raw(&self, reader: &mut impl BufRead) -> Result<(Keychain, Vec<Keychain>), KeyParseError> {
        let mut global_keychain = Keychain::new(Vec::new());
        let mut is_in_keychain_block = false;
        let mut keychains = Vec::new();

        let mut line = String::new();
        while reader.read_line(&mut line)? > 0 {
            // We separate parts that contain comments with #
            line = if line.contains('#') && !line.trim().starts_with("#!") {
                line.split('#').next().unwrap().to_string()
            } else {
                line.replace("#!", "").trim().to_string()
            };

            if line.is_empty() || line.starts_with('#') {
                line.clear();
                continue;
            }

            let entry = parse_line(&line)?;
            line.clear();

            match entry.0 {
                EntryType::Unknown(_) => {
                    if !self.ignore_unknown_entries {
                        return Err(KeyUnknownEntryError {
                            line,
                            entry: entry.0.to_string(),
                        }
                        .into());
                    }
                }
                EntryType::Host => {
                    let patterns = parse_patterns(&entry.1);
                    keychains.push(Keychain::new(patterns));
                    is_in_keychain_block = true;

                    continue;
                }
                _ => {}
            }

            if is_in_keychain_block {
                keychains.last_mut().unwrap().update(entry);
            } else {
                global_keychain.update(entry);
            }
        }

        Ok((global_keychain, keychains))
    }
}

fn parse_line(line: &str) -> Result<Entry, KeyParseError> {
    let (mut key, mut value) = line
        .trim()
        .split_once([' ', '\t', '='])
        .map(|(k, v)| (k.trim_end(), v.trim_start()))
        .ok_or(KeyParseError::UnparseableLine(line.to_string()))?;

    // Format can be key=value with whitespaces around the equal sign, strip the equal sign and whitespaces
    if key.ends_with('=') {
        key = key.trim_end_matches('=').trim_end();
    }
    if value.starts_with('=') {
        value = value.trim_start_matches('=').trim_start();
    }

    Ok((
        EntryType::from_str(key).unwrap_or(EntryType::Unknown(key.to_string())),
        value.to_string(),
    ))
}

fn parse_patterns(entry_value: &str) -> Vec<String> {
    let mut patterns = Vec::new();

    let mut pattern = String::new();
    let mut in_double_quotes = false;

    for c in entry_value.chars() {
        if c == '"' {
            if in_double_quotes {
                patterns.push(pattern.trim().to_string());
                pattern.clear();

                in_double_quotes = false;
            } else {
                in_double_quotes = true;
            }
        } else if c.is_whitespace() {
            if in_double_quotes {
                pattern.push(c);
            } else if !pattern.is_empty() {
                patterns.push(pattern.trim().to_string());
                pattern.clear();
            }
        } else {
            pattern.push(c);
        }
    }

    if !pattern.is_empty() {
        patterns.push(pattern.trim().to_string());
    }

    patterns
}
