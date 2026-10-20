//! Backend-discovered local Agent public keys and one explicitly selected authentication.
//! Discovery and signing use separate revocable grants. No socket paths, private keys or generic
//! signing interface are issued to the frontend; untrusted Agent frames are bounded before parsing.
use pulse_core::ssh_agent_identity::SshAgentIdentity;
#[cfg(unix)]
use russh::keys::PublicKey;
use std::sync::Arc;
/// Independent original-page discovery or actual-peer signing grant.
pub type Authorize = Arc<dyn Fn() -> bool + Send + Sync>;
/// Fixed errors never contain Agent payloads, key material or local socket paths.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// This platform has no implemented local Agent transport.
    #[error("ssh.agent_unsupported")]
    Unsupported,
    /// Current discovery or signing permission was revoked.
    #[error("ssh.agent_denied")]
    Denied,
    /// No admitted local Agent or the explicitly selected key is available.
    #[error("ssh.agent_unavailable")]
    Unavailable,
    /// Invalid, duplicate, over-budget, or cryptographically invalid response.
    #[error("ssh.agent_protocol")]
    Protocol,
    /// Agent refused the single requested signature.
    #[error("ssh.agent_refused")]
    Refused,
}
impl From<russh::SendError> for AgentError {
    fn from(_: russh::SendError) -> Self {
        Self::Protocol
    }
}
/// Drawing-only discovered public-key metadata. Comments from the Agent are never projected.
pub struct AgentPublicKey {
    #[cfg(unix)]
    key: PublicKey,
    algorithm: String,
    fingerprint: String,
}
impl AgentPublicKey {
    /// Canonical supported SSH public-key algorithm.
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
    /// Full SHA256 fingerprint, computed from the received public key.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}
/// Owned local connection; listing keys does not authorize signing.
/// ```compile_fail
/// fn dump(v: pulse_ssh::agent::AgentConnection) { println!("{v:?}"); }
/// ```
pub struct AgentConnection {
    #[cfg(unix)]
    wire: wire::Wire,
    keys: Vec<AgentPublicKey>,
    discovery: Authorize,
}
impl AgentConnection {
    /// Discover only the backend's SSH_AUTH_SOCK. Socket owner and actual peer uid must match.
    /// Never accepts a frontend path and never adds keys or requests Agent forwarding.
    pub async fn discover(discovery: Authorize) -> Result<Self, AgentError> {
        if !discovery() {
            return Err(AgentError::Denied);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{FileTypeExt, MetadataExt};
            let path = std::env::var_os("SSH_AUTH_SOCK")
                .map(std::path::PathBuf::from)
                .ok_or(AgentError::Unavailable)?;
            if !path.is_absolute() {
                return Err(AgentError::Unavailable);
            }
            let meta = std::fs::symlink_metadata(&path).map_err(|_| AgentError::Unavailable)?;
            let uid = rustix::process::geteuid().as_raw();
            if !meta.file_type().is_socket() || meta.uid() != uid {
                return Err(AgentError::Unavailable);
            }
            let stream = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                tokio::net::UnixStream::connect(path),
            )
            .await
            .map_err(|_| AgentError::Unavailable)?
            .map_err(|_| AgentError::Unavailable)?;
            if stream
                .peer_cred()
                .map_err(|_| AgentError::Unavailable)?
                .uid()
                != uid
            {
                return Err(AgentError::Unavailable);
            }
            Self::read_stream(stream, discovery).await
        }
        #[cfg(not(unix))]
        {
            Err(AgentError::Unsupported)
        }
    }
    #[cfg(unix)]
    async fn read_stream(
        stream: tokio::net::UnixStream,
        discovery: Authorize,
    ) -> Result<Self, AgentError> {
        let mut wire = wire::Wire::new(stream);
        let response = wire.packet(&[11], &discovery).await?;
        let keys = wire::decode_keys(&response)?;
        if !discovery() {
            return Err(AgentError::Denied);
        }
        Ok(Self {
            wire,
            keys,
            discovery,
        })
    }
    /// Metadata only; callers must save a selection with separate page-bound CAS approval.
    pub fn keys(&self) -> &[AgentPublicKey] {
        &self.keys
    }
    /// Consume this connection to attempt exactly the selected key on an already approved peer.
    /// The signing grant must bind original page/epoch, exact saved source and actual peer.
    /// No fallback, key iteration, agent forwarding, shell, PTY or execution occurs here.
    pub async fn authenticate<H: russh::client::Handler>(
        self,
        handle: &mut russh::client::Handle<H>,
        selected: &SshAgentIdentity,
        signing: Authorize,
    ) -> Result<bool, AgentError> {
        self.authenticate_result(handle, selected, signing)
            .await
            .map(|(result, _)| result.success())
    }
    /// Require one signature from the saved key and server-confirmed partial success declaring MFA.
    /// Full success and ordinary failure cannot satisfy this explicitly selected joint policy.
    pub async fn authenticate_interactive_first_factor<H: russh::client::Handler>(
        self,
        handle: &mut russh::client::Handle<H>,
        selected: &SshAgentIdentity,
        signing: Authorize,
    ) -> Result<bool, AgentError> {
        let (result, signed) = self.authenticate_result(handle, selected, signing).await?;
        Ok(signed
            && matches!(result, russh::client::AuthResult::Failure { partial_success: true, remaining_methods }
            if remaining_methods.contains(&russh::MethodKind::KeyboardInteractive)))
    }
    async fn authenticate_result<H: russh::client::Handler>(
        self,
        handle: &mut russh::client::Handle<H>,
        selected: &SshAgentIdentity,
        signing: Authorize,
    ) -> Result<(russh::client::AuthResult, bool), AgentError> {
        if !signing() || !(self.discovery)() {
            return Err(AgentError::Denied);
        }
        let mut matching = self.keys.iter().filter(|key| {
            key.algorithm == selected.algorithm() && key.fingerprint == selected.fingerprint()
        });
        let chosen = matching.next().ok_or(AgentError::Unavailable)?;
        if matching.next().is_some() {
            return Err(AgentError::Protocol);
        }
        #[cfg(unix)]
        {
            let public = chosen.key.clone();
            let hash = if public.algorithm().is_rsa() {
                Some(
                    handle
                        .best_supported_rsa_hash()
                        .await
                        .map_err(|_| AgentError::Protocol)?
                        .flatten()
                        .ok_or(AgentError::Unavailable)?,
                )
            } else {
                None
            };
            let discovery = self.discovery;
            let current = signing.clone();
            let combined: Authorize = Arc::new(move || discovery() && signing());
            let mut signer = wire::SelectedSigner {
                wire: self.wire,
                key: public.clone(),
                current: combined,
                used: false,
            };
            let result = handle
                .authenticate_publickey_with(selected.username(), public, hash, &mut signer)
                .await?;
            if !current() || !(signer.current)() {
                return Err(AgentError::Denied);
            }
            Ok((result, signer.used))
        }
        #[cfg(not(unix))]
        {
            let _ = (handle, chosen);
            Err(AgentError::Unsupported)
        }
    }
}
#[cfg(all(test, unix))]
#[path = "agent_tests.rs"]
mod tests;
#[cfg(unix)]
#[path = "agent_wire.rs"]
mod wire;
