//! SQLite admission, distinct from immutable text leases and atomic namespace replacement.
use super::{PrivateDirectory, PrivateFileError as Error, PrivateFileResult as Result, sys};
use crate::policy;
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::Path,
};

const NAME: &str = "state.db";

/// Pins the private parent and main file until the SQLite worker actually closes.
/// Exposes no raw handle, path, SQL or application authorization.
pub struct PrivateDatabase {
    file: File,
    identity: sys::Identity,
    directory: PrivateDirectory,
}

/// Privately created backup, pinned while SQLite writes and the final flush runs.
pub struct PrivateDatabaseBackup<'a> {
    file: File,
    identity: sys::Identity,
    name: String,
    database: &'a PrivateDatabase,
}

fn shared(path: &Path, writable: bool, deletable: bool) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(writable)
        .access_mode(0x80020000 | if writable { 0x40000000 } else { 0 })
        .share_mode(0x00000003 | if deletable { 0x00000004 } else { 0 })
        .custom_flags(0x00200000) // OPEN_REPARSE_POINT; file kind is checked separately.
        .open(path)
        .map_err(super::private_file::open_error)
}

fn file_facts(file: &File, user: &[u8], inherited_sidecar: bool) -> Result<sys::Facts> {
    let facts = sys::inspect(file).map_err(|_| Error::Location)?;
    policy::file_attributes(facts.attributes, facts.links).map_err(|_| Error::Location)?;
    if inherited_sidecar {
        // Only SQLite sidecars under the pinned private parent may inherit its trusted ACL.
        policy::sqlite_sidecar_acl(&facts.owner, user, facts.acl.as_deref(), facts.protected)
    } else {
        policy::admit_acl(
            &facts.owner,
            user,
            facts.acl.as_deref(),
            facts.protected,
            true,
        )
    }
    .map_err(|_| Error::Location)?;
    Ok(facts)
}

impl PrivateDirectory {
    /// Admit the fixed database without repairing or truncating existing files.
    /// The main pin shares SQLite writes and prevents namespace replacement.
    pub fn database(self) -> Result<PrivateDatabase> {
        self.validate().map_err(|_| Error::Location)?;
        let path = self.chain.last().ok_or(Error::Location)?.path.join(NAME);
        let (file, created_identity) = match shared(&path, false, false) {
            Ok(file) => (file, None),
            Err(Error::NotFound) => {
                let created = self.create_file(NAME, b"", 0)?;
                let identity = created.validate()?.identity;
                drop(created);
                (shared(&path, false, false)?, Some(identity))
            }
            Err(error) => return Err(error),
        };
        let identity = file_facts(&file, &self.user, false)?.identity;
        if created_identity.is_some_and(|original| original != identity) {
            return Err(Error::Location);
        }
        let result = PrivateDatabase {
            file,
            identity,
            directory: self,
        };
        result.validate()?;
        Ok(result)
    }
}

impl PrivateDatabase {
    /// Recheck parent/main identity and all existing WAL/SHM/journal files.
    /// Sidecars may disappear normally. This is not complete TOCTOU protection.
    pub fn validate(&self) -> Result<()> {
        self.directory.validate().map_err(|_| Error::Location)?;
        if file_facts(&self.file, &self.directory.user, false)?.identity != self.identity {
            return Err(Error::Location);
        }
        let parent = &self.directory.chain.last().ok_or(Error::Location)?.path;
        let reopened = shared(&parent.join(NAME), false, false)?;
        if file_facts(&reopened, &self.directory.user, false)?.identity != self.identity {
            return Err(Error::Location);
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            match shared(&parent.join(format!("{NAME}{suffix}")), false, true) {
                Ok(file) => {
                    file_facts(&file, &self.directory.user, true)?;
                }
                Err(Error::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        self.directory.validate().map_err(|_| Error::Location)
    }

    fn backup_slot_clear(&self, name: &str) -> Result<()> {
        let parent = &self.directory.chain.last().ok_or(Error::Location)?.path;
        for suffix in ["-wal", "-shm", "-journal"] {
            match shared(&parent.join(format!("{name}{suffix}")), false, true) {
                Err(Error::NotFound) => {}
                _ => return Err(Error::Conflict),
            }
        }
        Ok(())
    }
    /// Exclusively create a versioned backup. Existing targets are never overwritten.
    pub fn backup(&self, target_version: u32) -> Result<PrivateDatabaseBackup<'_>> {
        self.validate()?;
        if target_version == 0 {
            return Err(Error::Location);
        }
        let name = format!("{NAME}.bak-{target_version}");
        let parent = &self.directory.chain.last().ok_or(Error::Location)?.path;
        self.backup_slot_clear(&name)?;
        let created = self.directory.create_file(&name, b"", 0)?;
        let original = created.validate()?.identity;
        drop(created);
        let path = parent.join(&name);
        let file = shared(&path, true, false)?;
        let identity = file_facts(&file, &self.directory.user, false)?.identity;
        if identity != original {
            return Err(Error::Location);
        }
        Ok(PrivateDatabaseBackup {
            file,
            identity,
            name,
            database: self,
        })
    }
}

impl PrivateDatabaseBackup<'_> {
    /// Flush and validate after SQLite closes. Does not promise directory crash durability.
    /// A failed/partial backup is retained; it must not be silently overwritten on retry.
    pub fn sync(&self) -> Result<()> {
        self.database.validate()?;
        if file_facts(&self.file, &self.database.directory.user, false)?.identity != self.identity {
            return Err(Error::Location);
        }
        self.database.backup_slot_clear(&self.name)?;
        self.file.sync_all().map_err(|_| Error::Io)?;
        if file_facts(&self.file, &self.database.directory.user, false)?.identity != self.identity {
            return Err(Error::Location);
        }
        self.database.validate()
    }
}
