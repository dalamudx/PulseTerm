use super::*;
pub(super) use crate::private_file::Stage;
#[cfg(test)]
use crate::settings::SETTINGS_TEXT_LIMIT;
use crate::{
    private_file::{FileIoError, FileSlot, FileStore as Core, PrivateSnapshot},
    settings::{SettingsError, SettingsPatch},
    settings_edit::set_terminal_setting,
    settings_remove::remove_terminal_setting,
};
#[cfg(test)]
use rustix::fs::{self as sys, Mode, OFlags};
#[cfg(test)]
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};
#[cfg(test)]
const TARGET: &str = "settings.toml";
#[cfg(test)]
const LOCK: &str = ".settings.lock";
#[cfg(test)]
const OPEN: OFlags = OFlags::CLOEXEC
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK);
impl From<FileIoError> for SettingsFileError {
    fn from(e: FileIoError) -> Self {
        match e {
            FileIoError::Location => Self::Location,
            FileIoError::Busy => Self::Busy,
            FileIoError::Conflict => Self::Conflict,
            FileIoError::Io => Self::Io,
            FileIoError::Uncertain => Self::Uncertain,
            FileIoError::Limit => Self::Document(SettingsError::Size),
        }
    }
}
fn validate(s: &PrivateSnapshot) -> Result<()> {
    SettingsPatch::parse(s.text())
        .map(|_| ())
        .map_err(Into::into)
}
pub(super) struct FileStore {
    core: Core,
    #[cfg(test)]
    dir: File,
}
impl FileStore {
    pub(super) fn open(path: PathBuf) -> Result<Self> {
        let core = Core::open(path, FileSlot::Settings)?;
        #[cfg(test)]
        let dir = core.dir.try_clone().map_err(|_| SettingsFileError::Io)?;
        Ok(Self {
            core,
            #[cfg(test)]
            dir,
        })
    }
    pub(super) fn load(&self) -> Result<SettingsSnapshot> {
        self.core.load(validate).map(SettingsSnapshot)
    }
    pub(super) fn save<F: Fn(Stage) -> Result<()>>(
        &self,
        expected: &SettingsSnapshot,
        change: SettingsMutation,
        hook: F,
    ) -> Result<SettingsSnapshot> {
        let text = match change {
            SettingsMutation::Set(e) => set_terminal_setting(expected.text(), e)?,
            SettingsMutation::Remove(k) => remove_terminal_setting(expected.text(), k)?,
        };
        self.core
            .save(&expected.0, text, validate, hook)
            .map(SettingsSnapshot)
    }
    #[cfg(test)]
    fn lock(&self) -> Result<crate::private_file::unix::LockGuard> {
        self.core.lock().map_err(Into::into)
    }
}
#[cfg(test)]
#[path = "settings_file_process_tests.rs"]
mod process_tests;
#[cfg(test)]
#[path = "settings_file_tests.rs"]
mod tests;
