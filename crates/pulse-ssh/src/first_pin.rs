//! Trusted upper adapter only. No page-supplied public key, connection grant or Vault use.
use super::*;
use crate::peer::{ObservedHostKey, OfferedKeyStatus, observe_host_key};
use pulse_store::{StateStore, ssh_target_source::SshTargetSource};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

/// Content-free first-use refusal; no raw peer, target or database error text.
#[derive(Debug, thiserror::Error)]
pub enum FirstPinError {
    /// Original current-page action or its native guard was revoked/expired.
    #[error("ssh.first_pin.denied")]
    Denied,
    /// Existing/changed keys cannot enter the first-use approval path.
    #[error("ssh.first_pin.known")]
    Known,
    /// Whole trust/source/target could not be validated; no fallback or automatic retry.
    #[error("ssh.first_pin.source")]
    Source,
    /// No completed offered-key observation is available.
    #[error("ssh.first_pin.observation")]
    Observation,
}
struct Authority {
    live: AtomicBool,
    expires: tokio::time::Instant,
    native: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl Authority {
    fn current(&self) -> bool {
        self.live.load(Ordering::Acquire)
            && tokio::time::Instant::now() < self.expires
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.native)()))
                .unwrap_or(false)
    }
}
/// One owned first-use prompt from the actual offered key and the pre-dial saved source.
/// No constructor from a fingerprint/raw key and no Clone, Debug or page serialization.
/// Cancellation revokes native writes; a committed write is not promised to roll back.
/// ```compile_fail
/// fn dump(v:pulse_ssh::managed_store::FirstPinApproval){println!("{v:?}");}
/// ```
pub struct FirstPinApproval {
    candidate: ObservedHostKey,
    source: SshTargetSource,
    host: Uuid,
    authority: Arc<Authority>,
}
impl Drop for FirstPinApproval {
    fn drop(&mut self) {
        self.authority.live.store(false, Ordering::Release);
    }
}
/// One actual refused-handshake observation, never an authenticated connection.
/// A blocked matched/changed key cannot be converted into a first-use prompt.
pub enum FirstPinObservation {
    /// Unknown key; explicit original-page approval may consume this owned prompt.
    FirstUse(Box<FirstPinApproval>),
    /// Matched or changed key, public display metadata only; no approval surface.
    Blocked(ObservedHostKey),
}
impl FirstPinApproval {
    /// Trusted backend supplies the pre-dial whole source, selected host, owned route stream
    /// and original page/action guard (including native operation admission, but no keys).
    /// Nothing may be minted from remote data or a page-supplied key/scope/callback.
    /// All application keys parse before SSH starts. System absence differs from an empty view.
    /// Observation always rejects auth and destroys owned I/O before returning this prompt.
    /// Original five-minute prompt expiry includes observation time; handshake retains five seconds.
    pub async fn observe<R>(
        source: SshTargetSource,
        host: Uuid,
        system: Option<Vec<HostPin>>,
        config: Arc<russh::client::Config>,
        native: Arc<dyn Fn() -> bool + Send + Sync>,
        stream: R,
    ) -> Result<Self, FirstPinError>
    where
        R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        match Self::inspect(source, host, system, config, native, stream).await? {
            FirstPinObservation::FirstUse(prompt) => Ok(*prompt),
            FirstPinObservation::Blocked(_) => Err(FirstPinError::Known),
        }
    }
    /// Inspect the actual offered key once with identical source/page/I/O gates.
    /// Known/changed outcomes expose only public metadata and never mint approval.
    pub async fn inspect<R>(
        source: SshTargetSource,
        host: Uuid,
        system: Option<Vec<HostPin>>,
        config: Arc<russh::client::Config>,
        native: Arc<dyn Fn() -> bool + Send + Sync>,
        stream: R,
    ) -> Result<FirstPinObservation, FirstPinError>
    where
        R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let authority = Arc::new(Authority {
            live: AtomicBool::new(true),
            expires: tokio::time::Instant::now() + std::time::Duration::from_secs(300),
            native,
        });
        if !authority.current() {
            return Err(FirstPinError::Denied);
        }
        let endpoint = source
            .hosts()
            .records()
            .iter()
            .find(|entry| entry.id() == host)
            .ok_or(FirstPinError::Source)?
            .endpoint();
        let target = HostTarget::new(endpoint.target().host(), endpoint.target().port())
            .map_err(|_| FirstPinError::Source)?;
        let own = validated_pins(source.trust()).map_err(|_| FirstPinError::Source)?;
        let candidate =
            observe_host_key(target, Some(own), system, config, authority.clone(), stream)
                .await
                .map_err(|_| FirstPinError::Observation)?;
        if !authority.current() {
            return Err(FirstPinError::Denied);
        }
        if candidate.status() != OfferedKeyStatus::Unknown {
            return Ok(FirstPinObservation::Blocked(candidate));
        }
        Ok(FirstPinObservation::FirstUse(Box::new(Self {
            candidate,
            source,
            host,
            authority,
        })))
    }
    /// Backend-only public-key display metadata; not an approval or live peer claim.
    pub fn candidate(&self) -> &ObservedHostKey {
        &self.candidate
    }
    /// Immediately deny this prompt and any in-flight uncommitted native write.
    pub fn cancel(&self) {
        self.authority.live.store(false, Ordering::Release);
    }
    /// Consume once after an independently confirmed user decision bound to this prompt/page.
    /// Returns only a historical trust revision, never a connection/credential permission.
    /// The whole source and selected target are compared in the same transaction as insertion;
    /// any known target, changed source, foreign store or revoked page rejects without replacement.
    /// Native SQL retains the guard/admission until exit even if this waiting future is dropped.
    pub async fn confirm(self, store: &StateStore, approved: bool) -> Result<u64, FirstPinError> {
        self.confirm_with_guard(store, approved, Arc::new(|| true))
            .await
    }
    /// Additional native SQL reservation/predicate restricts the original authority;
    /// it cannot replace or upgrade the prompt's original page/source/expiry checks.
    pub async fn confirm_with_guard(
        self,
        store: &StateStore,
        approved: bool,
        write_guard: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<u64, FirstPinError> {
        self.confirm_source_with_guard(store, approved, write_guard)
            .await
            .map(|source| source.trust().revision())
    }
    /// Trusted routed adapter: preserve the exact source returned by the first-pin transaction.
    pub async fn confirm_source_with_guard(
        self,
        store: &StateStore,
        approved: bool,
        write_guard: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<SshTargetSource, FirstPinError> {
        if !approved || !self.authority.current() {
            return Err(FirstPinError::Denied);
        }
        let TrustEdit::Pin(record) = pin_edit(self.candidate.target(), self.candidate.public_key())
            .map_err(|_| FirstPinError::Source)?
        else {
            return Err(FirstPinError::Source);
        };
        let authority = self.authority.clone();
        let source = store
            .approve_first_ssh_pin(
                self.source.clone(),
                self.host,
                record,
                Arc::new(move || {
                    authority.current()
                        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write_guard()))
                            .unwrap_or(false)
                }),
            )
            .await
            .map_err(|_| FirstPinError::Source)?;
        if !self.authority.current() {
            return Err(FirstPinError::Denied);
        }
        Ok(source)
    }
}
