//! Explicit direct private-key + keyboard-interactive intent. No implicit upgrade from key-only use.
use crate::vault_owner::{
    Authorize, PageVaultCancellation, PageVaultOwner, PasswordSessionGesture,
    PrivateKeySessionAction, PrivateKeySessionIntent, PrivateKeySessionSink, PrivateKeySessionTask,
    Result, VaultOwnerError,
};
use crate::{protocol_session_owner::StartedProtocolSession, registry::PageLaunchScope};
use pulse_store::{StateStore, ssh_key_target_source::SshKeyTargetSource};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
/// Trusted adapter must require partial public-key success AND successful keyboard-interactive.
pub trait PrivateKeyInteractiveSessionSink: PrivateKeySessionSink {}
/// Trusted intermediate adapter must enforce both factors before forwarding.
pub trait PrivateKeyInteractiveForwardSink: crate::vault_owner::PrivateKeyForwardSink {}
/// Original-page, original-epoch, single-use explicit choice. Not cloneable or serializable.
pub struct PrivateKeyInteractiveSessionGesture {
    capture: PasswordSessionGesture,
}
/// Approved direct joint action bound to the original page, epoch and fixed deadline.
pub struct PrivateKeyInteractiveSessionAction {
    action: PrivateKeySessionAction,
    deadline: Instant,
}
/// Prepared direct joint intent; only a trusted two-factor adapter may start it.
pub struct PrivateKeyInteractiveSessionIntent {
    intent: PrivateKeySessionIntent,
}
impl PageVaultOwner {
    /// Capture an explicit private-key plus MFA choice without reading or decrypting a key.
    pub fn begin_private_key_interactive_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<PrivateKeyInteractiveSessionGesture> {
        let mut capture =
            self.begin_password_session_gesture_authorized(page, host, identity, original)?;
        capture.expires = Instant::now() + Duration::from_secs(300);
        Ok(PrivateKeyInteractiveSessionGesture { capture })
    }
}
impl PrivateKeyInteractiveSessionGesture {
    pub(crate) fn from_saved_capture(capture: PasswordSessionGesture) -> Result<Self> {
        if !capture.current() || Instant::now() >= capture.expires {
            return Err(VaultOwnerError::Denied);
        }
        Ok(Self { capture })
    }
    #[cfg(all(test, unix))]
    pub(crate) fn fixture_expire(&mut self) {
        self.capture.expires = Instant::now() - Duration::from_secs(1);
    }
    /// Check both saved identifiers against the original capture.
    pub fn matches_selection(&self, host: Uuid, identity: Uuid) -> bool {
        self.capture.matches_selection(host, identity)
    }
    /// Consume approval from the same page and original Vault epoch.
    pub fn begin_session_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        approved: Authorize,
    ) -> Result<PrivateKeyInteractiveSessionAction> {
        let deadline = self.capture.expires;
        let chosen = self.capture.approve_selection(owner, page, approved)?;
        let action = owner
            .begin_private_key_session_action_authorized(
                chosen.page,
                chosen.selected.0,
                chosen.selected.1,
                chosen.guard,
            )?
            .pin_original_epoch(chosen.epoch);
        Ok(PrivateKeyInteractiveSessionAction { action, deadline })
    }
}
impl PrivateKeyInteractiveSessionAction {
    /// Revoke this pending action while its supervised I/O retains capacity.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.action.cancellation()
    }
    /// Prepare the exact direct source using the deadline fixed at the original click.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<PrivateKeyInteractiveSessionIntent> {
        Ok(PrivateKeyInteractiveSessionIntent {
            intent: self
                .action
                .prepare_interactive(store, source, self.deadline)
                .await?,
        })
    }
    /// Prepare this exact frozen hop; direct metadata cannot acquire a route role.
    pub async fn prepare_route(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<PrivateKeyInteractiveSessionIntent> {
        Ok(PrivateKeyInteractiveSessionIntent {
            intent: self
                .action
                .prepare_interactive_route(store, source, self.deadline)
                .await?,
        })
    }
}
impl PrivateKeyInteractiveSessionIntent {
    /// Return bounded selected metadata; it grants no credential-use authority.
    pub fn selected_source(&self) -> (&SshKeyTargetSource, (Uuid, Uuid)) {
        self.intent.selected_source()
    }
    /// Return this attempt cancellation handle without affecting registered sessions.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.intent.cancellation()
    }
    /// Return the original fixed deadline; setup and questions cannot renew it.
    pub fn deadline(&self) -> Instant {
        self.intent.deadline()
    }
    /// Check original page, epoch, source liveness and deadline.
    pub fn authorization(&self) -> Authorize {
        self.intent.authorization()
    }
    /// Start supervised authentication and register only after both factors succeed.
    pub fn start<S: PrivateKeyInteractiveSessionSink>(
        self,
        store: StateStore,
        sink: S,
        started: StartedProtocolSession,
    ) -> PrivateKeySessionTask {
        self.intent.start(store, sink, started)
    }
    /// Forward only to this route's frozen next target; caller must drain on failure.
    pub async fn authenticate_forward<S: PrivateKeyInteractiveForwardSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
    ) -> Result<S::Transport> {
        self.intent.authenticate_forward(store, sink).await
    }
    /// Register only this route's leaf; an intermediate hop is rejected before adapter use.
    pub async fn authenticate_route_terminal<S: PrivateKeyInteractiveSessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
    ) -> Result<crate::protocol_session_owner::ProtocolSessionOwner> {
        self.intent
            .authenticate_route_terminal(store, sink, started)
            .await
    }
}
