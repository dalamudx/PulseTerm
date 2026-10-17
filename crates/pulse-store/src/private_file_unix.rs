use super::*;
use rustix::fs::{self as sys, AtFlags, Mode, OFlags, RenameFlags};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};
const OPEN: OFlags = OFlags::CLOEXEC
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK);
// Explicit unlock matters even when fork/dup keeps the same open description alive.
pub(crate) struct LockGuard(pub(crate) File);
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
        return Err(FileIoError::Location);
    }
    Ok(())
}
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    TempSynced,
    Validated,
    Installed,
}
pub(crate) struct FileStore {
    pub(crate) dir: File,
    path: PathBuf,
    identity: (u64, u64),
    slot: FileSlot,
    target: String,
    lock_name: &'static str,
}
impl FileStore {
    pub(crate) fn open(path: PathBuf, slot: FileSlot) -> Result<Self> {
        if !path.is_absolute() {
            return Err(FileIoError::Location);
        }
        let dir: File = sys::open(
            &path,
            OPEN | OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .map_err(|_| FileIoError::Location)?
        .into();
        let m = dir.metadata().map_err(|_| FileIoError::Location)?;
        private(&m, true)?;
        let result = Self {
            dir,
            path,
            identity: id(&m),
            slot,
            target: slot.target(),
            lock_name: slot.lock(),
        };
        result.validate_directory()?;
        Ok(result)
    }
    fn validate_directory(&self) -> Result<()> {
        for m in [self.dir.metadata(), fs::symlink_metadata(&self.path)] {
            let m = m.map_err(|_| FileIoError::Location)?;
            private(&m, true)?;
            if id(&m) != self.identity {
                return Err(FileIoError::Location);
            }
        }
        Ok(())
    }
    fn named(&self, name: &str) -> Result<Option<File>> {
        match sys::openat(&self.dir, name, OPEN | OFlags::RDONLY, Mode::empty()) {
            Ok(fd) => {
                let file: File = fd.into();
                private(&file.metadata().map_err(|_| FileIoError::Io)?, false)?;
                Ok(Some(file))
            }
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(_) => Err(FileIoError::Location),
        }
    }
    pub(crate) fn lock(&self) -> Result<LockGuard> {
        self.validate_directory()?;
        let lock: File = sys::openat(
            &self.dir,
            self.lock_name,
            OPEN | OFlags::RDWR | OFlags::CREATE,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|_| FileIoError::Location)?
        .into();
        private(&lock.metadata().map_err(|_| FileIoError::Io)?, false)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(FileIoError::Busy),
            Err(_) => return Err(FileIoError::Io),
        }
        let guard = LockGuard(lock);
        self.validate_lock(&guard)?;
        Ok(guard)
    }
    fn validate_lock(&self, lock: &LockGuard) -> Result<()> {
        self.validate_directory()?;
        let current = self.named(self.lock_name)?.ok_or(FileIoError::Location)?;
        if id(&current.metadata().map_err(|_| FileIoError::Io)?)
            != id(&lock.0.metadata().map_err(|_| FileIoError::Io)?)
        {
            return Err(FileIoError::Location);
        }
        Ok(())
    }
    fn snapshot(&self) -> Result<PrivateSnapshot> {
        let Some(file) = self.named(&self.target)? else {
            return Ok(PrivateSnapshot {
                directory: self.identity,
                slot: self.slot,
                stamp: None,
                text: String::new(),
            });
        };
        let before = stamp(&file.metadata().map_err(|_| FileIoError::Io)?);
        if before.len > self.slot.limit() as u64 {
            return Err(FileIoError::Limit);
        }
        let mut text = String::new();
        (&file)
            .take(self.slot.limit() as u64 + 1)
            .read_to_string(&mut text)
            .map_err(|_| FileIoError::Io)?;
        let after = file.metadata().map_err(|_| FileIoError::Io)?;
        private(&after, false)?;
        if before != stamp(&after) {
            return Err(FileIoError::Conflict);
        }
        Ok(PrivateSnapshot {
            directory: self.identity,
            slot: self.slot,
            stamp: Some(before),
            text,
        })
    }
    pub(crate) fn load<E: From<FileIoError>>(
        &self,
        validate: impl Fn(&PrivateSnapshot) -> std::result::Result<(), E>,
    ) -> std::result::Result<PrivateSnapshot, E> {
        let lock = self.lock()?;
        let result = self.snapshot()?;
        validate(&result)?;
        self.validate_lock(&lock)?;
        Ok(result)
    }
    pub(crate) fn save<E, V, F>(
        &self,
        expected: &PrivateSnapshot,
        text: String,
        validate: V,
        hook: F,
    ) -> std::result::Result<PrivateSnapshot, E>
    where
        E: From<FileIoError>,
        V: Fn(&PrivateSnapshot) -> std::result::Result<(), E>,
        F: Fn(Stage) -> std::result::Result<(), E>,
    {
        if expected.directory != self.identity || expected.slot != self.slot {
            return Err(FileIoError::Location.into());
        }
        if text.len() > self.slot.limit() {
            return Err(FileIoError::Limit.into());
        }
        let lock = self.lock()?;
        let current = self.snapshot()?;
        validate(&current)?;
        if current != *expected {
            return Err(FileIoError::Conflict.into());
        }
        self.validate_lock(&lock)?;
        if text == expected.text {
            return Ok(expected.clone());
        }
        let mut temp = Temp::create(self)?;
        temp.file
            .write_all(text.as_bytes())
            .map_err(|_| FileIoError::Io)?;
        temp.file.sync_all().map_err(|_| FileIoError::Io)?;
        hook(Stage::TempSynced)?;
        self.validate_lock(&lock)?;
        let current = self.snapshot()?;
        validate(&current)?;
        if current != *expected {
            return Err(FileIoError::Conflict.into());
        }
        temp.validate()?;
        hook(Stage::Validated)?;
        let renamed = if expected.exists() {
            sys::renameat(&self.dir, &temp.name, &self.dir, &self.target)
        } else {
            sys::renameat_with(
                &self.dir,
                &temp.name,
                &self.dir,
                &self.target,
                RenameFlags::NOREPLACE,
            )
        };
        match renamed {
            Ok(()) => {}
            Err(rustix::io::Errno::EXIST) => return Err(FileIoError::Conflict.into()),
            Err(_) => return Err(FileIoError::Io.into()),
        }
        temp.installed = true;
        hook(Stage::Installed).map_err(|_| E::from(FileIoError::Uncertain))?;
        self.dir.sync_all().map_err(|_| FileIoError::Uncertain)?;
        let metadata = temp.file.metadata().map_err(|_| FileIoError::Uncertain)?;
        private(&metadata, false).map_err(|_| FileIoError::Uncertain)?;
        Ok(PrivateSnapshot {
            directory: self.identity,
            slot: self.slot,
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
            getrandom::fill(&mut random).map_err(|_| FileIoError::Io)?;
            let value = u128::from_be_bytes(random);
            let name = format!(".{}-{value:032x}.tmp", store.slot.stem());
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
                Err(_) => return Err(FileIoError::Io),
            }
        }
        Err(FileIoError::Io)
    }
    fn validate(&self) -> Result<()> {
        let own = self.file.metadata().map_err(|_| FileIoError::Io)?;
        private(&own, false)?;
        let named = self.store.named(&self.name)?.ok_or(FileIoError::Location)?;
        if id(&own) != id(&named.metadata().map_err(|_| FileIoError::Io)?) {
            return Err(FileIoError::Location);
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
