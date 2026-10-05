use super::*;
use crate::{
    settings::{SETTINGS_TEXT_LIMIT, SettingsPatch},
    settings_edit::set_terminal_setting,
    settings_remove::remove_terminal_setting,
};
use rustix::fs::{self as sys, AtFlags, Mode, OFlags, RenameFlags};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};
const TARGET: &str = "settings.toml";
const LOCK: &str = ".settings.lock";
const OPEN: OFlags = OFlags::CLOEXEC
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK);
// Explicit unlock matters even when fork/dup keeps the same open description alive.
struct LockGuard(File);
impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
fn id(m: &fs::Metadata) -> (u64, u64) {
    (m.dev(), m.ino())
}
fn stamp(m: &fs::Metadata) -> Stamp {
    Stamp {
        dev: m.dev(),
        ino: m.ino(),
        len: m.len(),
        mtime: m.mtime(),
        mtime_ns: m.mtime_nsec(),
        ctime: m.ctime(),
        ctime_ns: m.ctime_nsec(),
    }
}
fn private(m: &fs::Metadata, dir: bool) -> Result<()> {
    let correct = if dir {
        m.is_dir() && m.mode() & 0o7777 == 0o700
    } else {
        m.is_file() && m.mode() & 0o7777 == 0o600 && m.nlink() == 1
    };
    if !correct || m.uid() != rustix::process::geteuid().as_raw() {
        return Err(SettingsFileError::Location);
    }
    Ok(())
}
#[derive(Clone, Copy)]
pub(super) enum Stage {
    TempSynced,
    Validated,
    Installed,
}
pub(super) struct FileStore {
    dir: File,
    path: PathBuf,
    identity: (u64, u64),
}
impl FileStore {
    pub(super) fn open(path: PathBuf) -> Result<Self> {
        if !path.is_absolute() {
            return Err(SettingsFileError::Location);
        }
        let dir: File = sys::open(
            &path,
            OPEN | OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .map_err(|_| SettingsFileError::Location)?
        .into();
        let m = dir.metadata().map_err(|_| SettingsFileError::Location)?;
        private(&m, true)?;
        let result = Self {
            dir,
            path,
            identity: id(&m),
        };
        result.validate_directory()?;
        Ok(result)
    }
    fn validate_directory(&self) -> Result<()> {
        for m in [self.dir.metadata(), fs::symlink_metadata(&self.path)] {
            let m = m.map_err(|_| SettingsFileError::Location)?;
            private(&m, true)?;
            if id(&m) != self.identity {
                return Err(SettingsFileError::Location);
            }
        }
        Ok(())
    }
    fn named(&self, name: &str) -> Result<Option<File>> {
        match sys::openat(&self.dir, name, OPEN | OFlags::RDONLY, Mode::empty()) {
            Ok(fd) => {
                let file: File = fd.into();
                private(&file.metadata().map_err(|_| SettingsFileError::Io)?, false)?;
                Ok(Some(file))
            }
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(_) => Err(SettingsFileError::Location),
        }
    }
    fn lock(&self) -> Result<LockGuard> {
        self.validate_directory()?;
        let lock: File = sys::openat(
            &self.dir,
            LOCK,
            OPEN | OFlags::RDWR | OFlags::CREATE,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|_| SettingsFileError::Location)?
        .into();
        private(&lock.metadata().map_err(|_| SettingsFileError::Io)?, false)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(SettingsFileError::Busy),
            Err(_) => return Err(SettingsFileError::Io),
        }
        let guard = LockGuard(lock);
        self.validate_lock(&guard)?;
        Ok(guard)
    }
    fn validate_lock(&self, lock: &LockGuard) -> Result<()> {
        self.validate_directory()?;
        let current = self.named(LOCK)?.ok_or(SettingsFileError::Location)?;
        if id(&current.metadata().map_err(|_| SettingsFileError::Io)?)
            != id(&lock.0.metadata().map_err(|_| SettingsFileError::Io)?)
        {
            return Err(SettingsFileError::Location);
        }
        Ok(())
    }
    fn snapshot(&self) -> Result<SettingsSnapshot> {
        let Some(file) = self.named(TARGET)? else {
            return Ok(SettingsSnapshot {
                directory: self.identity,
                stamp: None,
                text: String::new(),
            });
        };
        let before = stamp(&file.metadata().map_err(|_| SettingsFileError::Io)?);
        if before.len > SETTINGS_TEXT_LIMIT as u64 {
            return Err(SettingsError::Size.into());
        }
        let mut text = String::new();
        (&file)
            .take(SETTINGS_TEXT_LIMIT as u64 + 1)
            .read_to_string(&mut text)
            .map_err(|_| SettingsFileError::Io)?;
        let after = file.metadata().map_err(|_| SettingsFileError::Io)?;
        private(&after, false)?;
        if before != stamp(&after) {
            return Err(SettingsFileError::Conflict);
        }
        SettingsPatch::parse(&text)?;
        Ok(SettingsSnapshot {
            directory: self.identity,
            stamp: Some(before),
            text,
        })
    }
    pub(super) fn load(&self) -> Result<SettingsSnapshot> {
        let lock = self.lock()?;
        let result = self.snapshot()?;
        self.validate_lock(&lock)?;
        Ok(result)
    }
    pub(super) fn save<F>(
        &self,
        expected: &SettingsSnapshot,
        change: SettingsMutation,
        hook: F,
    ) -> Result<SettingsSnapshot>
    where
        F: Fn(Stage) -> Result<()>,
    {
        if expected.directory != self.identity {
            return Err(SettingsFileError::Location);
        }
        let text = match change {
            SettingsMutation::Set(edit) => set_terminal_setting(&expected.text, edit)?,
            SettingsMutation::Remove(key) => remove_terminal_setting(&expected.text, key)?,
        };
        let lock = self.lock()?;
        if self.snapshot()? != *expected {
            return Err(SettingsFileError::Conflict);
        }
        if text == expected.text {
            return Ok(expected.clone());
        }
        let mut temp = Temp::create(self)?;
        temp.file
            .write_all(text.as_bytes())
            .map_err(|_| SettingsFileError::Io)?;
        temp.file.sync_all().map_err(|_| SettingsFileError::Io)?;
        hook(Stage::TempSynced)?;
        self.validate_lock(&lock)?;
        if self.snapshot()? != *expected {
            return Err(SettingsFileError::Conflict);
        }
        temp.validate()?;
        hook(Stage::Validated)?;
        let renamed = if expected.exists() {
            sys::renameat(&self.dir, &temp.name, &self.dir, TARGET)
        } else {
            sys::renameat_with(
                &self.dir,
                &temp.name,
                &self.dir,
                TARGET,
                RenameFlags::NOREPLACE,
            )
        };
        match renamed {
            Ok(()) => {}
            Err(rustix::io::Errno::EXIST) => return Err(SettingsFileError::Conflict),
            Err(_) => return Err(SettingsFileError::Io),
        }
        temp.installed = true;
        hook(Stage::Installed).map_err(|_| SettingsFileError::Uncertain)?;
        self.dir
            .sync_all()
            .map_err(|_| SettingsFileError::Uncertain)?;
        let metadata = temp
            .file
            .metadata()
            .map_err(|_| SettingsFileError::Uncertain)?;
        private(&metadata, false).map_err(|_| SettingsFileError::Uncertain)?;
        Ok(SettingsSnapshot {
            directory: self.identity,
            stamp: Some(stamp(&metadata)),
            text,
        })
    }
}
struct Temp<'a> {
    store: &'a FileStore,
    file: File,
    name: String,
    installed: bool,
}
impl<'a> Temp<'a> {
    fn create(store: &'a FileStore) -> Result<Self> {
        for _ in 0..3 {
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|_| SettingsFileError::Io)?;
            let value = u128::from_be_bytes(random);
            let name = format!(".settings-{value:032x}.tmp");
            match sys::openat(
                &store.dir,
                &name,
                OPEN | OFlags::RDWR | OFlags::CREATE | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => {
                    let result = Self {
                        store,
                        file: fd.into(),
                        name,
                        installed: false,
                    };
                    result.validate()?;
                    return Ok(result);
                }
                Err(rustix::io::Errno::EXIST) => continue,
                Err(_) => return Err(SettingsFileError::Io),
            }
        }
        Err(SettingsFileError::Io)
    }
    fn validate(&self) -> Result<()> {
        let own = self.file.metadata().map_err(|_| SettingsFileError::Io)?;
        private(&own, false)?;
        let named = self
            .store
            .named(&self.name)?
            .ok_or(SettingsFileError::Location)?;
        if id(&own) != id(&named.metadata().map_err(|_| SettingsFileError::Io)?) {
            return Err(SettingsFileError::Location);
        }
        Ok(())
    }
}
impl Drop for Temp<'_> {
    fn drop(&mut self) {
        if !self.installed && self.validate().is_ok() {
            let _ = sys::unlinkat(&self.store.dir, &self.name, AtFlags::empty());
        }
    }
}
#[cfg(test)]
#[path = "settings_file_process_tests.rs"]
mod process_tests;
#[cfg(test)]
#[path = "settings_file_tests.rs"]
mod tests;
