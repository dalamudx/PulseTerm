//! Synchronous fixed-name keymap IO. Call from supervised blocking work, not GUI threads.
use crate::keymap::{KeymapAction, KeymapError, KeymapOverride, KeymapPatch, KeymapProjection};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::private_file::{FileIoError, FileSlot, FileStore, PrivateSnapshot, Stage};
use std::{path::PathBuf, sync::Arc};
/// Stable IO errors never contain paths or document contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeymapFileError {
    /// Native operation authorization was revoked or panicked before installation.
    #[error("keymap.file.authorization")]
    Authorization,
    /// Atomic replacement and ACL handling are not implemented for this platform.
    #[error("keymap.file.unsupported_platform")]
    Unsupported,
    /// Private ownership, no-follow, mode or unaliased identity checks failed.
    #[error("keymap.file.private_location_required")]
    Location,
    /// A cooperating operation owns the fixed keymap lock; do not retry writes automatically.
    #[error("keymap.file.busy")]
    Busy,
    /// Captured instance or file identity/text differs; reload and reapply deliberately.
    #[error("keymap.file.conflict")]
    Conflict,
    /// IO failed before installation.
    #[error("keymap.file.io_failed")]
    Io,
    /// Installation happened, but receipt/durability cannot be confirmed; reload, never replay.
    #[error("keymap.file.commit_uncertain")]
    Uncertain,
    /// Invalid complete source or edited document is not overwritten or reset.
    #[error("keymap.file.invalid_document")]
    Document(#[from] KeymapError),
}
type Result<T> = std::result::Result<T, KeymapFileError>;
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl From<FileIoError> for KeymapFileError {
    fn from(error: FileIoError) -> Self {
        match error {
            FileIoError::Location => Self::Location,
            FileIoError::Busy => Self::Busy,
            FileIoError::Conflict => Self::Conflict,
            FileIoError::Io => Self::Io,
            FileIoError::Uncertain => Self::Uncertain,
            FileIoError::Limit => Self::Document(KeymapError::Size),
        }
    }
}
/// Original bounded image, bound to its store instance and private directory. Not a permission.
/// No Debug/serde: source comments may contain private information.
/// ```compile_fail
/// fn log(s: pulse_store::keymap_file::KeymapSnapshot) { let _ = format!("{s:?}"); }
/// ```
#[derive(Clone)]
pub struct KeymapSnapshot {
    source: Arc<()>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    inner: PrivateSnapshot,
}
impl KeymapSnapshot {
    /// Capture-local validated overrides; missing file inherits, present empty file is invalid.
    pub fn document(&self) -> Result<KeymapPatch> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if self.inner.exists() {
                Ok(KeymapPatch::parse(self.inner.text())?)
            } else {
                Ok(KeymapPatch {
                    schema_version: 1,
                    bindings: Vec::new(),
                })
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(KeymapFileError::Unsupported)
        }
    }
    /// Resolve only this captured source, never reread the current file or grant execution.
    pub fn projection(&self) -> Result<KeymapProjection> {
        Ok(self.document()?.resolve()?)
    }
}
/// Typed per-action edits only; no executable arguments or arbitrary file paths.
pub enum KeymapMutation {
    /// Replace this action's alias list/condition; empty keys explicitly disables its shortcuts.
    Set(KeymapOverride),
    /// Remove this action's override to reveal built-in aliases.
    Restore(KeymapAction),
}
/// Instance-owned trusted-directory descriptor and fixed keymap basename/lock.
/// Opening does not create directories or discover a user's configuration location.
pub struct KeymapFile {
    source: Arc<()>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    inner: FileStore,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate(snapshot: &PrivateSnapshot) -> Result<()> {
    if snapshot.exists() {
        KeymapPatch::parse(snapshot.text())?;
    }
    Ok(())
}
impl KeymapFile {
    /// Pin a trusted private directory; Unix checks apply, macOS runtime acceptance remains separate.
    /// Unsupported platforms fail closed rather than pretend Windows ACL validation passed.
    pub fn open_private(directory: PathBuf) -> Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Ok(Self {
                source: Arc::new(()),
                inner: FileStore::open(directory, FileSlot::Keymap)?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = directory;
            Err(KeymapFileError::Unsupported)
        }
    }
    /// Take a bounded validated image under the cooperating-process lock.
    pub fn load(&self) -> Result<KeymapSnapshot> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Ok(KeymapSnapshot {
                source: self.source.clone(),
                inner: self.inner.load(validate)?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(KeymapFileError::Unsupported)
        }
    }
    /// Check the original instance and complete source image under the cooperating-file lock,
    /// then invoke a trusted current-context check inside that lock. This grants no Native permission.
    /// A successful point-in-time check does not freeze uncooperative external editors afterward.
    pub fn confirm_current_authorized(
        &self,
        expected: &KeymapSnapshot,
        authorize: impl Fn() -> bool,
    ) -> Result<()> {
        if !Arc::ptr_eq(&self.source, &expected.source) {
            return Err(KeymapFileError::Conflict);
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.load(|current| {
                validate(current)?;
                if *current != expected.inner {
                    return Err(KeymapFileError::Conflict);
                }
                if !std::panic::catch_unwind(std::panic::AssertUnwindSafe(&authorize))
                    .unwrap_or(false)
                {
                    return Err(KeymapFileError::Authorization);
                }
                Ok(())
            })?;
            Ok(())
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = authorize;
            Err(KeymapFileError::Unsupported)
        }
    }

    /// Canonically save a typed action edit after instance and whole-file CAS, rechecking authorization
    /// after temp fsync and immediately before rename. This is not byte/comment-preserving editing.
    /// After rename, revocation cannot undo a commit: caller must reject stale UI receipts separately.
    /// The caller's blocking closure must retain IO credit until this function actually exits.
    pub fn save_authorized(
        &self,
        expected: &KeymapSnapshot,
        mutation: KeymapMutation,
        authorize: impl Fn() -> bool,
    ) -> Result<KeymapSnapshot> {
        let allowed =
            || std::panic::catch_unwind(std::panic::AssertUnwindSafe(&authorize)).unwrap_or(false);
        if !allowed() {
            return Err(KeymapFileError::Authorization);
        }
        if !Arc::ptr_eq(&self.source, &expected.source) {
            return Err(KeymapFileError::Conflict);
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let mut document = expected.document()?;
            match mutation {
                KeymapMutation::Set(entry) => {
                    document.restore_action(entry.action);
                    document.bindings.push(entry);
                }
                KeymapMutation::Restore(action) => document.restore_action(action),
            }
            document.resolve()?;
            let text = toml::to_string(&document).map_err(|_| KeymapError::Document)?;
            let inner = self.inner.save(&expected.inner, text, validate, |stage| {
                if matches!(stage, Stage::Installed) || allowed() {
                    Ok(())
                } else {
                    Err(KeymapFileError::Authorization)
                }
            })?;
            Ok(KeymapSnapshot {
                source: self.source.clone(),
                inner,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = mutation;
            Err(KeymapFileError::Unsupported)
        }
    }
}
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "keymap_file_tests.rs"]
mod tests;
