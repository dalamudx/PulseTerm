//! Actual SSH transport host-key evidence, not an SSH credential-use permission.
use crate::{HostKeyError, HostPin, HostTarget, verify_host_key};
use russh::{
    client,
    keys::{PublicKey, PublicKeyOrCertificate},
};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::{AsyncRead, AsyncWrite};
/// Content-free refusal; never includes target, banner, key or credential material.
#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    /// Strict pinned-key admission refused.
    #[error("ssh.peer.host_key")]
    HostKey(#[source] HostKeyError),
    /// Host certificates require a separate trust-anchor policy.
    #[error("ssh.peer.certificate")]
    Certificate,
    /// Handshake/transport failed without usable peer evidence.
    #[error("ssh.peer.transport")]
    Transport,
    /// This owned native stream already carries its single session reservation.
    #[error("ssh.peer.admission_busy")]
    AdmissionBusy,
    /// Original transport/key claim is no longer current.
    #[error("ssh.peer.stale")]
    Stale,
}
struct Claim {
    target: HostTarget,
    key: PublicKey,
}
struct State {
    claim: Option<Arc<Claim>>,
    refusal: Option<PeerError>,
}
#[path = "peer_native_io.rs"]
mod native_io;
#[path = "peer_io.rs"]
mod revocable_io;
pub use native_io::NativeIoCompletion;
struct Shared {
    io: native_io::IoLifetime,
    wakeups: Mutex<[Option<std::task::Waker>; 2]>,
    closed: AtomicBool,
    state: Mutex<State>,
}
impl Shared {
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let wakeups = {
            let mut slots = self
                .wakeups
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            [slots[0].take(), slots[1].take()]
        };
        for wake in wakeups.into_iter().flatten() {
            wake.wake();
        }
    }
    fn guard(&self) -> Result<std::sync::MutexGuard<'_, State>, PeerError> {
        self.state.lock().map_err(|poison| {
            drop(poison);
            self.close();
            PeerError::Stale
        })
    }
}
struct CloseGuard(Arc<Shared>);
impl Drop for CloseGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}
/// Opaque evidence minted only after this library's actual pinned handshake.
/// Does not keep a transport alive; source/identity/epoch/target-use approval remain separate.
/// ```compile_fail
/// fn forbidden(value: pulse_ssh::peer::VerifiedPeer) {println!("{value:?}");}
/// ```
#[derive(Clone)]
pub struct VerifiedPeer {
    shared: Weak<Shared>,
    claim: Arc<Claim>,
}
impl VerifiedPeer {
    /// Canonical backend-selected target, not a name from the remote peer.
    pub fn target(&self) -> &HostTarget {
        &self.claim.target
    }
    /// Public key observed and verified in this actual transport handshake.
    pub fn public_key(&self) -> &PublicKey {
        &self.claim.key
    }
    /// Disconnect, owner drop, poison or a different accepted rekey revokes this claim.
    pub fn is_current(&self) -> bool {
        let Some(shared) = self.shared.upgrade() else {
            return false;
        };
        if shared.closed.load(Ordering::Acquire) {
            return false;
        }
        let Ok(state) = shared.guard() else {
            return false;
        };
        !shared.closed.load(Ordering::Acquire)
            && state
                .claim
                .as_ref()
                .is_some_and(|claim| Arc::ptr_eq(claim, &self.claim))
    }
}
/// Opaque russh handler; public only as the handle's concrete type, never caller-constructible.
pub struct PeerHandler {
    target: HostTarget,
    own: Vec<HostPin>,
    system: Option<Vec<HostPin>>,
    shared: Arc<Shared>,
}
impl Drop for PeerHandler {
    fn drop(&mut self) {
        self.shared.close();
    }
}
impl client::Handler for PeerHandler {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        presented: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let result = match presented {
            PublicKeyOrCertificate::PublicKey { key, .. } => {
                verify_host_key(&self.target, key, Some(&self.own), self.system.as_deref())
                    .map_err(PeerError::HostKey)
            }
            _ => Err(PeerError::Certificate),
        };
        let Ok(mut state) = self.shared.guard() else {
            return Ok(false);
        };
        if let Err(error) = result {
            state.refusal = Some(error);
            state.claim = None;
            drop(state);
            self.shared.close();
            return Ok(false);
        }
        if self.shared.closed.load(Ordering::Acquire) {
            return Ok(false);
        }
        let PublicKeyOrCertificate::PublicKey { key, .. } = presented else {
            return Ok(false);
        };
        if state.claim.as_ref().is_none_or(|claim| claim.key != *key) {
            state.claim = Some(Arc::new(Claim {
                target: self.target.clone(),
                key: key.clone(),
            }));
        }
        Ok(true)
    }
    async fn disconnected(
        &mut self,
        reason: client::DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        self.shared.close();
        match reason {
            client::DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            client::DisconnectReason::Error(error) => Err(error),
        }
    }
}
/// Owns one host-verified but not user-authenticated SSH transport.
/// Pure pin verification, saved IDs and possession of a peer proof do not authorize credentials.
pub struct PinnedTransport {
    handle: client::Handle<PeerHandler>,
    guard: CloseGuard,
}
/// Owned handshake attempt with a key-free physical stream exit observation available BEFORE polling.
/// The observation is neither SDK task join, peer evidence, credential-use nor session permission.
#[must_use]
pub struct PendingPinnedTransport {
    future: std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<PinnedTransport, PeerError>> + Send>,
    >,
    completion: NativeIoCompletion,
}
impl PendingPinnedTransport {
    /// Observe actual stream destruction even if connect is cancelled or rejects the peer.
    /// An upper supervisor must retain its quota and await this after dropping a failed attempt.
    pub fn native_io_completion(&self) -> NativeIoCompletion {
        self.completion.duplicate()
    }
    /// Drive the original strict handshake and five-second cutoff. Drop revokes/wakes owned I/O.
    /// Only success yields actual-connection peer evidence, still NOT credential permission.
    pub async fn connect(self) -> Result<PinnedTransport, PeerError> {
        self.future.await
    }
}
impl PinnedTransport {
    /// Compatibility strict connection API. Supervisors needing failure/cancel drainage use begin_connect_stream.
    pub async fn connect_stream<R>(
        target: HostTarget,
        own: Option<Vec<HostPin>>,
        system: Option<Vec<HostPin>>,
        config: Arc<client::Config>,
        stream: R,
    ) -> Result<Self, PeerError>
    where
        R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        Self::begin_connect_stream(target, own, system, config, stream)?
            .connect()
            .await
    }
    /// Existing route/dialer supplies a trusted target and owned stream; no DNS/filesystem access here.
    /// Pins must come from independently validated backend sources. Missing source is not empty.
    /// Cancellation/timeout closes evidence even if a spawned russh task finishes late.
    pub fn begin_connect_stream<R>(
        target: HostTarget,
        own: Option<Vec<HostPin>>,
        system: Option<Vec<HostPin>>,
        config: Arc<client::Config>,
        stream: R,
    ) -> Result<PendingPinnedTransport, PeerError>
    where
        R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let own = own.ok_or(PeerError::HostKey(HostKeyError::StoreUnavailable))?;
        if own.len() > crate::MAX_PINS
            || system
                .as_ref()
                .is_some_and(|pins| pins.len() > crate::MAX_PINS)
        {
            return Err(PeerError::HostKey(HostKeyError::StoreUnavailable));
        }
        let shared = Arc::new(Shared {
            io: native_io::IoLifetime::new(),
            wakeups: Mutex::new([None, None]),
            closed: AtomicBool::new(false),
            state: Mutex::new(State {
                claim: None,
                refusal: None,
            }),
        });
        let guard = CloseGuard(shared.clone());
        let handler = PeerHandler {
            target,
            own,
            system,
            shared: shared.clone(),
        };
        let completion = shared.io.receipt();
        let stream = revocable_io::RevocableIo::new(stream, shared.clone());
        let future = async move {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client::connect_stream(config, stream, handler),
            )
            .await;
            let handle = match result {
                Ok(Ok(handle)) => handle,
                _ => {
                    let error = {
                        let mut state = shared.guard()?;
                        state.refusal.take().unwrap_or(PeerError::Transport)
                    };
                    return Err(error);
                }
            };
            let transport = Self { handle, guard };
            transport.peer()?;
            Ok(transport)
        };
        Ok(PendingPinnedTransport {
            future: Box::pin(future),
            completion,
        })
    }
    /// Read current handshake evidence without authenticating or retrieving credentials.
    pub fn peer(&self) -> Result<VerifiedPeer, PeerError> {
        if self.handle.is_closed() || self.guard.0.closed.load(Ordering::Acquire) {
            return Err(PeerError::Stale);
        }
        let claim = self
            .guard
            .0
            .guard()?
            .claim
            .clone()
            .ok_or(PeerError::Stale)?;
        Ok(VerifiedPeer {
            shared: Arc::downgrade(&self.guard.0),
            claim,
        })
    }
    /// Retain one key-free backend session/dial reservation until the actual owned stream exits.
    /// Closing peer evidence or cancelling a receipt waiter cannot release it early.
    /// This is not credential-use permission; do not attach a Vault key or plaintext here.
    pub fn retain_native_admission(
        &mut self,
        proof: &VerifiedPeer,
        admission: Arc<dyn Send + Sync>,
    ) -> Result<NativeIoCompletion, PeerError> {
        let _client = self.client(proof)?;
        self.guard.0.io.retain(admission)
    }
    /// Close this transport and await BOTH the SDK task and actual owned stream teardown.
    /// Drop all channel/read/write handles first: a full channel queue can otherwise block SDK exit.
    /// An SDK error is not a normal SSH/remote-process result, but awaiting its Handle still proves
    /// task completion; success here only proves physical cleanup, never authentication or exit code.
    /// Upper supervisors must own this future to completion; dropping its waiter is not a join proof.
    pub async fn close_and_join(mut self) -> Result<(), PeerError> {
        let native = self.native_io_completion();
        // The SDK ignores Disconnect while waiting for a keyboard-interactive answer.
        // Revoke all socket reads/writes FIRST. An empty local response then releases
        // that wait without permitting an authentication packet to reach the network.
        self.guard.0.close();
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            self.handle
                .authenticate_keyboard_interactive_respond(Vec::new()),
        )
        .await;
        // The timeout does not prove cleanup or release admission; both joins below do.
        let _sdk_result = (&mut self.handle).await;
        native.wait().await
    }
    /// Observe underlying owned stream destruction only; not SDK task join or remote process exit.
    /// The key-free receipt cannot retain a connection, reservation, actor or thread.
    pub fn native_io_completion(&self) -> NativeIoCompletion {
        self.guard.0.io.receipt()
    }
    /// Trusted backend transport access after original connection/key identity revalidation.
    /// Caller must independently authorize any credential/target use; this is not that permission.
    pub fn client(
        &mut self,
        proof: &VerifiedPeer,
    ) -> Result<&mut client::Handle<PeerHandler>, PeerError> {
        if !Weak::ptr_eq(&proof.shared, &Arc::downgrade(&self.guard.0))
            || !proof.is_current()
            || self.handle.is_closed()
        {
            return Err(PeerError::Stale);
        }
        Ok(&mut self.handle)
    }
}
#[path = "peer_route_transports.rs"]
mod route_transports;
pub use route_transports::{RejectedRouteTransport, RouteTransportCleanup, RouteTransports};
#[path = "peer_observe.rs"]
mod observe;
pub use observe::{ObservedHostKey, OfferedKeyStatus, observe_host_key};
#[cfg(test)]
#[path = "peer_cancel_tests.rs"]
mod cancel_tests;
#[cfg(test)]
#[path = "peer_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "peer_timeout_tests.rs"]
mod timeout_tests;
