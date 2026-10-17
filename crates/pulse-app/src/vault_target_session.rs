//! Separate trusted backend session action. Auth-only receipts cannot convert into this intent.
use super::*;
use crate::protocol_session_owner::{
    ProtocolSessionError, ProtocolSessionOwner, StartedProtocolSession,
};
use pulse_term::session::SessionHandle;

/// Owned actor and physical native join, supplied ONLY by a trusted protocol adapter.
/// No secret, serialized callback or frontend completion can be placed in this boundary.
pub struct AuthenticatedProtocolParts {
    handle: SessionHandle,
    appearance: pulse_store::profile_editor::ProfileAppearance,
    native_join:
        Pin<Box<dyn Future<Output = std::result::Result<(), ProtocolSessionError>> + Send>>,
}
impl AuthenticatedProtocolParts {
    /// The future must own and join all actor and transport I/O, not just await a close/send receipt.
    pub fn new<F>(handle: SessionHandle, native_join: F) -> Self
    where
        F: Future<Output = std::result::Result<(), ProtocolSessionError>> + Send + 'static,
    {
        Self {
            handle,
            appearance: Default::default(),
            native_join: Box::pin(native_join),
        }
    }
    /// Trusted adapter-only frozen display data; grants no session or input permission.
    pub fn with_appearance(
        mut self,
        appearance: pulse_store::profile_editor::ProfileAppearance,
    ) -> Self {
        self.appearance = appearance;
        self
    }
    /// Trusted route adapter: retain every parent until the leaf's actor and native stream exit.
    /// Always drain the tail, even when the leaf could not prove successful completion.
    pub fn with_native_tail<F>(self, tail: F) -> Self
    where
        F: Future<Output = std::result::Result<(), ProtocolSessionError>> + Send + 'static,
    {
        let join = self.native_join;
        Self::new(self.handle, async move {
            let leaf = join.await;
            let parents = tail.await;
            leaf.and(parents)
        })
        .with_appearance(self.appearance)
    }
    pub(crate) fn commit(
        self,
        started: StartedProtocolSession,
    ) -> std::result::Result<ProtocolSessionOwner, ProtocolSessionError> {
        started
            .commit(self.handle, self.native_join)
            .map(|owner| owner.with_appearance(self.appearance))
    }
}
/// Trusted upper-layer protocol implementation; never implement from a plugin/page callback.
pub trait PasswordSessionSink: PasswordTargetSink {
    /// Set up this authenticated peer's terminal while retaining original native authorization.
    /// Permission revocation/deadline cancels the wait, not the mandatory physical cleanup.
    fn prepare_authenticated_terminal(
        &mut self,
        authorization: NativeGuard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    /// Move the already-authorized actor/transport into root-owned native parts, once only.
    /// Authentication/setup replies must have completed before releasing their original guard.
    /// Persistent transport admission moves with native_join; this must NOT release its lifetime quota.
    /// Keep no plaintext. Subsequent close_and_drain drains only leftovers, never the moved root parts.
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts>;
}
/// Trusted intermediate-hop adapter. A successful auth-only receipt cannot invoke this purpose.
pub trait PasswordForwardSink: PasswordTargetSink {
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
/// Explicit backend session gesture; no conversion from a control/source/auth boolean receipt.
pub struct PasswordSessionAction {
    action: PasswordTargetAction,
}
/// Original source/page/epoch/selection session intent, with no external constructor or secret getter.
/// ```compile_fail
/// fn upgrade(v:pulse_app::vault_owner::PasswordTargetUseIntent)->pulse_app::vault_owner::PasswordSessionIntent{v.into()}
/// ```
pub struct PasswordSessionIntent {
    intent: PasswordTargetUseIntent,
}
/// Backend-only root owner waiter. Drop revokes pending work but never aborts cleanup.
pub struct PasswordSessionTask {
    result: tokio::sync::oneshot::Receiver<Result<ProtocolSessionOwner>>,
    cancel: PageVaultCancellation,
    complete: bool,
}
impl Drop for PasswordSessionTask {
    fn drop(&mut self) {
        if !self.complete {
            self.cancel.cancel();
        }
    }
}
impl PasswordSessionTask {
    /// Receive a root owner, never plaintext or a frontend session capability.
    /// A lost result drops the owner into supervised native reclamation.
    pub async fn wait(mut self) -> Result<ProtocolSessionOwner> {
        let result = (&mut self.result)
            .await
            .map_err(|_| VaultOwnerError::Failed)?;
        self.complete = true;
        result
    }
}
impl PageVaultOwner {
    /// Explicit trusted backend session admission, distinct from read/control/auth-only validation.
    /// The upper adapter must independently authorize the user action and actual peer/trust.
    pub fn begin_password_session_action_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        guard: NativeGuard,
    ) -> Result<PasswordSessionAction> {
        Ok(PasswordSessionAction {
            action: self.begin_password_target_action_authorized(page, host, identity, guard)?,
        })
    }
}
impl PasswordSessionAction {
    pub(crate) fn pin_original_epoch(
        mut self,
        epoch: pulse_store::vault_lifecycle::AvailableKeyEpoch,
    ) -> Self {
        self.action.pinned_epoch = Some(epoch);
        self
    }
    /// Key-free revocation of this original action before source I/O, not use permission.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.action.operation.cancellation()
    }
    /// Freeze original page/role/source/epoch/identity once; a later unlock cannot upgrade it.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshTargetSource,
    ) -> Result<PasswordSessionIntent> {
        Ok(PasswordSessionIntent {
            intent: self.action.prepare(store, source).await?,
        })
    }
}
impl PasswordSessionIntent {
    /// Intermediate route use only. The trusted caller owns the sink and MUST drain it on failure
    /// before dropping this intent. Dropping a wait never establishes native completion.
    pub async fn authenticate_forward<S: PasswordForwardSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
    ) -> Result<S::Transport> {
        let p = &self.intent.prepared;
        let common = &p.source;
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
            .validate_ssh_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        sink.take_authenticated_forward()
    }

    /// Backend-only frozen metadata for dialing; data does not authorize credentials or a peer.
    pub fn selected_source(&self) -> (&SshTargetSource, (Uuid, Uuid)) {
        (&self.intent.prepared.source, self.intent.prepared.selected)
    }
    /// Original key-free predicate; cannot upgrade an old source/page/epoch or replace peer checks.
    pub fn authorization(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        self.intent.prepared.authorization()
    }
    /// Revoke this attempt only, without aborting native cleanup or touching a newer epoch.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.intent.prepared.operation.cancellation()
    }
    /// Original five-second use deadline; a new dial or UI receipt cannot extend it.
    pub fn deadline(&self) -> std::time::Instant {
        self.intent.prepared.deadline
    }
    /// Start a supervised session action using the SAME original page's protocol reservation.
    /// No pending startup or transferred connection becomes a root session on a foreign/new page.
    pub fn start<S: PasswordSessionSink>(
        self,
        store: StateStore,
        mut sink: S,
        started: StartedProtocolSession,
    ) -> PasswordSessionTask {
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
                    // Only leftover setup/auth resources remain in sink after successful transfer.
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
                        // Retain the whole original intent through actual native cleanup.
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
                        // Rejected registry commit already supervises the moved owned native parts.
                        let drained = sink.close_and_drain().await;
                        if drained.is_err() {
                            Err(VaultOwnerError::Failed)
                        } else {
                            Err(error)
                        }
                    }
                }
            };
            // Sending an owner into a closed receiver drops it; cleanup remains root-supervised.
            let _ = sender.send(result);
        });
        PasswordSessionTask {
            result,
            cancel,
            complete: false,
        }
    }
    /// Trusted routed driver: keep the original outer native reservation on failure until
    /// the caller drains every connection. This rejects direct and foreign-page sources.
    pub async fn authenticate_route_terminal<S: PasswordSessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
    ) -> Result<ProtocolSessionOwner> {
        let p = &self.intent.prepared;
        if !p.source.is_route_leaf() {
            return Err(VaultOwnerError::Denied);
        }
        let guard = p.authorization();
        let cancel = p.operation.cancellation();
        self.establish(store, sink, started, guard, &cancel).await
    }
    async fn establish<S: PasswordSessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
        guard: NativeGuard,
        cancel: &PageVaultCancellation,
    ) -> Result<ProtocolSessionOwner> {
        let p = &self.intent.prepared;
        let page = &p.operation.token.page;
        started
            .as_ref()
            .ok_or(VaultOwnerError::Denied)?
            .check_original_page(page)
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        sink.retain_native_authorization(guard.clone())?;
        if !self.intent.run(store, sink, guard.clone(), cancel).await? {
            return Err(VaultOwnerError::Denied);
        }
        let prepared = tokio::select! {
            biased;
            _ = cancel.wait_cancelled() => Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(p.deadline)) => Err(VaultOwnerError::Denied),
            result = sink.prepare_authenticated_terminal(guard.clone()) => result,
        };
        prepared?;
        store
            .validate_ssh_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&p.source, p.selected.0, p.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let started_ref = started.as_ref().ok_or(VaultOwnerError::Denied)?;
        started_ref
            .check_original_page(page)
            .map_err(|_| VaultOwnerError::Denied)?;
        let source_registry = started_ref.source_registry().clone();
        let parts = sink.take_authenticated_root_parts()?;
        let owner = started
            .take()
            .ok_or(VaultOwnerError::Denied)?
            .commit(parts.handle, parts.native_join)
            .map_err(|_| VaultOwnerError::Denied)?;
        // Source confirmation is a point check, NOT an atomic SQL-to-network transaction.
        // A provisional owner cannot be published after a retired epoch/page/source result.
        store
            .validate_ssh_target_source(p.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !owner.is_managed_by(&source_registry) {
            return Err(VaultOwnerError::Denied);
        }
        Ok(owner)
    }
}
