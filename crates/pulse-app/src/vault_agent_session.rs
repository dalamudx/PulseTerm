//! Independent selected-Agent action. No password/private-key intent or plaintext is issued.
use super::{Authorize, PageVaultOwner, PasswordSessionGesture, Result, VaultOwnerError};
use crate::{
    protocol_session_owner::{ProtocolSessionOwner, StartedProtocolSession},
    registry::PageLaunchScope,
    vault_owner::{AuthenticatedProtocolParts, PageVaultCancellation, PageVaultOperation},
};
use pulse_core::ssh_agent_identity::SshAgentIdentity;
use pulse_store::{
    StateStore,
    ssh_key_target_source::SshKeyTargetSource,
    vault_lifecycle::{AvailableKeyEpoch, VaultKeyLifecycle},
};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
/// Original-page and original-epoch selection; does not list keys or permit signing.
/// ```compile_fail
/// fn dump(v:pulse_app::vault_owner::AgentSessionGesture){println!("{v:?}");}
/// ```
pub struct AgentSessionGesture {
    pub(crate) capture: PasswordSessionGesture,
}
/// Explicit Agent action, separate from secret-read and either stored-secret authentication.
pub struct AgentSessionAction {
    operation: PageVaultOperation,
    selected: (Uuid, Uuid),
    epoch: AvailableKeyEpoch,
}
/// Frozen source and purpose. The trusted upper sink independently binds the actual peer.
/// ```compile_fail
/// fn send(v:pulse_app::vault_owner::AgentSessionIntent){let _=serde_json::to_vec(&v);}
/// ```
pub struct AgentSessionIntent {
    operation: PageVaultOperation,
    selected: (Uuid, Uuid),
    source: SshKeyTargetSource,
    scope: VaultKeyLifecycle,
    epoch: AvailableKeyEpoch,
    deadline: Instant,
}
/// Backend-only adapter for one selected key and one actual verified terminal peer.
/// Implementations MUST drain actual native resources before releasing a failed intent/reservation.
pub trait AgentSessionSink: Send + 'static {
    /// Compare exact metadata and actual-peer proof. Ordinary lookup is not sufficient.
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool;
    /// Carry operation credit until the actual native stream exits, including cancellation.
    fn retain_native_authorization(&mut self, guard: Authorize) -> Result<()>;
    /// Authenticate only this saved key. The implementation cannot try any fallback identity.
    fn authenticate_agent<'a>(
        &'a mut self,
        identity: &'a SshAgentIdentity,
        guard: Authorize,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;
    /// Prepare the authenticated leaf terminal under the same original guard.
    fn prepare_authenticated_terminal(
        &mut self,
        guard: Authorize,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    /// Move native parts only once, without returning an auth-only upgrade capability.
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts>;
}
/// Trusted intermediate route adapter. It must never request a PTY, shell or remote Agent forwarding.
pub trait AgentForwardSink: AgentSessionSink {
    /// Connection and target-constrained channel. Not an auth-only upgrade receipt.
    type Transport;
    /// Open only the next target derived from the frozen route source.
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        target: &'a pulse_core::ssh_target::SshTarget,
        guard: Authorize,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
    /// Single resource handoff after native replies. Failure remains owned by the caller for drain.
    fn take_authenticated_forward(&mut self) -> Result<Self::Transport>;
}
impl PageVaultOwner {
    /// Capture exact saved IDs before an independent GUI approval. No network or signing starts.
    pub fn begin_agent_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<AgentSessionGesture> {
        Ok(AgentSessionGesture {
            capture: self
                .begin_password_session_gesture_authorized(page, host, identity, original)?,
        })
    }
}
impl AgentSessionGesture {
    /// Metadata comparison only, never permission.
    pub fn matches_selection(&self, host: Uuid, identity: Uuid) -> bool {
        self.capture.matches_selection(host, identity)
    }
    /// Consume the original selection after an independently approved same-page action.
    pub fn begin_session_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        approved: Authorize,
    ) -> Result<AgentSessionAction> {
        let chosen = self.capture.approve_selection(owner, page, approved)?;
        Ok(AgentSessionAction {
            operation: owner.begin_authorized(chosen.page, chosen.guard)?,
            selected: chosen.selected,
            epoch: chosen.epoch,
        })
    }
}
impl AgentSessionAction {
    /// Revokes this attempt only and never establishes native completion.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.operation.cancellation()
    }
    /// Read/compare the joint source without loaning any Vault plaintext or signing.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<AgentSessionIntent> {
        self.prepare_at(store, source, Instant::now() + Duration::from_secs(5))
            .await
    }
    pub(super) async fn prepare_direct_interactive(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        deadline: Instant,
    ) -> Result<AgentSessionIntent> {
        if !source.supports_direct_agent_selection(self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        self.prepare_at(store, source, deadline).await
    }
    pub(super) async fn prepare_at(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        deadline: Instant,
    ) -> Result<AgentSessionIntent> {
        self.operation.current()?;
        if !source.supports_agent_selection(self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .operation
            .bind(source.vault().clone())?
            .ok_or(VaultOwnerError::Denied)?;
        if !self.epoch.is_current_for(&scope).unwrap_or(false) {
            return Err(VaultOwnerError::Denied);
        }
        let intent = AgentSessionIntent {
            operation: self.operation,
            selected: self.selected,
            source,
            scope,
            epoch: self.epoch,
            deadline,
        };
        let guard = intent.authorization();
        store
            .validate_ssh_key_target_source(intent.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(intent)
    }
}
impl AgentSessionIntent {
    /// Original page/epoch and bounded action lifetime, never a source or peer replacement.
    pub fn authorization(&self) -> Authorize {
        let page = self.operation.authorization();
        let scope = self.scope.clone();
        let epoch = self.epoch.clone();
        let live = self.source.target_liveness();
        let deadline = self.deadline;
        Arc::new(move || {
            page()
                && live()
                && Instant::now() < deadline
                && epoch.is_current_for(&scope).unwrap_or(false)
        })
    }
    /// Key-free exact selection and frozen backend data for the trusted adapter.
    pub fn selected_source(&self) -> (&SshKeyTargetSource, (Uuid, Uuid)) {
        (&self.source, self.selected)
    }
    /// Intermediate-only local Agent use. Trusted callers must drain their sink before dropping a failed intent.
    pub async fn authenticate_forward<S: AgentForwardSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
    ) -> Result<S::Transport> {
        let target = self
            .source
            .password_source()
            .forward_target()
            .ok_or(VaultOwnerError::Denied)?;
        let guard = self.authorization();
        let cancel = self.cancellation();
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        sink.retain_native_authorization(guard.clone())?;
        let identity = self
            .source
            .password_source()
            .agent_identities()
            .records()
            .find(|r| r.id() == self.selected.1)
            .ok_or(VaultOwnerError::Denied)?;
        let authenticate = sink.authenticate_agent(identity.identity(), guard.clone());
        let accepted = tokio::select! { biased;
            _ = cancel.wait_cancelled() => return Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline)) => return Err(VaultOwnerError::Denied),
            result = authenticate => result?,
        };
        if !accepted || !guard() {
            return Err(VaultOwnerError::Denied);
        }
        tokio::select! { biased;
            _ = cancel.wait_cancelled() => return Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline)) => return Err(VaultOwnerError::Denied),
            result = sink.prepare_authenticated_forward(target, guard.clone()) => result?,
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        sink.take_authenticated_forward()
    }
    /// Original bounded native action deadline; peer dialing cannot extend it.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Original action cancellation, not permission and not a native completion receipt.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.operation.cancellation()
    }
    /// Trusted driver only. It must keep this intent and drain its sink on any failure.
    /// The reserved original page is rechecked before signing and commit; lost cleanup stays charged.
    pub async fn authenticate_terminal<S: AgentSessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
    ) -> Result<ProtocolSessionOwner> {
        let guard = self.authorization();
        let cancel = self.cancellation();
        let reservation = started.as_ref().ok_or(VaultOwnerError::Denied)?;
        reservation
            .check_original_page(&self.operation.token.page)
            .map_err(|_| VaultOwnerError::Denied)?;
        if (self.source.password_source().is_route_bound()
            && !self.source.password_source().is_route_leaf())
            || !guard()
            || !sink.matches_target(&self.source, self.selected.0, self.selected.1)
        {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        sink.retain_native_authorization(guard.clone())?;
        let identity = self
            .source
            .password_source()
            .agent_identities()
            .records()
            .find(|row| row.id() == self.selected.1)
            .ok_or(VaultOwnerError::Denied)?
            .identity();
        let outcome = tokio::select! {biased;_=cancel.wait_cancelled()=>Err(VaultOwnerError::Denied),_=tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline))=>Err(VaultOwnerError::Denied),result=sink.authenticate_agent(identity,guard.clone())=>result};
        if !outcome? {
            return Err(VaultOwnerError::Denied);
        }
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let setup = tokio::select! {biased;_=cancel.wait_cancelled()=>Err(VaultOwnerError::Denied),_=tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline))=>Err(VaultOwnerError::Denied),result=sink.prepare_authenticated_terminal(guard.clone())=>result};
        setup?;
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        started
            .as_ref()
            .ok_or(VaultOwnerError::Denied)?
            .check_original_page(&self.operation.token.page)
            .map_err(|_| VaultOwnerError::Denied)?;
        let registry = started
            .as_ref()
            .ok_or(VaultOwnerError::Denied)?
            .source_registry()
            .clone();
        let owner = sink
            .take_authenticated_root_parts()?
            .commit(started.take().ok_or(VaultOwnerError::Denied)?)
            .map_err(|_| VaultOwnerError::Denied)?;
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !owner.is_managed_by(&registry) {
            return Err(VaultOwnerError::Denied);
        }
        Ok(owner)
    }
}
