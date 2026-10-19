//! Bound regular-file leases; not a settings store or an atomic-replace API.
use super::{PrivateDirectory, sys};
use crate::policy;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

/// Fixed file errors; no native paths, account identifiers or contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PrivateFileError {
    /// Ownership, permissions, file kind or location is not admissible.
    #[error("platform.file.private_location_required")]
    Location,
    /// The requested existing file is absent.
    #[error("platform.file.not_found")]
    NotFound,
    /// Another handle prevents the requested exclusive lease.
    #[error("platform.file.busy")]
    Busy,
    /// A new file already exists or a read changed during observation.
    #[error("platform.file.conflict")]
    Conflict,
    /// The bounded byte budget was exceeded.
    #[error("platform.file.limit")]
    Limit,
    /// An operation failed before a known complete result.
    #[error("platform.file.io")]
    Io,
    /// Cleanup failed; reload the namespace instead of retrying creation.
    #[error("platform.file.uncertain")]
    Uncertain,
}
/// Result of a regular-file primitive, without application authorization.
pub type PrivateFileResult<T> = Result<T, PrivateFileError>;

fn open_error(e: std::io::Error) -> PrivateFileError {
    match e.raw_os_error() {
        Some(2 | 3) => PrivateFileError::NotFound,
        Some(32 | 33) => PrivateFileError::Busy,
        Some(80 | 183) => PrivateFileError::Conflict,
        Some(5) => PrivateFileError::Location,
        _ => PrivateFileError::Io,
    }
}

/// A pinned, bounded regular-file reader; the directory outlives every lease.
/// No Debug/serde/raw-handle access. Drop the lease before replacing the file.
pub struct PrivateFile<'a> {
    directory: &'a PrivateDirectory,
    file: File,
    identity: sys::Identity,
    discard_on_drop: bool,
}
impl Drop for PrivateFile<'_> {
    fn drop(&mut self) {
        if self.discard_on_drop {
            let _ = sys::discard_file(&self.file);
        }
    }
}

impl PrivateDirectory {
    /// Open one backend-selected basename without following links or allowing concurrent writes/deletes.
    /// The directory's private ACL does not exempt this file from independent owner/ACL/link checks.
    pub fn open_file(&self, name: &str) -> PrivateFileResult<PrivateFile<'_>> {
        self.file_name(name)?;
        self.validate().map_err(|_| PrivateFileError::Location)?;
        let path = self
            .chain
            .last()
            .ok_or(PrivateFileError::Location)?
            .path
            .join(name);
        let file = sys::open_regular(&path, None).map_err(|error| {
            if self.validate().is_err() {
                PrivateFileError::Location
            } else {
                open_error(error)
            }
        })?;
        let identity = sys::inspect(&file)
            .map_err(|_| PrivateFileError::Location)?
            .identity;
        let result = PrivateFile {
            directory: self,
            file,
            identity,
            discard_on_drop: false,
        };
        result.validate()?;
        Ok(result)
    }
    /// Exclusively create a new file with its protected ACL and exact bytes, then flush file data.
    /// Never overwrites existing files. This is not atomic replacement or a directory-durability receipt.
    pub fn create_file(
        &self,
        name: &str,
        data: &[u8],
        limit: usize,
    ) -> PrivateFileResult<PrivateFile<'_>> {
        self.file_name(name)?;
        if !policy::file_budget(data.len() as u64, limit) {
            return Err(PrivateFileError::Limit);
        }
        self.validate().map_err(|_| PrivateFileError::Location)?;
        let path = self
            .chain
            .last()
            .ok_or(PrivateFileError::Location)?
            .path
            .join(name);
        let file = sys::open_regular(&path, Some(&self.user)).map_err(|error| {
            if self.validate().is_err() {
                PrivateFileError::Location
            } else {
                open_error(error)
            }
        })?;
        // The creator owns DELETE access before any bytes are written; cleanup uses this exact handle.
        let identity = match sys::inspect(&file) {
            Ok(facts) => facts.identity,
            Err(_) => {
                return Err(if sys::discard_file(&file).is_ok() {
                    PrivateFileError::Location
                } else {
                    PrivateFileError::Uncertain
                });
            }
        };
        let mut result = PrivateFile {
            directory: self,
            file,
            identity,
            discard_on_drop: true,
        };
        let work = (|| {
            result.validate()?;
            result
                .file
                .write_all(data)
                .map_err(|_| PrivateFileError::Io)?;
            result.file.sync_all().map_err(|_| PrivateFileError::Io)?;
            let facts = result.validate()?;
            if facts.length != data.len() as u64 {
                return Err(PrivateFileError::Conflict);
            }
            Ok(())
        })();
        if let Err(error) = work {
            return Err(if sys::discard_file(&result.file).is_ok() {
                result.discard_on_drop = false;
                error
            } else {
                PrivateFileError::Uncertain
            });
        }
        result.discard_on_drop = false;
        Ok(result)
    }
    fn file_name(&self, name: &str) -> PrivateFileResult<()> {
        policy::component(&name.encode_utf16().take(256).collect::<Vec<_>>())
            .map_err(|_| PrivateFileError::Location)
    }
}
impl PrivateFile<'_> {
    fn validate(&self) -> PrivateFileResult<sys::Facts> {
        self.directory
            .validate()
            .map_err(|_| PrivateFileError::Location)?;
        let facts = sys::inspect(&self.file).map_err(|_| PrivateFileError::Location)?;
        if facts.identity != self.identity {
            return Err(PrivateFileError::Location);
        }
        policy::file_attributes(facts.attributes, facts.links)
            .map_err(|_| PrivateFileError::Location)?;
        policy::admit_acl(
            &facts.owner,
            &self.directory.user,
            facts.acl.as_deref(),
            facts.protected,
            true,
        )
        .map_err(|_| PrivateFileError::Location)?;
        Ok(facts)
    }
    /// Read from offset zero with a caller budget (at most 1MiB), rechecking identity and permissions.
    /// The lease prevents normal write/delete handles; this is not a cross-process compare-and-swap.
    pub fn read_bounded(&mut self, limit: usize) -> PrivateFileResult<Vec<u8>> {
        let before = self.validate()?;
        if !policy::file_budget(before.length, limit) {
            return Err(PrivateFileError::Limit);
        }
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| PrivateFileError::Io)?;
        let mut bytes = Vec::with_capacity(before.length as usize);
        Read::by_ref(&mut self.file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| PrivateFileError::Io)?;
        let after = self.validate()?;
        if !policy::file_budget(bytes.len() as u64, limit) {
            return Err(PrivateFileError::Limit);
        }
        if before.length != after.length
            || before.last_write != after.last_write
            || bytes.len() as u64 != after.length
        {
            return Err(PrivateFileError::Conflict);
        }
        Ok(bytes)
    }
}
