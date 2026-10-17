//! Shared private-directory IO. Closed slots, bounded text, atomic replace; no public path capability.
use std::path::PathBuf;
use uuid::Uuid;
#[path = "private_file_unix.rs"]
pub(crate) mod unix;
pub(crate) use unix::{FileStore, Stage};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileSlot {
    Settings,
    Keymap,
    Theme(Uuid),
}
impl FileSlot {
    fn limit(self) -> usize {
        match self {
            Self::Settings => crate::settings::SETTINGS_TEXT_LIMIT,
            Self::Keymap => crate::keymap::KEYMAP_TEXT_LIMIT,
            Self::Theme(_) => crate::theme::MAX_SCHEME_BYTES,
        }
    }
    fn target(self) -> String {
        match self {
            Self::Settings => "settings.toml".into(),
            Self::Keymap => "keymap.toml".into(),
            Self::Theme(id) => format!("{id}.theme.toml"),
        }
    }
    fn lock(self) -> &'static str {
        match self {
            Self::Settings => ".settings.lock",
            Self::Keymap => ".keymap.lock",
            Self::Theme(_) => ".themes.lock",
        }
    }
    fn stem(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Keymap => "keymap",
            Self::Theme(_) => "theme",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileIoError {
    Location,
    Busy,
    Conflict,
    Io,
    Uncertain,
    Limit,
}
type Result<T> = std::result::Result<T, FileIoError>;
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}
// No Debug or serde: input might contain sensitive comments.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PrivateSnapshot {
    directory: (u64, u64),
    slot: FileSlot,
    stamp: Option<Stamp>,
    text: String,
}
impl PrivateSnapshot {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
    pub(crate) fn exists(&self) -> bool {
        self.stamp.is_some()
    }
}
