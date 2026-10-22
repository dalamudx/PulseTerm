//! Windows local private directories and file slots. Synchronous; dispatch off GUI/runtime threads.
use crate::{DirectoryError::Location, DirectoryResult, policy};
use std::{
    fs::{File, OpenOptions},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Component, Path, PathBuf, Prefix},
};
mod cold_cli;
pub use cold_cli::{LocalControlPresence, WorkspaceGui, local_control_presence};
mod database;
mod read_only_directory;
pub use database::{PrivateDatabase, PrivateDatabaseBackup};
pub use read_only_directory::{ReadOnlyDirectory, ReadOnlyStamp};
mod local_control;
mod private_file;
#[allow(unsafe_code)]
mod sys;
pub use crate::file_state::FileSnapshot;
pub use crate::web_link::WebLinkError;
pub use local_control::{
    LocalControlIdentity, LocalControlToken, local_application_data_directory,
    process_is_current_user, verify_single_instance_owner,
};
pub use private_file::AtomicFile;
pub use private_file::{PrivateFile, PrivateFileError, PrivateFileResult};
#[cfg(test)]
mod private_file_tests;
#[cfg(test)]
mod tests;

/// Read the OS-selected system directory, independent of PATH and page input.
/// Synchronous metadata only; grants no executable or launch authority.
pub fn system_directory() -> DirectoryResult<PathBuf> {
    sys::system_directory()
}

/// Read the current user's registered default WSL distribution name without executing WSL.
/// Absence is distinct from an unreadable or malformed registry value.
/// Callers must capture the returned name as literal argv before launch admission.
pub fn default_wsl_distribution() -> DirectoryResult<Option<String>> {
    sys::default_wsl_distribution()
}
/// Hand a caller-validated HTTP(S) URL to the system default handler without command arguments.
/// Call only on a dedicated blocking thread. Authorization is checked after COM initialization,
/// immediately before dispatch. Success means handler handoff, not browser or network completion.
/// The caller must retain native I/O capacity through this call even if its waiter is cancelled.
pub fn open_web_url(url: &str, authorize: impl FnOnce() -> bool) -> Result<(), WebLinkError> {
    crate::web_link::validate(url)?;
    sys::open_web_url(url, authorize)
}
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
    open_native(path).map_err(|_| Location)
}

fn open_native(path: &Path) -> std::io::Result<File> {
    // Metadata-only access does not enforce the no-delete sharing restriction.
    OpenOptions::new()
        .access_mode(0x00020081) // READ_CONTROL | FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY.
        .share_mode(3) // Read/write sharing, deliberately no FILE_SHARE_DELETE.
        .custom_flags(0x02200000) // BACKUP_SEMANTICS | OPEN_REPARSE_POINT.
        .open(path)
}

fn bind(
    paths: &[PathBuf],
    user: &[u8],
    private_leaf: bool,
    create_missing: bool,
) -> DirectoryResult<Vec<BoundDirectory>> {
    let mut result = Vec::with_capacity(paths.len());
    for (i, p) in paths.iter().enumerate() {
        let file = match open(p) {
            Ok(file) => file,
            Err(_) if create_missing && i > 0 => {
                // Every parent has already been admitted and stays pinned in result.
                // Existing entries are never repaired; creation must still reopen and validate.
                sys::create_private(p, user)?;
                open(p)?
            }
            Err(error) => return Err(error),
        };
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
            chain: bind(&paths(path.as_ref())?, &user, true, false)?,
            user,
        };
        result.validate()?;
        Ok(result)
    }
    /// Prepare absent components privately, keeping admitted ancestors pinned during creation.
    /// Existing directories are only validated; this grants no page or SQLite authority.
    pub fn prepare(path: impl AsRef<Path>) -> DirectoryResult<Self> {
        let user = sys::user()?;
        let result = Self {
            chain: bind(&paths(path.as_ref())?, &user, true, true)?,
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
        let _parents = bind(&all[..all.len() - 1], &user, false, false)?;
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
