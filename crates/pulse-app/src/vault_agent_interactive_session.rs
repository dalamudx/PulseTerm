//! Explicit saved-Agent plus MFA. Ordinary Agent gestures cannot be upgraded into this policy.
use crate::vault_owner::{
    AgentSessionAction, AgentSessionGesture, AgentSessionIntent, AgentSessionSink, Authorize,
    PageVaultCancellation, PageVaultOwner, PasswordSessionGesture, Result,
};
use crate::{
    protocol_session_owner::{ProtocolSessionOwner, StartedProtocolSession},
    registry::PageLaunchScope,
};
use pulse_store::{StateStore, ssh_key_target_source::SshKeyTargetSource};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
/// Trusted adapter must require the selected Agent signature, partial success and successful MFA.
pub trait AgentInteractiveSessionSink: AgentSessionSink {}
/// Trusted intermediate adapter must enforce both factors before opening the frozen next target.
pub trait AgentInteractiveForwardSink: crate::vault_owner::AgentForwardSink {}
/// Original page, epoch, selection and deadline, captured only for an explicit joint choice.
pub struct AgentInteractiveSessionGesture {
    capture: PasswordSessionGesture,
}
/// Single-use joint action; source preparation cannot extend the original deadline.
pub struct AgentInteractiveSessionAction {
    action: AgentSessionAction,
    deadline: Instant,
}
/// Saved source usable only through an explicitly selected two-factor adapter.
pub struct AgentInteractiveSessionIntent {
    intent: AgentSessionIntent,
}
impl PageVaultOwner {
    /// Capture an explicit Agent plus MFA choice without discovering or signing with an Agent.
    pub fn begin_agent_interactive_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<AgentInteractiveSessionGesture> {
        let mut capture =
            self.begin_password_session_gesture_authorized(page, host, identity, original)?;
        capture.expires = Instant::now() + Duration::from_secs(300);
        Ok(AgentInteractiveSessionGesture { capture })
    }
}
impl AgentInteractiveSessionGesture {
    pub(crate) fn from_saved_capture(capture: PasswordSessionGesture) -> Result<Self> {
        if !capture.current() || Instant::now() >= capture.expires {
            return Err(super::VaultOwnerError::Denied);
        }
        Ok(Self { capture })
    }
    /// Check the exact saved selection captured at the original click.
    pub fn matches_selection(&self, host: Uuid, identity: Uuid) -> bool {
        self.capture.matches_selection(host, identity)
    }
    /// Consume the original page and epoch approval; later unlocking cannot supply authority.
    pub fn begin_session_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        approved: Authorize,
    ) -> Result<AgentInteractiveSessionAction> {
        let deadline = self.capture.expires;
        let action = AgentSessionGesture {
            capture: self.capture,
        }
        .begin_session_authorized(owner, page, approved)?;
        Ok(AgentInteractiveSessionAction { action, deadline })
    }
}
impl AgentInteractiveSessionAction {
    /// Revoke this pending action without releasing native I/O capacity early.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.action.cancellation()
    }
    /// Prepare only this frozen route hop, keeping its original deadline and forwarding/leaf role.
    pub async fn prepare_route(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<AgentInteractiveSessionIntent> {
        if !source.password_source().is_route_bound() {
            return Err(super::VaultOwnerError::Denied);
        }
        Ok(AgentInteractiveSessionIntent {
            intent: self.action.prepare_at(store, source, self.deadline).await?,
        })
    }
    /// Prepare only a saved direct Agent source; temporary and route policies remain separate.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<AgentInteractiveSessionIntent> {
        Ok(AgentInteractiveSessionIntent {
            intent: self
                .action
                .prepare_direct_interactive(store, source, self.deadline)
                .await?,
        })
    }
    /// Prepare an explicitly selected temporary direct target, never a saved or route source.
    pub async fn prepare_temporary(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
    ) -> Result<AgentInteractiveSessionIntent> {
        if !source.is_temporary() || source.password_source().is_route_bound() {
            return Err(super::VaultOwnerError::Denied);
        }
        Ok(AgentInteractiveSessionIntent {
            intent: self.action.prepare_at(store, source, self.deadline).await?,
        })
    }
}
impl AgentInteractiveSessionIntent {
    /// Drawing-only source metadata; never Agent socket paths or signing authority.
    pub fn selected_source(&self) -> (&SshKeyTargetSource, (Uuid, Uuid)) {
        self.intent.selected_source()
    }
    /// Attempt-local cancellation retained until native drain.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.intent.cancellation()
    }
    /// Original fixed joint deadline.
    pub fn deadline(&self) -> Instant {
        self.intent.deadline()
    }
    /// Original page, epoch, deadline and cancellation guard.
    pub fn authorization(&self) -> Authorize {
        self.intent.authorization()
    }
    /// Authenticate both factors before forwarding only to this route's frozen next target.
    pub async fn authenticate_forward<S: AgentInteractiveForwardSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
    ) -> Result<S::Transport> {
        self.intent.authenticate_forward(store, sink).await
    }
    /// Register only the explicitly approved route leaf after both factors succeed.
    pub async fn authenticate_route_terminal<S: AgentInteractiveSessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
    ) -> Result<ProtocolSessionOwner> {
        if !self
            .intent
            .selected_source()
            .0
            .password_source()
            .is_route_leaf()
        {
            return Err(super::VaultOwnerError::Denied);
        }
        self.intent
            .authenticate_terminal(store, sink, started)
            .await
    }
    /// Register a terminal only after both explicitly selected authentication factors succeed.
    pub async fn authenticate_terminal<S: AgentInteractiveSessionSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
        started: &mut Option<StartedProtocolSession>,
    ) -> Result<ProtocolSessionOwner> {
        self.intent
            .authenticate_terminal(store, sink, started)
            .await
    }
}
