//! Synchronous single-reader reload state, ready for a bounded trusted event adapter.
//! No notify dependency, background tasks, directory selection or settings.changed IPC.
use crate::{
    settings::SettingsPatch,
    settings_file::{SettingsFile, SettingsFileError, SettingsSnapshot},
};
/// A delta of sparse fields, including explicit-default/false provenance changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsChangedFields {
    /// The user font size override changed.
    pub font_size: bool,
    /// The user font family override changed.
    pub font_family: bool,
    /// The user cursor blink override changed.
    pub cursor_blink: bool,
}
/// No document or comments travel in these application receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsReloadReceipt {
    /// No work pending or the sparse model is semantically unchanged.
    Unchanged,
    /// The validated sparse model changed. This is not a GUI event.
    Applied {
        /// Monotonic model revision, not a timestamp or file lease.
        revision: u64,
        /// Exactly the fields changed in the sparse user model.
        fields: SettingsChangedFields,
    },
}
/// Stable rejection retains previous values and the previous successful save snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingsReloadError {
    /// Underlying read/validation failed without resetting the file or cached model.
    #[error("settings.reload.rejected")]
    File(#[from] SettingsFileError),
    /// Explicit shutdown prevents any further read or publication.
    #[error("settings.reload.closed")]
    Closed,
    /// Counter exhaustion closes the cache rather than wrapping into an old revision.
    #[error("settings.reload.revision_exhausted")]
    Exhausted,
}
/// One mutable owner serializes reads. Repeated invalidations coalesce in one bool.
/// Initially the user layer is empty (compiled defaults), not a verified file snapshot.
/// Read failures consume the invalidation: no busy loop or automatic timeout retry;
/// a trusted subsequent signal/manual refresh must explicitly invalidate again.
pub struct SettingsReloadCache {
    file: SettingsFile,
    patch: SettingsPatch,
    snapshot: Option<SettingsSnapshot>,
    dirty: bool,
    closed: bool,
    revision: u64,
}
impl SettingsReloadCache {
    /// Takes ownership of an already admitted fixed private file target.
    pub fn new(file: SettingsFile) -> Self {
        Self {
            file,
            patch: SettingsPatch::default(),
            snapshot: None,
            dirty: true,
            closed: false,
            revision: 0,
        }
    }
    /// Mark dirty without queuing another job; an event adapter must handle its own ownership.
    pub fn invalidate(&mut self) -> Result<(), SettingsReloadError> {
        if self.closed {
            return Err(SettingsReloadError::Closed);
        }
        self.dirty = true;
        Ok(())
    }
    /// Last successfully validated user overlay, or the explicit empty bootstrap overlay.
    pub fn user_patch(&self) -> &SettingsPatch {
        &self.patch
    }
    /// Last successful source snapshot; not a permission to save or a live file lease.
    pub fn snapshot(&self) -> Option<&SettingsSnapshot> {
        self.snapshot.as_ref()
    }
    /// Current model revision; comment-only changes do not advance it.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Bounded synchronous read; callers must keep it off the GUI thread.
    pub fn refresh(&mut self) -> Result<SettingsReloadReceipt, SettingsReloadError> {
        if self.closed {
            return Err(SettingsReloadError::Closed);
        }
        if !self.dirty {
            return Ok(SettingsReloadReceipt::Unchanged);
        }
        self.dirty = false;
        let snapshot = self.file.load()?;
        let patch = SettingsPatch::parse(snapshot.text()).map_err(SettingsFileError::from)?;
        if patch == self.patch {
            self.snapshot = Some(snapshot);
            return Ok(SettingsReloadReceipt::Unchanged);
        }
        let Some(revision) = self.revision.checked_add(1) else {
            self.closed = true;
            return Err(SettingsReloadError::Exhausted);
        };
        let fields = SettingsChangedFields {
            font_size: patch.terminal.font_size != self.patch.terminal.font_size,
            font_family: patch.terminal.font_family != self.patch.terminal.font_family,
            cursor_blink: patch.terminal.cursor_blink != self.patch.terminal.cursor_blink,
        };
        self.patch = patch;
        self.snapshot = Some(snapshot);
        self.revision = revision;
        Ok(SettingsReloadReceipt::Applied { revision, fields })
    }
    /// Stops further reads/publications; it is not a registry/window authorization revocation.
    pub fn close(&mut self) {
        self.closed = true;
        self.dirty = false;
    }
}
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "settings_reload_tests.rs"]
mod tests;
