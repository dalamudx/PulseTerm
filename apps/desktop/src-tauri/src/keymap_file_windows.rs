//! Windows keymap IO uses the same pinned directory and exact-snapshot replacement as settings.
#[cfg(test)]
use pulse_store::keymap::{KeymapAction, KeymapOverride};
use pulse_store::{
    keymap::{KeymapError, KeymapPatch, KeymapProjection},
    keymap_file::{KeymapFileError, KeymapMutation},
    keymap_io::KeymapBackend,
};
use std::{path::PathBuf, sync::Arc};
type Result<T> = std::result::Result<T, KeymapFileError>;
use pulse_platform::windows::{FileSnapshot, PrivateDirectory, PrivateFileError};
use pulse_store::keymap::KEYMAP_TEXT_LIMIT;

fn platform(error: PrivateFileError) -> KeymapFileError {
    match error {
        PrivateFileError::Location => KeymapFileError::Location,
        PrivateFileError::Revoked => KeymapFileError::Authorization,
        PrivateFileError::NotFound | PrivateFileError::Io => KeymapFileError::Io,
        PrivateFileError::Busy => KeymapFileError::Busy,
        PrivateFileError::Conflict => KeymapFileError::Conflict,
        PrivateFileError::Limit => KeymapError::Size.into(),
        PrivateFileError::Uncertain => KeymapFileError::Uncertain,
    }
}

/// Exact bounded image bound to its original store instance; never Debug or serialize comments.
#[derive(Clone)]
pub struct KeymapSnapshot {
    source: Arc<()>,
    inner: FileSnapshot,
}
impl KeymapSnapshot {
    /// Missing inherits built-in shortcuts; present empty or malformed files fail closed.
    pub fn document(&self) -> Result<KeymapPatch> {
        if self.inner.exists() {
            let text =
                std::str::from_utf8(self.inner.bytes()).map_err(|_| KeymapError::Document)?;
            Ok(KeymapPatch::parse(text)?)
        } else {
            Ok(KeymapPatch {
                schema_version: 1,
                bindings: Vec::new(),
            })
        }
    }
    /// Resolve this image only; this grants no execution or terminal authority.
    pub fn projection(&self) -> Result<KeymapProjection> {
        Ok(self.document()?.resolve()?)
    }
}

/// Fixed-name Windows keymap store. Opening never creates a directory or repairs existing ACLs.
pub struct KeymapFile {
    source: Arc<()>,
    directory: PrivateDirectory,
}
impl KeymapFile {
    /// Pin an already-admitted private directory for the supervised owner's lifetime.
    pub fn open_private(path: PathBuf) -> Result<Self> {
        Ok(Self {
            source: Arc::new(()),
            directory: PrivateDirectory::open(path).map_err(|_| KeymapFileError::Location)?,
        })
    }
    fn snapshot(&self, inner: FileSnapshot) -> Result<KeymapSnapshot> {
        let snapshot = KeymapSnapshot {
            source: self.source.clone(),
            inner,
        };
        snapshot.document()?;
        Ok(snapshot)
    }
    /// Read bounded exact bytes under the cooperating lock and validate the complete document.
    pub fn load(&self) -> Result<KeymapSnapshot> {
        let slot = self
            .directory
            .atomic_file("keymap.toml", KEYMAP_TEXT_LIMIT)
            .map_err(platform)?;
        self.snapshot(slot.load().map_err(platform)?)
    }
    /// Check the original instance, exact identity/bytes, and current caller authority under lock.
    pub fn confirm_current_authorized(
        &self,
        expected: &KeymapSnapshot,
        authorize: impl Fn() -> bool,
    ) -> Result<()> {
        if !Arc::ptr_eq(&self.source, &expected.source) {
            return Err(KeymapFileError::Conflict);
        }
        self.directory
            .atomic_file("keymap.toml", KEYMAP_TEXT_LIMIT)
            .map_err(platform)?
            .confirm_current_authorized(&expected.inner, authorize)
            .map_err(platform)
    }
    /// Apply one typed edit with whole-file CAS and repeated current-context authorization.
    /// The caller retains physical IO credit until return; uncertainty requires reload, never replay.
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
        let mut document = expected.document()?;
        match mutation {
            KeymapMutation::Set(entry) => {
                document.restore_action(entry.action);
                document.bindings.push(entry);
            }
            KeymapMutation::Restore(action) => document.restore_action(action),
        }
        document.resolve()?;
        let text = document.to_toml()?;
        let slot = self
            .directory
            .atomic_file("keymap.toml", KEYMAP_TEXT_LIMIT)
            .map_err(platform)?;
        let installed = slot
            .save(&expected.inner, text.as_bytes(), allowed)
            .map_err(platform)?;
        self.snapshot(installed)
            .map_err(|_| KeymapFileError::Uncertain)
    }
}

#[cfg(test)]
#[path = "keymap_file_windows_tests.rs"]
mod tests;

impl KeymapBackend for KeymapFile {
    type Snapshot = KeymapSnapshot;
    fn load(&self) -> Result<KeymapSnapshot> {
        self.load()
    }
    fn confirm_current_authorized(
        &self,
        expected: &KeymapSnapshot,
        authorize: impl Fn() -> bool,
    ) -> Result<()> {
        self.confirm_current_authorized(expected, authorize)
    }
    fn save_authorized(
        &self,
        expected: &KeymapSnapshot,
        mutation: KeymapMutation,
        authorize: impl Fn() -> bool,
    ) -> Result<KeymapSnapshot> {
        self.save_authorized(expected, mutation, authorize)
    }
}
