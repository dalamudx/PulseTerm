//! Descriptor-relative implementation of the shared, synchronous config source interface.
use super::*;
use crate::config::file::{directory, private_directory, regular, stamp};
use rustix::fs::{self as sys, Mode, OFlags};
use std::{
    fs::{File, Metadata},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
const OPEN: OFlags = OFlags::CLOEXEC
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::RDONLY);
/// Backend-only Unix directory identity; fields cannot be selected by page data.
pub struct UnixConfigFilesystem {
    path: PathBuf,
    identity: (u64, u64),
}
/// One admitted descriptor, including no-follow traversal for every nested directory.
pub struct UnixConfigDirectory(File);
impl SourceDirectoryReader<UnixConfigFilesystem> {
    /// Admit the private directory through no-follow descriptors; do not create or repair it.
    pub fn open(path: &Path) -> Result<Self, ConfigError> {
        let opened = directory(path)?;
        let stat = metadata(&opened)?;
        Ok(Self::from_filesystem(UnixConfigFilesystem {
            path: path.to_owned(),
            identity: (stat.dev(), stat.ino()),
        }))
    }
}
impl ConfigSourceFilesystem for UnixConfigFilesystem {
    type Directory = UnixConfigDirectory;
    fn open(&self) -> Result<Self::Directory, ConfigError> {
        let opened = directory(&self.path)?;
        let stat = metadata(&opened)?;
        if (stat.dev(), stat.ino()) != self.identity {
            return Err(ConfigError::SourceChanged);
        }
        Ok(UnixConfigDirectory(opened))
    }
}
fn metadata(file: &File) -> Result<Metadata, ConfigError> {
    file.metadata().map_err(|_| ConfigError::SourceUnavailable)
}
fn component(name: &str) -> Result<(), ConfigError> {
    valid_name(name)?;
    if name.contains('/') {
        return Err(ConfigError::Syntax);
    }
    Ok(())
}
impl ConfigSourceDirectory for UnixConfigDirectory {
    type Stamp = (u64, u64, u64, i64, i64, i64, i64);
    fn stamp(&self) -> Result<Self::Stamp, ConfigError> {
        let metadata = metadata(&self.0)?;
        if !private_directory(&metadata) {
            return Err(ConfigError::SourceUnavailable);
        }
        Ok(stamp(&metadata))
    }
    fn duplicate(&self) -> Result<Self, ConfigError> {
        Ok(Self(
            self.0
                .try_clone()
                .map_err(|_| ConfigError::SourceUnavailable)?,
        ))
    }
    fn child(&self, name: &str) -> Result<Self, ConfigError> {
        component(name)?;
        let opened: File = sys::openat(&self.0, name, OPEN | OFlags::DIRECTORY, Mode::empty())
            .map_err(|_| ConfigError::SourceUnavailable)?
            .into();
        if !private_directory(&metadata(&opened)?) {
            return Err(ConfigError::SourceUnavailable);
        }
        Ok(Self(opened))
    }
    fn members(
        &self,
        entries_limit: usize,
        bytes_limit: usize,
    ) -> Result<Vec<Vec<u8>>, ConfigError> {
        self.stamp()?;
        let mut entries =
            sys::Dir::read_from(&self.0).map_err(|_| ConfigError::SourceUnavailable)?;
        let mut names = Vec::new();
        let mut bytes = 0usize;
        while let Some(entry) = entries.read() {
            let entry = entry.map_err(|_| ConfigError::SourceUnavailable)?;
            let value = entry.file_name().to_bytes();
            if value == b"." || value == b".." {
                continue;
            }
            bytes = bytes.checked_add(value.len()).ok_or(ConfigError::Limit)?;
            if names.len() >= entries_limit || bytes > bytes_limit {
                return Err(ConfigError::Limit);
            }
            names.push(value.to_vec());
        }
        Ok(names)
    }
    fn source(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<ConfigSourceFile<Self::Stamp>, ConfigError> {
        component(name)?;
        let file: File = sys::openat(&self.0, name, OPEN, Mode::empty())
            .map_err(|_| ConfigError::SourceUnavailable)?
            .into();
        let before = metadata(&file)?;
        if !regular(&before) {
            return Err(ConfigError::SourceUnavailable);
        }
        let limit = limit.min(MAX_SOURCE);
        if before.len() > limit as u64 {
            return Err(ConfigError::Limit);
        }
        let mut bytes = Vec::new();
        (&file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ConfigError::SourceUnavailable)?;
        if bytes.len() > limit {
            return Err(ConfigError::Limit);
        }
        let after = metadata(&file)?;
        if !regular(&after) || stamp(&before) != stamp(&after) {
            return Err(ConfigError::SourceChanged);
        }
        Ok(ConfigSourceFile {
            bytes,
            stamp: stamp(&before),
        })
    }
}
