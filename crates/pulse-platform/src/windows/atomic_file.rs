//! Cooperating-process exact-snapshot replacement; not application authorization or crash durability.
use super::{
    PrivateDirectory, PrivateFile, PrivateFileError as Error, PrivateFileResult as Result,
    open_error, sys,
};
use crate::{
    file_state::{FileSnapshot, authorized},
    policy,
};

const LOCK: &str = ".pulse-write.lock";
const TEMP_PREFIX: &str = ".pulse-atomic-";

/// One backend-selected bounded file slot, borrowing its pinned private directory.
/// A fixed directory-wide lock serializes cooperating stores. Same-account external edits
/// after the final snapshot check are not an atomic compare-and-rename transaction.
pub struct AtomicFile<'a> {
    directory: &'a PrivateDirectory,
    name: String,
    limit: usize,
}
impl PrivateDirectory {
    /// Select one native file slot. No read, execution or page authorization is granted.
    /// Internal lock/staging names are reserved; data budgets cannot exceed 1MiB.
    pub fn atomic_file(&self, name: &str, limit: usize) -> Result<AtomicFile<'_>> {
        self.file_name(name)?;
        if name.eq_ignore_ascii_case(LOCK) || name.to_ascii_lowercase().starts_with(TEMP_PREFIX) {
            return Err(Error::Location);
        }
        if !policy::file_budget(0, limit) {
            return Err(Error::Limit);
        }
        Ok(AtomicFile {
            directory: self,
            name: name.into(),
            limit,
        })
    }
}
impl<'a> AtomicFile<'a> {
    fn lock(&self) -> Result<PrivateFile<'a>> {
        let guard = match self.directory.create_file(LOCK, &[], 0) {
            Ok(guard) => guard,
            Err(Error::Conflict) => {
                let path = self
                    .directory
                    .chain
                    .last()
                    .ok_or(Error::Location)?
                    .path
                    .join(LOCK);
                let file = sys::open_lock(&path).map_err(|error| {
                    if self.directory.validate().is_err() {
                        Error::Location
                    } else {
                        open_error(error)
                    }
                })?;
                let identity = sys::inspect(&file).map_err(|_| Error::Location)?.identity;
                PrivateFile {
                    directory: self.directory,
                    file,
                    identity,
                    discard_on_drop: false,
                }
            }
            Err(error) => return Err(error),
        };
        self.validate_lock(&guard)?;
        Ok(guard)
    }
    fn validate_lock(&self, lock: &PrivateFile<'_>) -> Result<()> {
        if lock.validate()?.length != 0 {
            return Err(Error::Location);
        }
        Ok(())
    }
    fn capture(&self) -> Result<FileSnapshot> {
        self.directory.validate().map_err(|_| Error::Location)?;
        let (stamp, bytes) = match self.directory.open_file(&self.name) {
            Ok(mut file) => {
                let bytes = file.read_bounded(self.limit)?;
                (Some(file.validate()?.stamp()), bytes)
            }
            Err(Error::NotFound) => (None, Vec::new()),
            Err(error) => return Err(error),
        };
        Ok(FileSnapshot {
            directory: self.directory.chain.last().ok_or(Error::Location)?.identity,
            name: self.name.clone(),
            limit: self.limit,
            stamp,
            bytes,
        })
    }
    /// Capture exact bytes, presence and identity under the cooperative lock.
    pub fn load(&self) -> Result<FileSnapshot> {
        let lock = self.lock()?;
        let snapshot = self.capture()?;
        self.validate_lock(&lock)?;
        Ok(snapshot)
    }
    fn stage(&self, data: &[u8]) -> Result<PrivateFile<'a>> {
        for _ in 0..3 {
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|_| Error::Io)?;
            let name = format!("{TEMP_PREFIX}{:032x}.tmp", u128::from_be_bytes(random));
            match self.directory.create_file(&name, data, self.limit) {
                Ok(mut file) => {
                    file.discard_on_drop = true;
                    return Ok(file);
                }
                Err(Error::Conflict) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(Error::Busy)
    }
    fn abort(mut file: PrivateFile<'_>, error: Error) -> Result<FileSnapshot> {
        if sys::discard_file(&file.file).is_ok() {
            file.discard_on_drop = false;
            Err(error)
        } else {
            Err(Error::Uncertain)
        }
    }
    /// Save exact bytes only if the original snapshot still matches under the cooperative lock.
    /// Authorization is checked before I/O, after staging and immediately before the native rename.
    /// Callback panic rejects. Revocation after the final grant cannot undo an executing native call;
    /// upper layers must retain their I/O reservation and reject stale publication receipts.
    /// Once rename is attempted, ambiguous failure is Uncertain: preserve the file and reload;
    /// never automatically retry or delete a file that may already have become the published target.
    /// Success proves namespace installation and file-data flush, not directory crash durability.
    pub fn save(
        &self,
        expected: &FileSnapshot,
        data: &[u8],
        authorize: impl Fn() -> bool,
    ) -> Result<FileSnapshot> {
        let directory = self.directory.chain.last().ok_or(Error::Location)?.identity;
        if !expected.belongs_to(directory, &self.name, self.limit) {
            return Err(Error::Location);
        }
        if !policy::file_budget(data.len() as u64, self.limit) {
            return Err(Error::Limit);
        }
        if !authorized(&authorize) {
            return Err(Error::Revoked);
        }
        let lock = self.lock()?;
        let current = self.capture()?;
        if &current != expected {
            return Err(Error::Conflict);
        }
        self.validate_lock(&lock)?;
        if expected.exists() && data == expected.bytes() {
            if !authorized(&authorize) {
                return Err(Error::Revoked);
            }
            self.validate_lock(&lock)?;
            return Ok(current);
        }
        let mut staged = self.stage(data)?;
        let prepare = (|| {
            if !authorized(&authorize) {
                return Err(Error::Revoked);
            }
            if self.capture()? != *expected {
                return Err(Error::Conflict);
            }
            self.validate_lock(&lock)?;
            staged.validate()?;
            let path = self
                .directory
                .chain
                .last()
                .ok_or(Error::Location)?
                .path
                .join(&self.name);
            let request =
                sys::prepare_rename(&path, expected.exists()).map_err(|_| Error::Location)?;
            if !authorized(&authorize) {
                return Err(Error::Revoked);
            }
            self.validate_lock(&lock)?;
            Ok(request)
        })();
        let request = match prepare {
            Ok(request) => request,
            Err(error) => return Self::abort(staged, error),
        };
        // The commit may already have installed this exact source even if its receipt is uncertain.
        staged.discard_on_drop = false;
        sys::rename_file(&staged.file, &request).map_err(|_| Error::Uncertain)?;
        staged.file.sync_all().map_err(|_| Error::Uncertain)?;
        let installed = staged.validate().map_err(|_| Error::Uncertain)?.identity;
        drop(staged);
        let snapshot = self.capture().map_err(|_| Error::Uncertain)?;
        self.validate_lock(&lock).map_err(|_| Error::Uncertain)?;
        if snapshot
            .stamp
            .as_ref()
            .is_none_or(|stamp| stamp.identity != installed)
            || snapshot.bytes() != data
        {
            return Err(Error::Uncertain);
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
#[path = "atomic_file_tests.rs"]
mod tests;
