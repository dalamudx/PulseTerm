//! Explicit backend target action, separate from read-only source preparation. No network implementation here.
use super::super::{PageVaultCancellation, PageVaultOperation, PageVaultOwner};
use super::{Result, SavedPasswordUsePreparation, SshTargetSource, StateStore, VaultOwnerError};
use crate::registry::PageLaunchScope;
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;
#[path = "vault_target_session.rs"]
mod session;
pub use session::{
    AuthenticatedProtocolParts, PasswordForwardSink, PasswordSessionAction, PasswordSessionIntent,
    PasswordSessionSink, PasswordSessionTask,
};
type NativeGuard = Arc<dyn Fn() -> bool + Send + Sync>;
/// Trusted Rust protocol implementation only. Never implement from a frontend/plugin/serialized callback.
/// Match the exact actual peer/connection/complete trust source and selected identity independently.
/// Native guard retention and drain must follow actual I/O, not just this method's waiting future.
pub trait PasswordTargetSink: Send + 'static {
    /// Verify this particular actual transport and complete trust source against the fixed selection.
    fn matches_target(&self, source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool;
    /// Retain original authorization in native resources through reply completion or actual cancellation exit.
    /// Must survive caller/runtime cancellation; cannot keep it only in an async supervisor future.
    fn retain_native_authorization(&mut self, guard: NativeGuard) -> Result<()>;
    /// Borrow bytes only for this explicit authentication; never return/log/cache/serialize plaintext.
    /// SDK admission/reply is not atomic with SQL; uncertain sends cannot be retried.
    fn authenticate<'a>(
        &'a mut self,
        username: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;
    /// Close this validation action's transport and await actual native resource exit, even on rejection/cancel.
    /// This auth-only boundary does not register an SSH session or return a reusable connection.
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}
/// Explicit native action issuer; ordinary control/read-only preparations cannot convert into it.
/// The caller must first authorize the user action and actual peer/trust in the trusted upper adapter.
pub struct PasswordTargetAction {
    operation: PageVaultOperation,
    selected: (Uuid, Uuid),
    pinned_epoch: Option<pulse_store::vault_lifecycle::AvailableKeyEpoch>,
}
/// One original-source/page/epoch/selection action, with no external constructor or plaintext getter.
/// ```compile_fail
/// fn upgrade(p:pulse_app::vault_owner::SavedPasswordUsePreparation)->pulse_app::vault_owner::PasswordTargetUseIntent{p.into()}
/// ```
pub struct PasswordTargetUseIntent {
    prepared: SavedPasswordUsePreparation,
}
/// Key-free boolean result waiter; dropping it cancels authority but not the supervising native cleanup.
pub struct PasswordTargetUseTask {
    result: tokio::sync::oneshot::Receiver<Result<bool>>,
    cancel: PageVaultCancellation,
    complete: bool,
}
impl Drop for PasswordTargetUseTask {
    fn drop(&mut self) {
        if !self.complete {
            self.cancel.cancel();
        }
    }
}
impl PasswordTargetUseTask {
    /// Authentication is delivered only after transport native drain; no secret/session permission returned.
    pub async fn wait(mut self) -> Result<bool> {
        let result = (&mut self.result)
            .await
            .map_err(|_| VaultOwnerError::Failed)?;
        self.complete = true;
        result
    }
}
impl PageVaultOwner {
    /// Trusted backend explicit target-action admission, never a frontend-provided guard or SecretRef.
    /// It shares bounded operation capacity and current-page provenance, but is distinct from control/read APIs.
    pub fn begin_password_target_action_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        guard: NativeGuard,
    ) -> Result<PasswordTargetAction> {
        if host.is_nil() || identity.is_nil() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(PasswordTargetAction {
            operation: self.begin_authorized(page, guard)?,
            selected: (host, identity),
            pinned_epoch: None,
        })
    }
}
impl PasswordTargetAction {
    /// Freeze original role/Vault/source/epoch/selection and deadline once. Later unlock cannot upgrade it.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshTargetSource,
    ) -> Result<PasswordTargetUseIntent> {
        let prepared = self
            .operation
            .prepare_saved_password_use_pinned(
                store,
                source,
                self.selected.0,
                self.selected.1,
                self.pinned_epoch,
            )
            .await?;
        Ok(PasswordTargetUseIntent { prepared })
    }
}
impl PasswordTargetUseIntent {
    /// Start one supervised Rust-only auth action. Waiter cancellation never aborts the cleanup task.
    /// Sink retention must bind actual native I/O before any plaintext exists. No plaintext is returned.
    pub fn start<S: PasswordTargetSink>(
        self,
        store: StateStore,
        mut sink: S,
    ) -> Result<PasswordTargetUseTask> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| VaultOwnerError::Denied)?;
        let guard = self.prepared.authorization();
        if !guard()
            || !sink.matches_target(
                &self.prepared.source,
                self.prepared.selected.0,
                self.prepared.selected.1,
            )
        {
            return Err(VaultOwnerError::Denied);
        }
        sink.retain_native_authorization(guard.clone())?;
        let cancel = self.prepared.operation.cancellation();
        let task_cancel = cancel.clone();
        let (tx, result) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let outcome = self.run(&store, &mut sink, guard, &task_cancel).await;
            // Always drain, including pre-decrypt/source/authentication failures. No password reaches SQL.
            let drained = sink.close_and_drain().await;
            let delivery = self
                .prepared
                .operation
                .owner
                .upgrade()
                .is_some_and(|owner| {
                    let token = &self.prepared.operation.token;
                    token.live.load(std::sync::atomic::Ordering::Acquire)
                        && owner.registry.check_launch_page(&token.page).is_ok()
                        && owner.guard().is_ok_and(|state| {
                            !state.closed
                                && state
                                    .pending
                                    .upgrade()
                                    .is_some_and(|current| Arc::ptr_eq(&current, token))
                        })
                })
                && std::time::Instant::now() < self.prepared.deadline
                && self
                    .prepared
                    .epoch
                    .is_current_for(&self.prepared.scope)
                    .unwrap_or(false);
            // Historical auth-only result after known native closure, never a new peer/use capability.
            // Peer guard is intentionally not re-used after own close; page/token/original epoch still gate delivery.
            let outcome = if delivery {
                drained.and(outcome)
            } else {
                Err(VaultOwnerError::Denied)
            };
            drop(sink);
            drop(self);
            let _sent = tx.send(outcome);
        });
        Ok(PasswordTargetUseTask {
            result,
            cancel,
            complete: false,
        })
    }
    async fn run<S: PasswordTargetSink>(
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
            .validate_ssh_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let row = p
            .source
            .identity_vault()
            .identities()
            .records()
            .find(|row| row.id() == p.selected.1)
            .ok_or(VaultOwnerError::Denied)?;
        let reference = row.identity().password();
        let record = p
            .source
            .identity_vault()
            .vault()
            .credentials()
            .iter()
            .find(|record| record.id() == reference.id())
            .ok_or(VaultOwnerError::Denied)?;
        let plain = record
            .open_for_adapter_in_epoch(&p.scope, &p.epoch, reference.id(), "ssh_password")
            .map_err(|_| VaultOwnerError::Denied)?;
        if plain.expose().len() > 4096 {
            return Err(VaultOwnerError::Denied);
        }
        let password = std::str::from_utf8(plain.expose()).map_err(|_| VaultOwnerError::Denied)?;
        // Recheck exact source after authenticated decoding, without copying plaintext into a SQL predicate.
        store
            .validate_ssh_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let outcome = tokio::select! {biased;_ = cancel.wait_cancelled()=>Err(VaultOwnerError::Denied),_ = tokio::time::sleep_until(tokio::time::Instant::from_std(p.deadline))=>Err(VaultOwnerError::Denied),result = sink.authenticate(row.identity().username(),password)=>result};
        drop(plain);
        if !guard() {
            return Err(VaultOwnerError::Denied);
        }
        // A point grant is not an ongoing source lock or an atomic SQL-to-network transaction.
        store
            .validate_ssh_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        outcome
    }
}
