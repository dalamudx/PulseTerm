//! Synchronous private theme persistence. Dispatch off GUI threads; IDs are not authorization.
//! No automatic directory creation, permissions repair, default theme fallback, arbitrary basename,
//! file discovery, watcher, or theme application. Snapshot CAS coordinates cooperating writers;
//! noncooperating same-UID edits after final comparison are not atomic compare-and-rename.
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::private_file::{FileIoError, FileSlot, FileStore, PrivateSnapshot};
use crate::theme::{ThemeDocument, ThemeError, parse_theme_document};
use std::path::PathBuf;
use uuid::Uuid;
/// Static errors never contain paths, theme text, or OS diagnostics.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ThemeFileError {
    /// Platform ACL/atomic installation implementation is not available.
    #[error("theme.file.unsupported_platform")]
    Unsupported,
    /// Backend must select a non-nil UUID; this is not a window permission.
    #[error("theme.file.invalid_id")]
    Id,
    /// Directory/slot/ownership/alias/permissions check failed. Existing objects are not repaired.
    #[error("theme.file.private_location_required")]
    Location,
    /// Another cooperating writer owns the themes lock. No automatic retry.
    #[error("theme.file.busy")]
    Busy,
    /// Original identity/text changed. Re-read/reapply deliberately, never blind overwrite.
    #[error("theme.file.conflict")]
    Conflict,
    /// Failure before installation.
    #[error("theme.file.io_failed")]
    Io,
    /// Installed but receipt/directory durability was not confirmed. Re-read, never auto-replay.
    #[error("theme.file.commit_uncertain")]
    Uncertain,
    /// Bad complete theme/oversized file. Never reset it to defaults.
    #[error("theme.file.invalid_document")]
    Document(#[from] ThemeError),
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl From<FileIoError> for ThemeFileError {
    fn from(e: FileIoError) -> Self {
        match e {
            FileIoError::Location => Self::Location,
            FileIoError::Busy => Self::Busy,
            FileIoError::Conflict => Self::Conflict,
            FileIoError::Io => Self::Io,
            FileIoError::Uncertain => Self::Uncertain,
            FileIoError::Limit => Self::Document(ThemeError::Budget),
        }
    }
}
type Result<T> = std::result::Result<T, ThemeFileError>;
/// Bounded original document and opaque directory/slot identity; not a live permission or lease.
/// Intentionally not Debug/serde: comments may be sensitive.
/// ```compile_fail
/// fn log(s: pulse_store::theme_file::ThemeSnapshot) { let _ = format!("{s:?}"); }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct ThemeSnapshot(
    #[cfg(any(target_os = "linux", target_os = "macos"))] PrivateSnapshot,
    #[cfg(not(any(target_os = "linux", target_os = "macos")))] (),
);
impl ThemeSnapshot {
    /// Original validated text. Never log wholesale or use its display name as a filesystem path.
    pub fn text(&self) -> &str {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.0.text()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            ""
        }
    }
    /// Missing differs from present-but-empty (which is an invalid complete theme).
    pub fn exists(&self) -> bool {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.0.exists()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            false
        }
    }
    /// Parse the captured image, not the current file. No default fallback.
    pub fn document(&self) -> Result<Option<ThemeDocument>> {
        if self.exists() {
            Ok(Some(parse_theme_document(self.text())?))
        } else {
            Ok(None)
        }
    }
}
/// Owned no-follow private directory descriptor, closed UUID basename and global themes lock.
/// The caller separately authorizes operations. No directories are created or discovered here.
pub struct ThemeFile {
    id: Uuid,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    inner: FileStore,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate(s: &PrivateSnapshot) -> Result<()> {
    if s.exists() {
        parse_theme_document(s.text())?;
    }
    Ok(())
}
impl ThemeFile {
    /// Open an explicit trusted private directory; leaf is exactly `<backend UUID>.theme.toml`.
    /// Linux private identity checks apply; macOS runtime/Windows ACL acceptance remains separate.
    pub fn open_private(directory: PathBuf, id: Uuid) -> Result<Self> {
        if id.is_nil() {
            return Err(ThemeFileError::Id);
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Ok(Self {
                id,
                inner: FileStore::open(directory, FileSlot::Theme(id))?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = directory;
            Err(ThemeFileError::Unsupported)
        }
    }
    /// Plain backend identity; never a window/IPC permission.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Bounded captured image under themes lock. Invalid existing files are explicitly rejected.
    pub fn load(&self) -> Result<ThemeSnapshot> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.load(validate).map(ThemeSnapshot)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(ThemeFileError::Unsupported)
        }
    }
    /// Canonically replace the entire explicitly selected document after snapshot CAS.
    /// This is not a comment-preserving edit. Success includes directory fsync, not power-loss proof.
    pub fn save(
        &self,
        expected: &ThemeSnapshot,
        document: &ThemeDocument,
    ) -> Result<ThemeSnapshot> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner
                .save(&expected.0, document.to_toml()?, validate, |_| Ok(()))
                .map(ThemeSnapshot)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (expected, document);
            Err(ThemeFileError::Unsupported)
        }
    }
}
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "theme_file_tests.rs"]
mod tests;
