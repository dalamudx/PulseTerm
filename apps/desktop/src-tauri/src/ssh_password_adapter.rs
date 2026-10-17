//! Backend-only credential borrowing on an actual pinned connection; no secret getter or UI callback.
use pulse_app::vault_owner::{PasswordTargetSink, VaultOwnerError};
use pulse_ssh::{
    managed_store::validated_pins,
    peer::{NativeIoCompletion, PinnedTransport, VerifiedPeer},
    verify_host_key,
};
use pulse_store::ssh_target_source::SshTargetSource;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use uuid::Uuid;
#[path = "ssh_password_forward.rs"]
pub(crate) mod forward;
pub(super) type Guard = Arc<dyn Fn() -> bool + Send + Sync>;
pub(crate) struct Admission {
    pub(super) guard: Mutex<Option<Guard>>,
}
pub(super) struct Sink {
    pub(super) transport: Option<PinnedTransport>,
    pub(super) peer: VerifiedPeer,
    selected: (Uuid, Uuid),
    pub(super) admission: Arc<Admission>,
    pub(super) completion: Option<NativeIoCompletion>,
}
pub(crate) fn peer_guard(
    source: &SshTargetSource,
    host: Uuid,
    transport: &PinnedTransport,
) -> Result<Guard, VaultOwnerError> {
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
        })
    }
    pub(super) fn new(
        mut transport: PinnedTransport,
        selected: (Uuid, Uuid),
    ) -> Result<Self, VaultOwnerError> {
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
        })
    }
}
impl PasswordTargetSink for Sink {
    fn matches_target(&self, source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.selected == (host, identity)
            && source
                .identity_vault()
                .identities()
                .records()
                .any(|row| row.id() == identity)
            && self
                .transport
                .as_ref()
                .is_some_and(|t| peer_guard(source, host, t).is_ok_and(|guard| guard()))
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<(), VaultOwnerError> {
        let mut state = self
            .admission
            .guard
            .lock()
            .map_err(|_| VaultOwnerError::Failed)?;
        if state.is_some() {
            return Err(VaultOwnerError::Denied);
        }
        *state = Some(guard);
        Ok(())
    }
    fn authenticate<'a>(
        &'a mut self,
        user: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<bool, VaultOwnerError>> + Send + 'a>> {
        Box::pin(async move {
            let transport = self.transport.as_mut().ok_or(VaultOwnerError::Denied)?;
            let client = transport
                .client(&self.peer)
                .map_err(|_| VaultOwnerError::Denied)?;
            Ok(client
                .authenticate_password(user, password)
                .await
                .map_err(|_| VaultOwnerError::Failed)?
                .success())
        })
    }
    fn close_and_drain(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), VaultOwnerError>> + Send + '_>> {
        Box::pin(async move {
            if let Some(transport) = self.transport.take() {
                transport
                    .close_and_join()
                    .await
                    .map_err(|_| VaultOwnerError::Failed)?;
            }
            if let Some(completion) = self.completion.take() {
                completion
                    .wait()
                    .await
                    .map_err(|_| VaultOwnerError::Failed)?;
            }
            let held = self
                .admission
                .guard
                .lock()
                .map_err(|_| VaultOwnerError::Failed)?
                .take();
            drop(held);
            Ok(())
        })
    }
}
