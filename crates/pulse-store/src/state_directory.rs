//! Unix platform-app-data directory admission, not a page-selected path API.
//! The caller supplies the OS app-data directory; no user-supplied path is accepted by IPC.
use crate::{Result, StoreError};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Component, Path, PathBuf},
};

/// Create absent components privately and reject symbolic links or unsafe existing parents.
/// The final directory must belong to this effective user and have exactly 0700 mode.
/// This checks the path at admission; it cannot guarantee protection from same-UID
/// replacement or power loss between this check and SQLite's independent checks.
pub fn prepare(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::CurDir | Component::Prefix(_)
            )
        })
    {
        return Err(StoreError::Location);
    }
    let owner = rustix::process::geteuid().as_raw();
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(value) => {
                current.push(value);
                let metadata = match fs::symlink_metadata(&current) {
                    Ok(m) => m,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let parent = current.parent().ok_or(StoreError::Location)?;
                        let metadata =
                            fs::symlink_metadata(parent).map_err(|_| StoreError::Location)?;
                        let write_bit = if metadata.uid() == owner {
                            0o200
                        } else {
                            0o002
                        };
                        // Honor directory read-only intent even when running as root.
                        if metadata.mode() & write_bit == 0 {
                            return Err(StoreError::Location);
                        }
                        DirBuilder::new()
                            .mode(0o700)
                            .create(&current)
                            .or_else(|e| {
                                if e.kind() == std::io::ErrorKind::AlreadyExists {
                                    Ok(())
                                } else {
                                    Err(e)
                                }
                            })
                            .map_err(|_| StoreError::Location)?;
                        fs::symlink_metadata(&current).map_err(|_| StoreError::Location)?
                    }
                    Err(_) => return Err(StoreError::Location),
                };
                if !metadata.is_dir() || (metadata.uid() != owner && metadata.uid() != 0) {
                    return Err(StoreError::Location);
                }
                let final_component = current == path;
                if final_component {
                    if metadata.uid() != owner || metadata.mode() & 0o7777 != 0o700 {
                        return Err(StoreError::Location);
                    }
                } else if metadata.mode() & 0o022 != 0
                    && !(metadata.uid() == 0 && metadata.mode() & 0o1777 == 0o1777)
                {
                    // Only the root-owned sticky /tmp-style parent is permitted.
                    return Err(StoreError::Location);
                }
            }
            _ => return Err(StoreError::Location),
        }
    }
    if current != path || current == Path::new("/") {
        return Err(StoreError::Location);
    }
    Ok(current)
}

#[cfg(test)]
#[path = "state_directory_tests.rs"]
mod tests;
