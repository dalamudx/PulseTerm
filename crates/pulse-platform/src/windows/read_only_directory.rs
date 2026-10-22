//! Synchronous, bounded read-only source trees; callers retain native I/O credit through exit.
use super::{PrivateDirectory, PrivateFileError, PrivateFileResult, sys};
use crate::{file_state::FileStamp, policy};
use std::{path::Path, sync::Arc};

/// Exact native identity and mutation metadata, without file bytes, Debug or serialization.
#[derive(Clone, PartialEq, Eq)]
pub struct ReadOnlyStamp(FileStamp);
impl ReadOnlyStamp {
    /// Compare only volume/file identity when reopening an originally selected directory.
    /// Equality of identity does not establish permissions, freshness or application authority.
    pub fn same_identity(&self, other: &Self) -> bool {
        self.0.identity == other.0.identity
    }
}
/// An admitted private source directory, with its original ancestors pinned.
/// This interface never creates entries, changes permissions or grants page/credential authority.
#[derive(Clone)]
pub struct ReadOnlyDirectory(Arc<PrivateDirectory>);
impl ReadOnlyDirectory {
    /// Open an existing local private directory. Reparse points and unsafe ACLs are rejected.
    pub fn open(path: &Path) -> PrivateFileResult<Self> {
        Ok(Self(Arc::new(
            PrivateDirectory::open(path).map_err(|_| PrivateFileError::Location)?,
        )))
    }
    /// Revalidate the pinned path, owner and ACL before capturing native metadata.
    pub fn stamp(&self) -> PrivateFileResult<ReadOnlyStamp> {
        self.0.validate().map_err(|_| PrivateFileError::Location)?;
        let binding = self.0.chain.last().ok_or(PrivateFileError::Location)?;
        Ok(ReadOnlyStamp(
            sys::inspect(&binding.file)
                .map_err(|_| PrivateFileError::Location)?
                .stamp(),
        ))
    }
    /// Open one existing child component, retaining the original admitted ancestors.
    pub fn child(&self, name: &str) -> PrivateFileResult<Self> {
        policy::component(&name.encode_utf16().take(256).collect::<Vec<_>>())
            .map_err(|_| PrivateFileError::Location)?;
        self.0.validate().map_err(|_| PrivateFileError::Location)?;
        let binding = self.0.chain.last().ok_or(PrivateFileError::Location)?;
        let result = Self::open(&binding.path.join(name))?;
        self.0.validate().map_err(|_| PrivateFileError::Location)?;
        Ok(result)
    }
    /// Enumerate names only; no entry contents, types or identity-file targets are opened.
    /// Budgets are enforced while iterating, with absolute maxima of 4096 names and 512KiB.
    /// Names not representable as UTF-8 fail explicitly. This is not an atomic directory snapshot.
    pub fn members(
        &self,
        entry_limit: usize,
        byte_limit: usize,
    ) -> PrivateFileResult<Vec<Vec<u8>>> {
        if entry_limit > 4096 || byte_limit > 512 * 1024 {
            return Err(PrivateFileError::Limit);
        }
        self.0.validate().map_err(|_| PrivateFileError::Location)?;
        let binding = self.0.chain.last().ok_or(PrivateFileError::Location)?;
        let entries = std::fs::read_dir(&binding.path).map_err(|_| PrivateFileError::Io)?;
        let mut names = Vec::new();
        let mut bytes = 0usize;
        for entry in entries {
            let entry = entry.map_err(|_| PrivateFileError::Io)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(PrivateFileError::Location)?.as_bytes();
            bytes = bytes
                .checked_add(name.len())
                .ok_or(PrivateFileError::Limit)?;
            if names.len() >= entry_limit || bytes > byte_limit {
                return Err(PrivateFileError::Limit);
            }
            names.push(name.to_vec());
        }
        self.0.validate().map_err(|_| PrivateFileError::Location)?;
        Ok(names)
    }
    /// Read one backend-selected basename through the existing single-link, private-file lease.
    /// The lease denies concurrent normal write/delete handles and validates the stable read.
    pub fn source(&self, name: &str, limit: usize) -> PrivateFileResult<(Vec<u8>, ReadOnlyStamp)> {
        let mut file = self.0.open_file(name)?;
        let bytes = file.read_bounded(limit)?;
        let stamp = ReadOnlyStamp(file.validate()?.stamp());
        Ok((bytes, stamp))
    }
}
