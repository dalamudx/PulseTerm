//! Windows filesystem adapter for shared SSH config capture. No GUI or execution authority.
use pulse_platform::windows::{PrivateFileError, ReadOnlyDirectory, ReadOnlyStamp};
use pulse_ssh::config::{
    ConfigError, ConfigSourceDirectory, ConfigSourceFile, ConfigSourceFilesystem,
    SourceDirectoryReader, SourceFileSnapshot,
};
use std::path::{Path, PathBuf};

pub(crate) struct WindowsConfigFilesystem {
    path: PathBuf,
    identity: ReadOnlyStamp,
}
pub(crate) struct WindowsConfigDirectory(ReadOnlyDirectory);
pub(crate) type ConfigDirectoryReader = SourceDirectoryReader<WindowsConfigFilesystem>;
pub(crate) type ConfigFileSnapshot = SourceFileSnapshot<WindowsConfigFilesystem>;
fn error(value: PrivateFileError) -> ConfigError {
    match value {
        PrivateFileError::Limit => ConfigError::Limit,
        PrivateFileError::Conflict => ConfigError::SourceChanged,
        _ => ConfigError::SourceUnavailable,
    }
}
pub(crate) fn open_reader(path: &Path) -> Result<ConfigDirectoryReader, ConfigError> {
    let directory = ReadOnlyDirectory::open(path).map_err(error)?;
    let identity = directory.stamp().map_err(error)?;
    Ok(SourceDirectoryReader::from_filesystem(
        WindowsConfigFilesystem {
            path: path.to_owned(),
            identity,
        },
    ))
}
impl ConfigSourceFilesystem for WindowsConfigFilesystem {
    type Directory = WindowsConfigDirectory;
    fn open(&self) -> Result<Self::Directory, ConfigError> {
        let directory = ReadOnlyDirectory::open(&self.path).map_err(error)?;
        if !directory
            .stamp()
            .map_err(error)?
            .same_identity(&self.identity)
        {
            return Err(ConfigError::SourceChanged);
        }
        Ok(WindowsConfigDirectory(directory))
    }
}
impl ConfigSourceDirectory for WindowsConfigDirectory {
    type Stamp = ReadOnlyStamp;
    fn stamp(&self) -> Result<Self::Stamp, ConfigError> {
        self.0.stamp().map_err(error)
    }
    fn duplicate(&self) -> Result<Self, ConfigError> {
        Ok(Self(self.0.clone()))
    }
    fn child(&self, name: &str) -> Result<Self, ConfigError> {
        self.0.child(name).map(Self).map_err(error)
    }
    fn members(&self, entries: usize, bytes: usize) -> Result<Vec<Vec<u8>>, ConfigError> {
        self.0.members(entries, bytes).map_err(error)
    }
    fn source(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<ConfigSourceFile<Self::Stamp>, ConfigError> {
        let (bytes, stamp) = self.0.source(name, limit).map_err(error)?;
        Ok(ConfigSourceFile { bytes, stamp })
    }
}
#[cfg(test)]
#[path = "native_ssh_config_windows_tests.rs"]
mod tests;
