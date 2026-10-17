//! Explicit private-key terminal action; auth-only results cannot become session authority.
use super::{
    NativeGuard, PageVaultCancellation, PageVaultOwner, PrivateKeyTargetAction,
    PrivateKeyTargetSink, PrivateKeyTargetUseIntent, Result, SshKeyTargetSource, StateStore,
    VaultOwnerError,
};
use crate::{
    protocol_session_owner::{ProtocolSessionError, ProtocolSessionOwner, StartedProtocolSession},
    registry::PageLaunchScope,
    vault_owner::AuthenticatedProtocolParts,
};
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;

/// Trusted protocol implementation only. Credentials must remain inside the audited Rust adapter.
pub trait PrivateKeySessionSink: PrivateKeyTargetSink {
    /// Set up the authenticated peer while retaining original native authorization.
    fn prepare_authenticated_terminal(
        &mut self,
        authorization: NativeGuard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    /// Transfer actor and all physical I/O joins once. No remaining plaintext or lifetime-credit release.
    /// Subsequent cleanup drains only leftovers, never the moved root resources.
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts>;
}
/// Trusted intermediate-hop adapter. A successful auth-only receipt cannot invoke this purpose.
pub trait PrivateKeyForwardSink: PrivateKeyTargetSink {
    /// Owned physical connection and exact-target channel; contains no plaintext.
    type Transport: Send + 'static;
    /// Open only the frozen next-hop target, under the original native guard.
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        target: &'a pulse_core::ssh_target::SshTarget,
        authorization: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
    /// Move the completed connection/channel once, clearing the finished credential guard.
    /// The caller must supervise physical cleanup on every failure or lost delivery.
    fn take_authenticated_forward(&mut self) -> Result<Self::Transport>;
}
/// A separate backend user gesture, not a source confirmation or successful authentication receipt.
pub struct PrivateKeySessionAction {
    action: PrivateKeyTargetAction,
}
/// Frozen original page, complete source and epoch; no external constructor or plaintext getter.
/// ```compile_fail
/// fn upgrade(v:pulse_app::vault_owner::PrivateKeyTargetUseIntent)->pulse_app::vault_owner::PrivateKeySessionIntent{v.into()}
/// ```
pub struct PrivateKeySessionIntent {
    intent: PrivateKeyTargetUseIntent,
}
/// Waiter cancellation revokes startup but never cancels mandatory native cleanup.
pub struct PrivateKeySessionTask {
    result: tokio::sync::oneshot::Receiver<Result<ProtocolSessionOwner>>,
    cancel: PageVaultCancellation,
    complete: bool,
}
impl Drop for PrivateKeySessionTask {
    fn drop(&mut self) {
        if !self.complete {
            self.cancel.cancel();
        }
    }
}
impl PrivateKeySessionTask {
    /// Return a supervised root owner; a lost result still reclaims all native resources.
    pub async fn wait(mut self) -> Result<ProtocolSessionOwner> {
        let result = (&mut self.result)
            .await
            .map_err(|_| VaultOwnerError::Failed)?;
        self.complete = true;
        result
    }
}
impl PageVaultOwner {
    /// Trusted explicit session admission. The upper layer must authorize the actual peer and user action.
    pub fn begin_private_key_session_action_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        guard: NativeGuard,
    ) -> Result<PrivateKeySessionAction> {
        Ok(PrivateKeySessionAction {
            action: self.begin_private_key_target_action_authorized(page, host, identity, guard)?,
        })
    }
}
impl PrivateKeySessionAction {
    // Keep only the original key-free gesture epoch; do not capture a later unlock.
    pub(crate) fn pin_original_epoch(
        mut self,
        epoch: pulse_store::vault_lifecycle::AvailableKeyEpoch,
    ) -> Self {
        self.action.epoch = epoch;
        self
    }
    /// Revoke only this original attempt; not permission to use any credential.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.action.operation.cancellation()
    }
    /// Freeze both credential roles and their original source/epoch without later refresh or upgrade.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<PrivateKeySessionIntent> {
        Ok(PrivateKeySessionIntent {
            intent: self.action.prepare(store, source).await?,
        })
    }
    /// Crate-private preparation for an explicitly captured direct joint action.
    pub(crate) async fn prepare_interactive(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        deadline: std::time::Instant,
    ) -> Result<PrivateKeySessionIntent> {
        self.prepare_interactive_source(store, source, deadline, false)
            .await
    }
    /// Prepare only a route-bound joint capture, preserving its original expiry.
    pub(crate) async fn prepare_interactive_route(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        deadline: std::time::Instant,
    ) -> Result<PrivateKeySessionIntent> {
        self.prepare_interactive_source(store, source, deadline, true)
            .await
    }
    async fn prepare_interactive_source(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        deadline: std::time::Instant,
        route_bound: bool,
    ) -> Result<PrivateKeySessionIntent> {
        if source.password_source().is_route_bound() != route_bound
            || std::time::Instant::now() >= deadline
        {
            return Err(VaultOwnerError::Denied);
        }
        let action = self.action;
        let prepared = action
            .operation
            .prepare_saved_private_key_use_pinned_until(
                store,
                source,
                action.selected.0,
                action.selected.1,
                Some(action.epoch),
                deadline,
            )
            .await?;
        Ok(PrivateKeySessionIntent {
            intent: PrivateKeyTargetUseIntent { prepared },
        })
    }
}
impl PrivateKeySessionIntent {
    /// Intermediate route use only. The trusted caller owns the sink and MUST drain it on failure
    /// before dropping this intent. Dropping a wait never establishes native completion.
    pub async fn authenticate_forward<S: PrivateKeyForwardSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
    ) -> Result<S::Transport> {
        let p = &self.intent.prepared;
        let common = p.source.password_source();
        let target = common.forward_target().ok_or(VaultOwnerError::Denied)?;
        let guard = p.authorization();
        let cancel = p.operation.cancellation();
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        sink.retain_native_authorization(guard.clone())?;
        if !self.intent.run(store, sink, guard.clone(), &cancel).await? {
            return Err(VaultOwnerError::Denied);
        }
        tokio::select! { biased;
            _ = cancel.wait_cancelled() => return Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(p.deadline)) => return Err(VaultOwnerError::Denied),
            result = sink.prepare_authenticated_forward(target, guard.clone()) => result?,
        }
        store
            .validate_ssh_key_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        sink.take_authenticated_forward()
    }

    /// Dialing metadata only; neither it nor a verified fingerprint authorizes credential use.
    pub fn selected_source(&self) -> (&SshKeyTargetSource, (Uuid, Uuid)) {
        (&self.intent.prepared.source, self.intent.prepared.selected)
    }
    /// Original key-free predicate; cannot refresh a page, source or key epoch.
    pub fn authorization(&self) -> NativeGuard {
        self.intent.prepared.authorization()
    }
    /// Revoke the pending attempt without touching a newer epoch or abandoning cleanup.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.intent.prepared.operation.cancellation()
    }
    /// Original five-second startup deadline; neither dialing nor setup renews it.
    pub fn deadline(&self) -> std::time::Instant {
        self.intent.prepared.deadline
    }
    /// Consume the same original page's protocol reservation, never a new page or foreign registry.
    pub fn start<S: PrivateKeySessionSink>(
        self,
        store: StateStore,
        mut sink: S,
        started: StartedProtocolSession,
    ) -> PrivateKeySessionTask {
        let runtime = started.root_runtime();
        let cancel = self.intent.prepared.operation.cancellation();
        let task_cancel = cancel.clone();
        let (sender, result) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let guard = self.intent.prepared.authorization();
            let mut started = Some(started);
            let outcome = self
                .establish(&store, &mut sink, &mut started, guard.clone(), &task_cancel)
                .await;
            let result = match outcome {
                Ok(owner) => {
                    let drained = sink.close_and_drain().await;
                    if drained.is_ok() && guard() {
                        Ok(owner)
                    } else {
                        drop(owner);
                        Err(VaultOwnerError::Denied)
                    }
                }
                Err(error) => {
                    task_cancel.cancel();
                    if let Some(started) = started.take() {
                        let cleanup = started.finish_unregistered(async move {
                            let result = sink.close_and_drain().await;
                            drop(self);
                            result.map_err(|_| ProtocolSessionError::NativeUnknown)
                        });
                        if cleanup.join().await.is_err() {
                            Err(VaultOwnerError::Failed)
                        } else {
                            Err(error)
                        }
                    } else {
                        // A rejected commit already supervises the moved native parts.
                        let drained = sink.close_and_drain().await;
                        if drained.is_err() {
                            Err(VaultOwnerError::Failed)
                        } else {
                            Err(error)
                        }
                    }
                }
            };
            let _sent = sender.send(result);
        });
        PrivateKeySessionTask {
            result,
            cancel,
            complete: false,
        }
    }
    /// Trusted routed driver: keep the original outer native reservation on failure until
    /// the caller drains every connection. This rejects direct and foreign-page sources.
    pub async fn authenticate_route_terminal<S: PrivateKeySessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
    ) -> Result<ProtocolSessionOwner> {
        let p = &self.intent.prepared;
        if !p.source.password_source().is_route_leaf() {
            return Err(VaultOwnerError::Denied);
        }
        let guard = p.authorization();
        let cancel = p.operation.cancellation();
        self.establish(store, sink, started, guard, &cancel).await
    }
    async fn establish<S: PrivateKeySessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
        guard: NativeGuard,
        cancel: &PageVaultCancellation,
    ) -> Result<ProtocolSessionOwner> {
        let p = &self.intent.prepared;
        let page = &p.operation.token.page;
        // Keep native authorization even when this page, reservation or peer is rejected.
        sink.retain_native_authorization(guard.clone())?;
        started
            .as_ref()
            .ok_or(VaultOwnerError::Denied)?
            .check_original_page(page)
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        if !self.intent.run(store, sink, guard.clone(), cancel).await? {
            return Err(VaultOwnerError::Denied);
        }
        let prepared = tokio::select! { biased;
            _ = cancel.wait_cancelled() => Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(p.deadline)) => Err(VaultOwnerError::Denied),
            result = sink.prepare_authenticated_terminal(guard.clone()) => result,
        };
        prepared?;
        store
            .validate_ssh_key_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let original = started.as_ref().ok_or(VaultOwnerError::Denied)?;
        original
            .check_original_page(page)
            .map_err(|_| VaultOwnerError::Denied)?;
        let registry = original.source_registry().clone();
        let parts = sink.take_authenticated_root_parts()?;
        let owner = parts
            .commit(started.take().ok_or(VaultOwnerError::Denied)?)
            .map_err(|_| VaultOwnerError::Denied)?;
        // SQL-to-network confirmation is a point check, not an atomic distributed transaction.
        store
            .validate_ssh_key_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !owner.is_managed_by(&registry) {
            return Err(VaultOwnerError::Denied);
        }
        Ok(owner)
    }
}
