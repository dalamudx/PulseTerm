//! Windows local private directories and file slots. Synchronous; dispatch off GUI/runtime threads.
use crate::{DirectoryError::Location, DirectoryResult, policy};
use std::{
    fs::{File, OpenOptions},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Component, Path, PathBuf, Prefix},
};
mod private_file;
#[allow(unsafe_code)]
mod sys;
pub use crate::file_state::FileSnapshot;
pub use private_file::AtomicFile;
pub use private_file::{PrivateFile, PrivateFileError, PrivateFileResult};
#[cfg(test)]
mod private_file_tests;
#[cfg(test)]
mod tests;

struct BoundDirectory {
    file: File,
    path: PathBuf,
    identity: sys::Identity,
}

/// An admitted directory and its pinned ancestors; grants no page or credential authority.
/// No Debug/serde: paths and account identifiers remain native-only.
pub struct PrivateDirectory {
    chain: Vec<BoundDirectory>,
    user: Vec<u8>,
}

fn paths(path: &Path) -> DirectoryResult<Vec<PathBuf>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().take(32768).collect();
    if wide.len() > 32767
        || wide
            .split(|&x| x == 47 || x == 92)
            .any(|x| x == [46] || x == [46, 46])
    {
        return Err(Location);
    }
    let mut parts = path.components();
    let prefix = match parts.next() {
        Some(Component::Prefix(p)) => p,
        _ => return Err(Location),
    };
    let drive = match prefix.kind() {
        Prefix::Disk(d) | Prefix::VerbatimDisk(d) if d.is_ascii_alphabetic() => d,
        _ => return Err(Location),
    };
    if parts.next() != Some(Component::RootDir) {
        return Err(Location);
    }
    sys::local_drive(drive)?;
    let mut current = PathBuf::from(prefix.as_os_str());
    current.push("\\");
    let mut result = vec![current.clone()];
    for c in parts {
        let Component::Normal(name) = c else {
            return Err(Location);
        };
        policy::component(&name.encode_wide().collect::<Vec<_>>())?;
        current.push(name);
        result.push(current.clone());
        if result.len() > 64 {
            return Err(Location);
        }
    }
    if result.len() < 2 {
        return Err(Location);
    }
    Ok(result)
}

fn open(path: &Path) -> DirectoryResult<File> {
    OpenOptions::new()
        .access_mode(0x00020080) // READ_CONTROL | FILE_READ_ATTRIBUTES, never data writes.
        .share_mode(3) // Read/write sharing, deliberately no FILE_SHARE_DELETE.
        .custom_flags(0x02200000) // BACKUP_SEMANTICS | OPEN_REPARSE_POINT.
        .open(path)
        .map_err(|_| Location)
}

fn bind(
    paths: &[PathBuf],
    user: &[u8],
    private_leaf: bool,
) -> DirectoryResult<Vec<BoundDirectory>> {
    let mut result = Vec::with_capacity(paths.len());
    for (i, p) in paths.iter().enumerate() {
        let file = open(p)?;
        let facts = sys::inspect(&file)?;
        policy::directory_attributes(facts.attributes)?;
        policy::admit_acl(
            &facts.owner,
            user,
            facts.acl.as_deref(),
            facts.protected,
            private_leaf && i + 1 == paths.len(),
        )?;
        result.push(BoundDirectory {
            file,
            path: p.clone(),
            identity: facts.identity,
        });
    }
    Ok(result)
}

impl PrivateDirectory {
    /// Admit an existing protected directory owned by this process user, without repairing it.
    /// Only local fixed ACL volumes, bounded normal paths and trusted ancestors are accepted.
    pub fn open(path: impl AsRef<Path>) -> DirectoryResult<Self> {
        let user = sys::user()?;
        let result = Self {
            chain: bind(&paths(path.as_ref())?, &user, true)?,
            user,
        };
        result.validate()?;
        Ok(result)
    }
    /// Create only the final component with a protected ACL from the moment of creation.
    /// Parents must already exist. Existing directories are validated, never chmod/re-owned.
    pub fn create(path: impl AsRef<Path>) -> DirectoryResult<Self> {
        let path = path.as_ref();
        let all = paths(path)?;
        let user = sys::user()?;
        let _parents = bind(&all[..all.len() - 1], &user, false)?;
        sys::create_private(all.last().ok_or(Location)?, &user)?;
        Self::open(path)
    }
    /// Recheck ownership, ACL, reparse state and exact handle/path identities.
    /// Held ancestors prevent rename, but same-account ACL changes are not an atomic transaction.
    pub fn validate(&self) -> DirectoryResult<()> {
        if sys::user()? != self.user {
            return Err(Location);
        }
        for (i, b) in self.chain.iter().enumerate() {
            for file in [&b.file, &open(&b.path)?] {
                let facts = sys::inspect(file)?;
                if facts.identity != b.identity {
                    return Err(Location);
                }
                policy::directory_attributes(facts.attributes)?;
                policy::admit_acl(
                    &facts.owner,
                    &self.user,
                    facts.acl.as_deref(),
                    facts.protected,
                    i + 1 == self.chain.len(),
                )?;
            }
        }
        Ok(())
    }
}
