//! Fixed-name private settings IO. Synchronous: dispatch away from GUI threads.
use crate::{
    settings::SettingsError, settings_edit::TerminalSettingEdit,
    settings_remove::TerminalSettingKey,
};
use std::path::PathBuf;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "settings_file_unix.rs"]
mod unix;
/// Stable errors never embed paths, document contents or OS error diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingsFileError {
    /// Platform atomic replace and ACL handling have not been validated.
    #[error("settings.file.unsupported_platform")]
    Unsupported,
    /// Require a trusted absolute directory, private ownership and unaliased files.
    #[error("settings.file.private_location_required")]
    Location,
    /// Another cooperating operation holds the fixed lock; do not auto-retry.
    #[error("settings.file.busy")]
    Busy,
    /// Snapshot identity/text no longer match; reload and reapply deliberately.
    #[error("settings.file.conflict")]
    Conflict,
    /// Failure before installation; original target was not replaced by this operation.
    #[error("settings.file.io_failed")]
    Io,
    /// Rename occurred but durability/receipt could not be confirmed; reload, never replay.
    #[error("settings.file.commit_uncertain")]
    Uncertain,
    /// Reject bad documents instead of overwriting/resetting them.
    #[error("settings.file.invalid_document")]
    Document(#[from] SettingsError),
}
type Result<T> = std::result::Result<T, SettingsFileError>;
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
/// Bounded validated original text plus file/directory identity. Not a path capability.
/// No Debug/serde; comments might contain information inappropriate for logs.
#[derive(Clone, PartialEq, Eq)]
pub struct SettingsSnapshot {
    directory: (u64, u64),
    stamp: Option<Stamp>,
    text: String,
}
impl SettingsSnapshot {
    /// Original non-secret configuration text; caller must not log comments wholesale.
    pub fn text(&self) -> &str {
        &self.text
    }
    /// A missing file differs from a present empty file for conflict detection.
    pub fn exists(&self) -> bool {
        self.stamp.is_some()
    }
}
/// Only supported display fields; no raw replacement, arbitrary filename or executable setting.
pub enum SettingsMutation {
    /// Update one supported scalar.
    Set(TerminalSettingEdit),
    /// Remove one scalar to restore lower-layer provenance.
    Remove(TerminalSettingKey),
}
/// Caller chooses a trusted private directory, never a basename or arbitrary file target.
/// Fixed settings.toml and .settings.lock; cooperating processes use try-lock + snapshot CAS.
/// Noncooperating same-UID edits after the final check are not an atomic compare-and-rename.
pub struct SettingsFile {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    inner: unix::FileStore,
}
impl SettingsFile {
    /// Pin a private directory with a no-follow descriptor. Does not read user defaults.
    pub fn open_private(directory: PathBuf) -> Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Ok(Self {
                inner: unix::FileStore::open(directory)?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = directory;
            Err(SettingsFileError::Unsupported)
        }
    }
    /// Take a bounded validated snapshot under the cooperating-process lock.
    pub fn load(&self) -> Result<SettingsSnapshot> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.load()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(SettingsFileError::Unsupported)
        }
    }
    /// Compare original file identity/text and atomically install only a typed edit.
    /// Successful return includes directory fsync; it is a commit snapshot, not a live lease.
    pub fn save(
        &self,
        expected: &SettingsSnapshot,
        change: SettingsMutation,
    ) -> Result<SettingsSnapshot> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.save(expected, change, |_| Ok(()))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (expected, change);
            Err(SettingsFileError::Unsupported)
        }
    }
}
