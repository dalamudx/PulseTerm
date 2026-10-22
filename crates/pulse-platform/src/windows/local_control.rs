//! Private local-control namespace, peer account queries and identity-bound token cleanup.
use super::{PrivateDirectory, PrivateFileError, PrivateFileResult, sys};
use crate::{DirectoryResult, file_state::Identity, policy};
use std::os::windows::io::OwnedHandle;

/// Check the process token user against this process, using only read-only native queries.
/// This proves an account match, not executable identity or application execution authority.
pub fn process_is_current_user(pid: u32) -> DirectoryResult<bool> {
    sys::process_is_current_user(pid)
}

/// Read the current user's LocalAppData KnownFolder, without using caller-selected paths or env vars.
/// The returned location is metadata only; callers must independently admit its private descendants.
pub fn local_application_data_directory() -> DirectoryResult<std::path::PathBuf> {
    sys::local_application_data_directory()
}
/// Prove the locked single-instance plugin's exact hidden window belongs to this process.
/// This does not authenticate CLI clients or authorize page actions. Run after plugin setup.
pub fn verify_single_instance_owner(identifier: &str) -> DirectoryResult<()> {
    sys::verify_single_instance_owner(identifier)
}
/// Native-only local pipe namespace and protected ACL derived from an admitted private directory.
/// No Debug/serde; process/account identifiers never need to be projected to a page.
pub struct LocalControlIdentity {
    pipe_name: String,
    descriptor: String,
}
impl LocalControlIdentity {
    /// Fixed local pipe path bound to directory identity and current process account.
    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }
    /// Protected owner/System ACL in SDDL, for a trusted named-pipe listener only.
    pub fn security_descriptor(&self) -> &str {
        &self.descriptor
    }
    /// Nonblocking open with IDENTIFICATION QOS; busy returns its native error to the async caller.
    /// Does not authenticate the peer or grant any CLI action. Never calls WaitNamedPipe.
    pub fn open_client_handle(&self) -> std::io::Result<OwnedHandle> {
        sys::open_control_client(&self.pipe_name)
    }
}
impl PrivateDirectory {
    /// Derive a local-control namespace from the held final directory identity, never a caller name.
    pub fn local_control_identity(&self) -> PrivateFileResult<LocalControlIdentity> {
        self.validate().map_err(|_| PrivateFileError::Location)?;
        let id = self
            .chain
            .last()
            .ok_or(PrivateFileError::Location)?
            .identity;
        let user = sys::sid_text(&self.user).map_err(|_| PrivateFileError::Location)?;
        let index = id
            .index
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>();
        Ok(LocalControlIdentity {
            pipe_name: format!(r"\\.\pipe\pulseterm.cli.{user}.{:016x}.{index}", id.volume),
            descriptor: format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;SY)"),
        })
    }
}

/// Own one newly created control.token and its pinned private directory until explicit cleanup.
/// Replaced files are never deleted. Dropping retries no uncertain explicit cleanup.
/// A crash may leave the token; later startup must reject it rather than delete stale endpoints.
pub struct LocalControlToken {
    directory: PrivateDirectory,
    identity: Identity,
    attempted: bool,
}
impl LocalControlToken {
    /// Create exactly one 64-byte lowercase hexadecimal token, without overwriting an existing file.
    /// The writable handle is closed before clients may read. This grants no page/action authority.
    pub fn create(directory: PrivateDirectory, token: &[u8]) -> PrivateFileResult<Self> {
        if token.len() != 64
            || !token
                .iter()
                .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(x))
        {
            return Err(PrivateFileError::Limit);
        }
        let created = directory.create_file("control.token", token, 64)?;
        let identity = created.validate()?.identity;
        drop(created);
        Ok(Self {
            directory,
            identity,
            attempted: false,
        })
    }
    /// Validate the parent and exact original file, then delete only that opened identity.
    /// Active readers, permission changes or replacement fail closed; there is no stale-path repair.
    pub fn cleanup(&mut self) -> PrivateFileResult<()> {
        if self.attempted {
            return Err(PrivateFileError::Uncertain);
        }
        self.attempted = true;
        self.directory
            .validate()
            .map_err(|_| PrivateFileError::Location)?;
        let path = self
            .directory
            .chain
            .last()
            .ok_or(PrivateFileError::Location)?
            .path
            .join("control.token");
        let file =
            sys::open_control_token_for_delete(&path).map_err(super::private_file::open_error)?;
        let facts = sys::inspect(&file).map_err(|_| PrivateFileError::Location)?;
        if facts.identity != self.identity {
            return Err(PrivateFileError::Conflict);
        }
        policy::file_attributes(facts.attributes, facts.links)
            .map_err(|_| PrivateFileError::Location)?;
        policy::admit_acl(
            &facts.owner,
            &self.directory.user,
            facts.acl.as_deref(),
            facts.protected,
            true,
        )
        .map_err(|_| PrivateFileError::Location)?;
        sys::discard_file(&file).map_err(|_| PrivateFileError::Uncertain)
    }
}
impl Drop for LocalControlToken {
    fn drop(&mut self) {
        if !self.attempted {
            let _ = self.cleanup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[test]
    fn namespace_is_local_directory_bound_and_peer_query_matches_current_process() -> TestResult {
        let root = tempfile::tempdir()?;
        let one = PrivateDirectory::create(root.path().join("one"))?;
        let two = PrivateDirectory::create(root.path().join("two"))?;
        let a = one.local_control_identity()?;
        assert!(a.pipe_name().starts_with(r"\\.\pipe\pulseterm.cli."));
        assert_ne!(a.pipe_name(), two.local_control_identity()?.pipe_name());
        assert!(process_is_current_user(std::process::id())?);
        assert!(process_is_current_user(0).is_err());
        Ok(())
    }
    #[test]
    fn token_is_readable_exact_and_exclusive_until_explicit_identity_cleanup() -> TestResult {
        let root = tempfile::tempdir()?;
        let path = root.path().join("control");
        let dir = PrivateDirectory::create(&path)?;
        let mut owned = LocalControlToken::create(dir, &[b'a'; 64])?;
        let dir = PrivateDirectory::open(&path)?;
        assert_eq!(
            dir.open_file("control.token")?.read_bounded(64)?,
            [b'a'; 64]
        );
        assert!(LocalControlToken::create(PrivateDirectory::open(&path)?, &[b'b'; 64]).is_err());
        owned.cleanup()?;
        assert!(!path.join("control.token").exists());
        Ok(())
    }
    #[test]
    fn replacement_is_never_deleted_and_invalid_tokens_create_nothing() -> TestResult {
        let root = tempfile::tempdir()?;
        let path = root.path().join("control");
        let mut owned = LocalControlToken::create(PrivateDirectory::create(&path)?, &[b'a'; 64])?;
        std::fs::rename(path.join("control.token"), path.join("original"))?;
        drop(PrivateDirectory::open(&path)?.create_file("control.token", &[b'b'; 64], 64)?);
        assert_eq!(owned.cleanup(), Err(PrivateFileError::Conflict));
        drop(owned);
        assert_eq!(std::fs::read(path.join("control.token"))?, [b'b'; 64]);
        let bad = root.path().join("bad");
        assert!(LocalControlToken::create(PrivateDirectory::create(&bad)?, b"invalid").is_err());
        assert!(!bad.join("control.token").exists());
        Ok(())
    }
}
