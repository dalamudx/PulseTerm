//! Explicit private-key auth-only action. Ordinary preparation never grants credential use.
use super::super::{PageVaultCancellation, PageVaultOperation, PageVaultOwner};
use super::{
    Result, SavedPrivateKeyUsePreparation, SshKeyTargetSource, StateStore, VaultOwnerError,
};
use crate::registry::PageLaunchScope;
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;
type NativeGuard = Arc<dyn Fn() -> bool + Send + Sync>;

#[path = "vault_key_target_session.rs"]
mod session;
pub use session::{
    PrivateKeyForwardSink, PrivateKeySessionAction, PrivateKeySessionIntent, PrivateKeySessionSink,
    PrivateKeySessionTask,
};
/// Audited Rust protocol adapter only; never implement from page/plugin/serialized callbacks.
/// It must verify the actual connection and exact complete trust source independently of this action.
pub trait PrivateKeyTargetSink: Send + 'static {
    /// Match actual peer/transport and saved host/identity, not a standalone fingerprint receipt.
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool;
    /// Retain authorization in actual native I/O and CPU work through real exit, including cancellation.
    fn retain_native_authorization(&mut self, guard: NativeGuard) -> Result<()>;
    /// Borrow only for bounded supported-key decoding and one explicit authentication.
    /// Passphrase decrypts this private key; it must never be used as an SSH password.
    /// Never log/cache/serialize/return plaintext. Uncertain network effects cannot be retried.
    fn authenticate<'a>(
        &'a mut self,
        username: &'a str,
        key: &'a [u8],
        passphrase: Option<&'a [u8]>,
        confirmation: &'a PrivateKeySourceConfirmation,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;
    /// Await actual transport and CPU exit on every failure or cancellation; no reusable connection is returned.
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}

/// Opaque original-source check lent only within a separately authorized private-key action.
/// Confirm after slow decoding and immediately before sending authentication; not a target-use permit.
/// ```compile_fail
/// fn log(c: pulse_app::vault_owner::PrivateKeySourceConfirmation) { println!("{c:?}"); }
/// ```
pub struct PrivateKeySourceConfirmation {
    store: StateStore,
    source: SshKeyTargetSource,
    guard: NativeGuard,
}
impl PrivateKeySourceConfirmation {
    /// Check the original store/source, page and key epoch without refreshing or upgrading them.
    pub async fn confirm(&self) -> Result<()> {
        if !(self.guard)() {
            return Err(VaultOwnerError::Denied);
        }
        self.store
            .validate_ssh_key_target_source(self.source.clone(), self.guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if (self.guard)() {
            Ok(())
        } else {
            Err(VaultOwnerError::Denied)
        }
    }
}
/// Trusted backend explicit-action admission, distinct from source reads and preparation.
pub struct PrivateKeyTargetAction {
    operation: PageVaultOperation,
    selected: (Uuid, Uuid),
    epoch: pulse_store::vault_lifecycle::AvailableKeyEpoch,
}
/// Single-use source/page/original-epoch action; no external constructor or plaintext getter.
/// ```compile_fail
/// fn upgrade(p: pulse_app::vault_owner::SavedPrivateKeyUsePreparation) -> pulse_app::vault_owner::PrivateKeyTargetUseIntent { p.into() }
/// ```
pub struct PrivateKeyTargetUseIntent {
    prepared: SavedPrivateKeyUsePreparation,
}
/// Key-free historical authentication result, delivered only after native resource drain.
pub struct PrivateKeyTargetUseTask {
    result: tokio::sync::oneshot::Receiver<Result<bool>>,
    cancel: PageVaultCancellation,
    complete: bool,
}
impl Drop for PrivateKeyTargetUseTask {
    fn drop(&mut self) {
        if !self.complete {
            self.cancel.cancel();
        }
    }
}
impl PrivateKeyTargetUseTask {
    /// Wait without owning cleanup; waiter cancellation cannot release active native/CPU credit early.
    pub async fn wait(mut self) -> Result<bool> {
        let result = (&mut self.result)
            .await
            .map_err(|_| VaultOwnerError::Failed)?;
        self.complete = true;
        result
    }
}
impl PageVaultOwner {
    /// Issue only after a separate trusted user decision and actual-peer authorization in the upper adapter.
    pub fn begin_private_key_target_action_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        guard: NativeGuard,
    ) -> Result<PrivateKeyTargetAction> {
        if host.is_nil() || identity.is_nil() {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .inner
            .guard()?
            .scope
            .clone()
            .ok_or(VaultOwnerError::Denied)?;
        let epoch = scope
            .available_epoch()
            .map_err(|_| VaultOwnerError::Denied)?;
        Ok(PrivateKeyTargetAction {
            operation: self.begin_authorized(page, guard)?,
            selected: (host, identity),
            epoch,
        })
    }
}
impl PrivateKeyTargetAction {
    /// Freeze both credential roles, original source, current page and available epoch once for five seconds.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<PrivateKeyTargetUseIntent> {
        Ok(PrivateKeyTargetUseIntent {
            prepared: self
                .operation
                .prepare_saved_private_key_use_pinned(
                    store,
                    source,
                    self.selected.0,
                    self.selected.1,
                    Some(self.epoch),
                )
                .await?,
        })
    }
}
impl PrivateKeyTargetUseIntent {
    /// Start one supervised auth-only loan. Result and credentials do not grant a session or input permission.
    pub fn start<S: PrivateKeyTargetSink>(
        self,
        store: StateStore,
        mut sink: S,
    ) -> Result<PrivateKeyTargetUseTask> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| VaultOwnerError::Denied)?;
        let guard = self.prepared.authorization();
        // Retain native credit even on stale-page/wrong-peer rejection; cleanup still owns the operation.
        let retained = sink.retain_native_authorization(guard.clone());
        let admission = if guard()
            && sink.matches_target(
                &self.prepared.source,
                self.prepared.selected.0,
                self.prepared.selected.1,
            ) {
            retained
        } else {
            Err(VaultOwnerError::Denied)
        };
        let cancel = self.prepared.operation.cancellation();
        let task_cancel = cancel.clone();
        let (tx, result) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let outcome = match admission {
                Ok(()) => self.run(&store, &mut sink, guard, &task_cancel).await,
                Err(error) => Err(error),
            };
            let drained = sink.close_and_drain().await;
            // Own close retires the peer guard. Delivery still requires the original page/token/key epoch.
            let p = &self.prepared;
            let delivery = p.operation.owner.upgrade().is_some_and(|owner| {
                let token = &p.operation.token;
                token.live.load(std::sync::atomic::Ordering::Acquire)
                    && owner.registry.check_launch_page(&token.page).is_ok()
                    && owner.guard().is_ok_and(|state| {
                        !state.closed
                            && state
                                .pending
                                .upgrade()
                                .is_some_and(|current| Arc::ptr_eq(&current, token))
                    })
            }) && std::time::Instant::now() < p.deadline
                && p.epoch.is_current_for(&p.scope).unwrap_or(false);
            let outcome = if delivery {
                drained.and(outcome)
            } else {
                Err(VaultOwnerError::Denied)
            };
            drop(sink);
            drop(self);
            let _sent = tx.send(outcome);
        });
        Ok(PrivateKeyTargetUseTask {
            result,
            cancel,
            complete: false,
        })
    }
    async fn run<S: PrivateKeyTargetSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        guard: NativeGuard,
        cancel: &PageVaultCancellation,
    ) -> Result<bool> {
        let p = &self.prepared;
        if !guard() {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_key_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let row = p
            .source
            .identities()
            .records()
            .find(|row| row.id() == p.selected.1)
            .ok_or(VaultOwnerError::Denied)?;
        let identity = row.identity();
        let key_reference = identity.private_key();
        let record = p
            .source
            .vault()
            .credentials()
            .iter()
            .find(|record| record.id() == key_reference.id())
            .ok_or(VaultOwnerError::Denied)?;
        let key = record
            .open_for_adapter_in_epoch(&p.scope, &p.epoch, key_reference.id(), "ssh_private_key")
            .map_err(|_| VaultOwnerError::Denied)?;
        let passphrase = match identity.passphrase() {
            Some(reference) => {
                let record = p
                    .source
                    .vault()
                    .credentials()
                    .iter()
                    .find(|record| record.id() == reference.id())
                    .ok_or(VaultOwnerError::Denied)?;
                let plain = record
                    .open_for_adapter_in_epoch(
                        &p.scope,
                        &p.epoch,
                        reference.id(),
                        "ssh_key_passphrase",
                    )
                    .map_err(|_| VaultOwnerError::Denied)?;
                if plain.expose().len() > 4096 {
                    return Err(VaultOwnerError::Denied);
                }
                Some(plain)
            }
            None => None,
        };
        // Recheck after both AEAD loans, without plaintext entering any SQL predicate.
        store
            .validate_ssh_key_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let confirmation = PrivateKeySourceConfirmation {
            store: store.clone(),
            source: p.source.clone(),
            guard: guard.clone(),
        };
        let outcome = tokio::select! { biased;
            _ = cancel.wait_cancelled() => Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(p.deadline)) => Err(VaultOwnerError::Denied),
            result = sink.authenticate(identity.username(), key.expose(), passphrase.as_ref().map(|value| value.expose()), &confirmation) => result,
        };
        drop(passphrase);
        drop(key);
        if !guard() {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_key_target_source(p.source.clone(), guard)
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        outcome
    }
}
