//! Fixed GUI sibling leases and read-only cold-control observation. Neither grants action authority.
use super::{
    BoundDirectory, PrivateDirectory, PrivateFileError, bind, open, open_native, paths, sys,
};
use crate::{DirectoryError::Location, DirectoryResult, file_state::FileStamp, policy};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

/// Presence only, not authentication. Partial or unsafe namespaces are rejected.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LocalControlPresence {
    /// No token or pipe, or an absent component after validated ancestors.
    Absent,
    /// Both exist; the existing transport must still authenticate, without cold fallback.
    Present,
}

/// Observe the backend-selected control directory without creating, repairing or connecting anything.
/// Each existing ancestor is pinned and checked before inspecting its child. Run off GUI threads.
pub fn local_control_presence(directory: &Path) -> DirectoryResult<LocalControlPresence> {
    let all = paths(directory)?;
    let user = sys::user()?;
    let mut chain = Vec::new();
    for (i, path) in all.iter().enumerate() {
        let file = match open_native(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LocalControlPresence::Absent);
            }
            Err(_) => return Err(Location),
        };
        let facts = sys::inspect(&file)?;
        policy::directory_attributes(facts.attributes)?;
        policy::admit_acl(
            &facts.owner,
            &user,
            facts.acl.as_deref(),
            facts.protected,
            i + 1 == all.len(),
        )?;
        chain.push(BoundDirectory {
            file,
            path: path.clone(),
            identity: facts.identity,
        });
    }
    let directory = PrivateDirectory { chain, user };
    let token = match directory.open_file("control.token") {
        Ok(_) => true,
        Err(PrivateFileError::NotFound) => false,
        Err(_) => return Err(Location),
    };
    let pipe = sys::control_pipe_present(
        directory
            .local_control_identity()
            .map_err(|_| Location)?
            .pipe_name(),
    )?;
    directory.validate()?;
    match (token, pipe) {
        (false, false) => Ok(LocalControlPresence::Absent),
        (true, true) => Ok(LocalControlPresence::Present),
        _ => Err(Location),
    }
}

/// Pin the fixed pulseterm-workspace.exe beside the native CLI executable.
/// No caller-selected target, shell, PATH lookup or permission repair is provided.
/// Installation directories may be readable by others, but cannot grant them mutation rights.
pub struct WorkspaceGui {
    chain: Vec<BoundDirectory>,
    user: Vec<u8>,
    file: File,
    path: PathBuf,
    stamp: FileStamp,
}
impl WorkspaceGui {
    /// The trusted CLI supplies current_exe(); this never reads or executes the candidate's bytes.
    pub fn beside(current_executable: &Path) -> DirectoryResult<Self> {
        let parent = current_executable.parent().ok_or(Location)?;
        let user = sys::user()?;
        let chain = bind(&paths(parent)?, &user, false, false)?;
        let path = parent.join("pulseterm-workspace.exe");
        let file = sys::open_regular(&path, None).map_err(|_| Location)?;
        let stamp = sys::inspect(&file)?.stamp();
        let result = Self {
            chain,
            user,
            file,
            path,
            stamp,
        };
        result.validate()?;
        Ok(result)
    }
    /// Native-only fixed path; the caller retains this lease through OS process creation.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Recheck the original file, directory identities and ACLs immediately before process creation.
    /// Normal writes, rename and deletion are denied by held handles; this is not an ACL transaction.
    pub fn validate(&self) -> DirectoryResult<()> {
        if sys::user()? != self.user {
            return Err(Location);
        }
        for (i, entry) in self.chain.iter().enumerate() {
            for file in [&entry.file, &open(&entry.path)?] {
                let facts = sys::inspect(file)?;
                if facts.identity != entry.identity {
                    return Err(Location);
                }
                policy::directory_attributes(facts.attributes)?;
                if i + 1 == self.chain.len() {
                    policy::program_acl(
                        &facts.owner,
                        &self.user,
                        facts.acl.as_deref(),
                        facts.protected,
                    )?;
                } else {
                    policy::admit_acl(
                        &facts.owner,
                        &self.user,
                        facts.acl.as_deref(),
                        facts.protected,
                        false,
                    )?;
                }
            }
        }
        for file in [
            &self.file,
            &sys::open_regular(&self.path, None).map_err(|_| Location)?,
        ] {
            let facts = sys::inspect(file)?;
            // Read-only images are valid; directories, devices, reparse points and aliases are not.
            if facts.attributes & 0x450 != 0 || facts.links != 1 || facts.stamp() != self.stamp {
                return Err(Location);
            }
            policy::program_acl(
                &facts.owner,
                &self.user,
                facts.acl.as_deref(),
                facts.protected,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
