//! Explicit keyboard-interactive metadata intent. Never borrows or returns a stored secret.
use super::{Authorize, PageVaultOwner, PasswordSessionGesture, Result, VaultOwnerError};
use crate::{
    protocol_session_owner::{ProtocolSessionOwner, StartedProtocolSession},
    registry::PageLaunchScope,
    vault_owner::{AuthenticatedProtocolParts, PageVaultCancellation, PageVaultOperation},
};
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
/// Original page, exact account and original available epoch. No credential authority.
#[must_use]
pub struct InteractiveSessionGesture {
    capture: PasswordSessionGesture,
}
/// A separately approved interactive action. Cannot be used as a password or key action.
pub struct InteractiveSessionAction {
    operation: PageVaultOperation,
    selected: (Uuid, Uuid),
    epoch: AvailableKeyEpoch,
    deadline: Instant,
}
/// Frozen source and bounded user interaction. No secret-read or transport-upgrade receipt.
#[must_use]
pub struct InteractiveSessionIntent {
    operation: PageVaultOperation,
    selected: (Uuid, Uuid),
    source: SshKeyTargetSource,
    scope: VaultKeyLifecycle,
    epoch: AvailableKeyEpoch,
    deadline: Instant,
}
/// Trusted upper adapter bound to the actual verified peer. Failure MUST drain native resources.
pub trait InteractiveSessionSink: Send + 'static {
    /// Exact metadata and actual-peer match; an ordinary directory lookup is insufficient.
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool;
    /// Retain operation credit until actual native completion, including cancellation.
    fn retain_native_authorization(&mut self, guard: Authorize) -> Result<()>;
    /// Ask the user for each challenge under the original guard; never fall back to another method.
    fn authenticate_interactive<'a>(
        &'a mut self,
        username: &'a str,
        guard: Authorize,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;
    /// Create a terminal only after the server accepted this exact authentication.
    fn prepare_authenticated_terminal(
        &mut self,
        guard: Authorize,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    /// Single resource handoff. This is not an auth-only upgrade capability.
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts>;
}
impl PageVaultOwner {
    /// Capture the available original epoch once; later unlocks cannot authorize this action.
    pub fn begin_interactive_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<InteractiveSessionGesture> {
        let mut capture =
            self.begin_password_session_gesture_authorized(page, host, identity, original)?;
        capture.expires = Instant::now() + Duration::from_secs(300);
        Ok(InteractiveSessionGesture { capture })
    }
}
impl InteractiveSessionGesture {
    pub(crate) fn from_saved_capture(capture: PasswordSessionGesture) -> Result<Self> {
        if !capture.current() || Instant::now() >= capture.expires {
            return Err(VaultOwnerError::Denied);
        }
        Ok(Self { capture })
    }
    /// Compare saved identities only, without granting any use authority.
    pub fn matches_selection(&self, host: Uuid, identity: Uuid) -> bool {
        self.capture.matches_selection(host, identity)
    }
    /// Consume exactly once after an independent same-page approval.
    pub fn begin_session_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        approved: Authorize,
    ) -> Result<InteractiveSessionAction> {
        let deadline = self.capture.expires;
        let chosen = self.capture.approve_selection(owner, page, approved)?;
        Ok(InteractiveSessionAction {
            operation: owner.begin_authorized(chosen.page, chosen.guard)?,
            selected: chosen.selected,
            epoch: chosen.epoch,
            deadline,
        })
    }
}
#[path = "vault_interactive_route.rs"]
mod route;
pub use route::InteractiveForwardSink;
impl InteractiveSessionAction {
    /// Cancellation revokes only this attempt and does not establish native completion.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.operation.cancellation()
    }
    /// Freeze and compare full metadata; never resolve, borrow or decrypt password references.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<InteractiveSessionIntent> {
        self.prepare_bound(store, source, false).await
    }
    /// Exact route-hop source only; direct metadata cannot acquire a route role.
    pub async fn prepare_route(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<InteractiveSessionIntent> {
        self.prepare_bound(store, source, true).await
    }
    async fn prepare_bound(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        routed: bool,
    ) -> Result<InteractiveSessionIntent> {
        self.operation.current()?;
        if !(if routed {
            source.supports_route_interactive_selection(self.selected.0, self.selected.1)
        } else {
            source.supports_interactive_selection(self.selected.0, self.selected.1)
        }) || Instant::now() >= self.deadline
        {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .operation
            .bind(source.vault().clone())?
            .ok_or(VaultOwnerError::Denied)?;
        if !self.epoch.is_current_for(&scope).unwrap_or(false) {
            return Err(VaultOwnerError::Denied);
        }
        let intent = InteractiveSessionIntent {
            operation: self.operation,
            selected: self.selected,
            source,
            scope,
            epoch: self.epoch,
            deadline: self.deadline,
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
impl InteractiveSessionIntent {
    /// Original page, epoch, target lifetime and fixed deadline. Does not replace source validation.
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
    /// Frozen backend-only metadata, not a credential or use permission.
    pub fn selected_source(&self) -> (&SshKeyTargetSource, (Uuid, Uuid)) {
        (&self.source, self.selected)
    }
    /// Deadline captured at the original click. Challenge responses cannot renew it.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Attempt cancellation, not a native cleanup receipt.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.operation.cancellation()
    }
    /// Trusted driver keeps this intent alive until its sink is physically drained on failure.
    pub async fn authenticate_terminal<S: InteractiveSessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
    ) -> Result<ProtocolSessionOwner> {
        if self.source.password_source().is_route_bound()
            && !self.source.password_source().is_route_leaf()
        {
            return Err(VaultOwnerError::Denied);
        }
        let guard = self.authorization();
        let cancel = self.cancellation();
        started
            .as_ref()
            .ok_or(VaultOwnerError::Denied)?
            .check_original_page(&self.operation.token.page)
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        sink.retain_native_authorization(guard.clone())?;
        let username = self
            .source
            .password_source()
            .identity_vault()
            .identities()
            .records()
            .find(|row| row.id() == self.selected.1)
            .ok_or(VaultOwnerError::Denied)?
            .identity()
            .username();
        let accepted = tokio::select! { biased;
            _ = cancel.wait_cancelled() => return Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline)) => return Err(VaultOwnerError::Denied),
            result = sink.authenticate_interactive(username, guard.clone()) => result?,
        };
        if !accepted
            || !guard()
            || !sink.matches_target(&self.source, self.selected.0, self.selected.1)
        {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        tokio::select! { biased;
            _ = cancel.wait_cancelled() => return Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline)) => return Err(VaultOwnerError::Denied),
            result = sink.prepare_authenticated_terminal(guard.clone()) => result?,
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        let registry = started
            .as_ref()
            .ok_or(VaultOwnerError::Denied)?
            .source_registry()
            .clone();
        started
            .as_ref()
            .ok_or(VaultOwnerError::Denied)?
            .check_original_page(&self.operation.token.page)
            .map_err(|_| VaultOwnerError::Denied)?;
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
