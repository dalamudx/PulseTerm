//! Candidate Rust-only auth adapter. No Tauri command or production private-key session entry.
//! Initial supported subset: OpenSSH Ed25519, optionally bcrypt-encrypted with bounded parameters.
pub(crate) use crate::ssh_private_key_decode::{DECODE_BUDGET, decode_bounded};
use crate::ssh_private_key_decode::{KEY_BYTES_LIMIT, PASSPHRASE_BYTES_LIMIT};
use pulse_app::vault_owner::{PrivateKeySourceConfirmation, PrivateKeyTargetSink, VaultOwnerError};
use pulse_ssh::{
    managed_store::validated_pins,
    peer::{NativeIoCompletion, PinnedTransport, VerifiedPeer},
    verify_host_key,
};
use pulse_store::ssh_key_target_source::SshKeyTargetSource;
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use uuid::Uuid;
use zeroize::Zeroizing;

fn spawn_decode(
    key: Zeroizing<Vec<u8>>,
    pass: Option<Zeroizing<Vec<u8>>>,
    permit: tokio::sync::OwnedSemaphorePermit,
    guard: Guard,
) -> tokio::task::JoinHandle<Result<PrivateKey>> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if !guard() {
            return Err(VaultOwnerError::Denied);
        }
        let result = decode_bounded(&key, pass.as_ref().map(|bytes| bytes.as_slice()));
        if !guard() {
            return Err(VaultOwnerError::Denied);
        }
        result
    })
}
#[path = "ssh_private_key_forward.rs"]
pub(crate) mod forward;
#[path = "ssh_private_key_interactive_adapter.rs"]
pub(crate) mod interactive;
#[path = "ssh_private_key_session_adapter.rs"]
pub(crate) mod session;
#[cfg(test)]
#[path = "ssh_private_key_adapter_tests.rs"]
mod tests;
type Guard = Arc<dyn Fn() -> bool + Send + Sync>;
type Result<T> = std::result::Result<T, VaultOwnerError>;
struct Admission {
    guard: Mutex<Option<Guard>>,
}
/// Owns the original transport plus decode work; dropping a waiter never proves their exit.
pub(crate) struct Sink {
    transport: Option<PinnedTransport>,
    peer: VerifiedPeer,
    selected: (Uuid, Uuid),
    admission: Arc<Admission>,
    completion: Option<NativeIoCompletion>,
    decode: Option<tokio::task::JoinHandle<Result<PrivateKey>>>,
    interactive: Option<Arc<crate::ssh_session_adapter::interactive::Broker>>,
}
pub(crate) fn peer_guard(
    source: &SshKeyTargetSource,
    host: Uuid,
    transport: &PinnedTransport,
) -> Result<Guard> {
    let peer = transport.peer().map_err(|_| VaultOwnerError::Denied)?;
    let row = source
        .hosts()
        .records()
        .iter()
        .find(|row| row.id() == host)
        .ok_or(VaultOwnerError::Denied)?;
    if row.endpoint().target().host() != peer.target().host()
        || row.endpoint().target().port() != peer.target().port()
    {
        return Err(VaultOwnerError::Denied);
    }
    let pins = validated_pins(source.trust()).map_err(|_| VaultOwnerError::Denied)?;
    verify_host_key(peer.target(), peer.public_key(), Some(&pins), None)
        .map_err(|_| VaultOwnerError::Denied)?;
    Ok(Arc::new(move || peer.is_current()))
}
impl Sink {
    pub(crate) async fn new_route(
        mut transport: PinnedTransport,
        selected: (Uuid, Uuid),
    ) -> std::result::Result<Self, bool> {
        let peer = match transport.peer() {
            Ok(peer) => peer,
            Err(_) => return Err(transport.close_and_join().await.is_ok()),
        };
        let admission = Arc::new(Admission {
            guard: Mutex::new(None),
        });
        let completion = match transport.retain_native_admission(&peer, admission.clone()) {
            Ok(done) => done,
            Err(_) => return Err(transport.close_and_join().await.is_ok()),
        };
        Ok(Self {
            transport: Some(transport),
            peer,
            selected,
            admission,
            completion: Some(completion),
            decode: None,
            interactive: None,
        })
    }
    pub(crate) fn new(mut transport: PinnedTransport, selected: (Uuid, Uuid)) -> Result<Self> {
        let peer = transport.peer().map_err(|_| VaultOwnerError::Denied)?;
        let admission = Arc::new(Admission {
            guard: Mutex::new(None),
        });
        let completion = transport
            .retain_native_admission(&peer, admission.clone())
            .map_err(|_| VaultOwnerError::Denied)?;
        Ok(Self {
            transport: Some(transport),
            peer,
            selected,
            admission,
            completion: Some(completion),
            decode: None,
            interactive: None,
        })
    }
}
impl PrivateKeyTargetSink for Sink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.selected == (host, identity)
            && source
                .identities()
                .records()
                .any(|row| row.id() == identity)
            && self.transport.as_ref().is_some_and(|transport| {
                peer_guard(source, host, transport).is_ok_and(|guard| guard())
            })
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        let mut admission = self
            .admission
            .guard
            .lock()
            .map_err(|_| VaultOwnerError::Failed)?;
        if admission.is_some() {
            return Err(VaultOwnerError::Denied);
        }
        *admission = Some(guard);
        Ok(())
    }
    fn authenticate<'a>(
        &'a mut self,
        username: &'a str,
        key: &'a [u8],
        passphrase: Option<&'a [u8]>,
        confirmation: &'a PrivateKeySourceConfirmation,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            if key.is_empty()
                || key.len() > KEY_BYTES_LIMIT
                || passphrase.is_some_and(|pass| pass.len() > PASSPHRASE_BYTES_LIMIT)
                || self.decode.is_some()
            {
                return Err(VaultOwnerError::Denied);
            }
            let guard = self
                .admission
                .guard
                .lock()
                .map_err(|_| VaultOwnerError::Failed)?
                .clone()
                .ok_or(VaultOwnerError::Denied)?;
            if !guard() {
                return Err(VaultOwnerError::Denied);
            }
            let permit = DECODE_BUDGET
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| VaultOwnerError::Failed)?;
            if !guard() {
                return Err(VaultOwnerError::Denied);
            }
            let owned_key = Zeroizing::new(key.to_vec());
            let owned_pass = passphrase.map(|pass| Zeroizing::new(pass.to_vec()));
            self.decode = Some(spawn_decode(owned_key, owned_pass, permit, guard.clone()));
            let decoded = self.decode.as_mut().ok_or(VaultOwnerError::Failed)?.await;
            // No await between consuming the result and clearing the completed handle.
            drop(self.decode.take());
            let key = decoded.map_err(|_| VaultOwnerError::Failed)??;
            confirmation.confirm().await?;
            if !guard() || !self.peer.is_current() {
                return Err(VaultOwnerError::Denied);
            }
            let transport = self.transport.as_mut().ok_or(VaultOwnerError::Denied)?;
            let client = transport
                .client(&self.peer)
                .map_err(|_| VaultOwnerError::Denied)?;
            let response = client
                .authenticate_publickey(username, PrivateKeyWithHashAlg::new(Arc::new(key), None))
                .await
                .map_err(|_| VaultOwnerError::Failed)?;
            let Some(broker) = &self.interactive else {
                return Ok(response.success());
            };
            eprintln!(
                "workspace.ssh_joint_partial_admitted.{}",
                interactive::requires_interactive(&response)
            );
            if !interactive::requires_interactive(&response) {
                return Ok(false);
            }
            confirmation.confirm().await?;
            eprintln!("workspace.ssh_joint_source_rechecked");
            crate::ssh_session_adapter::interactive::authenticate_client(
                transport,
                &self.peer,
                broker,
                username,
                guard,
                || confirmation.confirm(),
            )
            .await
        })
    }
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let mut outcome = Ok(());
            if let Some(transport) = self.transport.take()
                && transport.close_and_join().await.is_err()
            {
                outcome = Err(VaultOwnerError::Failed);
            }
            // Active blocking parsing retains its permit and original native guard until real exit.
            if let Some(decode) = self.decode.take()
                && decode.await.is_err()
            {
                outcome = Err(VaultOwnerError::Failed);
            }
            if let Some(completion) = self.completion.take()
                && completion.wait().await.is_err()
            {
                outcome = Err(VaultOwnerError::Failed);
            }
            let held = self
                .admission
                .guard
                .lock()
                .map_err(|_| VaultOwnerError::Failed)?
                .take();
            drop(held);
            outcome
        })
    }
}
