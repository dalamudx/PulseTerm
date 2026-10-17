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
/// Original page, epoch, selection and deadline, captured only for an explicit joint choice.
pub struct AgentInteractiveSessionGesture {
    capture: PasswordSessionGesture,
}
/// Single-use joint action; source preparation cannot extend the original deadline.
pub struct AgentInteractiveSessionAction {
    action: AgentSessionAction,
    deadline: Instant,
}
/// Saved direct source usable only through a two-factor adapter.
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
