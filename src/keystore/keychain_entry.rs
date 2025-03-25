use strum_macros;

/// List from <https://man7.org/linux/man-pages/man5/ssh_config.5.html>
#[derive(Debug, strum_macros::Display, strum_macros::EnumString, Eq, PartialEq, Hash, Clone)]
#[strum(ascii_case_insensitive)]
pub enum EntryType {
    #[strum(disabled)]
    Unknown(String),
    Host,
    Match,
    Password
}
