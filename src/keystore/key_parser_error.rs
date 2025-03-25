#[derive(Debug)]
pub struct KeyUnknownEntryError {
    pub line: String,
    pub entry: String,
}

#[derive(Debug)]
pub enum KeyInvalidIncludeErrorDetails {
    Pattern(glob::PatternError),
    Glob(glob::GlobError),
    Io(std::io::Error),
    HostsInsideHostBlock,
}

#[derive(Debug)]
pub struct KeyInvalidIncludeError {
    pub line: String,
    pub details: KeyInvalidIncludeErrorDetails,
}

#[derive(Debug)]
pub enum KeyParseError {
    Io(std::io::Error),
    UnparseableLine(String),
    UnknownEntry(KeyUnknownEntryError),
    InvalidInclude(KeyInvalidIncludeError),
}

impl From<std::io::Error> for KeyParseError {
    fn from(e: std::io::Error) -> Self {
        KeyParseError::Io(e)
    }
}

impl From<KeyUnknownEntryError> for KeyParseError {
    fn from(e: KeyUnknownEntryError) -> Self {
        KeyParseError::UnknownEntry(e)
    }
}

impl From<KeyInvalidIncludeError> for KeyParseError {
    fn from(e: KeyInvalidIncludeError) -> Self {
        KeyParseError::InvalidInclude(e)
    }
}
