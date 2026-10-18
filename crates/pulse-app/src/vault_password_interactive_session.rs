//! Explicit saved or temporary direct password plus MFA; ordinary intentions cannot be upgraded.
use crate::vault_owner::{
    Authorize, PageVaultCancellation, PageVaultOwner, PasswordSessionAction,
    PasswordSessionGesture, PasswordSessionIntent, PasswordSessionSink, PasswordSessionTask,
    Result, VaultOwnerError,
};
use crate::{protocol_session_owner::StartedProtocolSession, registry::PageLaunchScope};
use pulse_store::{StateStore, ssh_target_source::SshTargetSource};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Trusted adapter requires password partial success, then MFA before terminal setup.
/// Password borrowing must finish before prompting for the second factor.
pub trait PasswordInteractiveSessionSink: PasswordSessionSink {}
/// Original-page, epoch, selection and deadline for one explicit joint choice.
pub struct PasswordInteractiveSessionGesture {
    capture: PasswordSessionGesture,
}
/// Approved single-use joint action; preparation cannot renew the original deadline.
pub struct PasswordInteractiveSessionAction {
    action: PasswordSessionAction,
    deadline: Instant,
}
/// Prepared direct source, usable only with an explicitly selected trusted joint adapter.
pub struct PasswordInteractiveSessionIntent {
    intent: PasswordSessionIntent,
}
impl PageVaultOwner {
    /// Capture explicit password plus MFA without reading credentials or opening a connection.
    pub fn begin_password_interactive_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<PasswordInteractiveSessionGesture> {
        let mut capture =
            self.begin_password_session_gesture_authorized(page, host, identity, original)?;
        capture.expires = Instant::now() + Duration::from_secs(300);
        Ok(PasswordInteractiveSessionGesture { capture })
    }
}
impl PasswordInteractiveSessionGesture {
    pub(crate) fn from_saved_capture(capture: PasswordSessionGesture) -> Result<Self> {
        if !capture.current() || Instant::now() >= capture.expires {
            return Err(VaultOwnerError::Denied);
        }
        Ok(Self { capture })
    }
    /// Check the exact identifiers captured by the original click.
    pub fn matches_selection(&self, host: Uuid, identity: Uuid) -> bool {
        self.capture.matches_selection(host, identity)
    }
    /// Consume approval from the original page and Vault epoch.
    pub fn begin_session_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        approved: Authorize,
    ) -> Result<PasswordInteractiveSessionAction> {
        let deadline = self.capture.expires;
        let action = self
            .capture
            .begin_session_authorized(owner, page, approved)?;
        Ok(PasswordInteractiveSessionAction { action, deadline })
    }
    #[cfg(test)]
    pub(crate) fn fixture_expire(&mut self) {
        self.capture.expires = Instant::now() - Duration::from_secs(1);
    }
}
impl PasswordInteractiveSessionAction {
    /// Revoke pending authority while physical cleanup retains native capacity.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.action.cancellation()
    }
    /// Prepare only a saved direct source under the deadline captured before approval.
    pub async fn prepare(
        self,
        store: &StateStore,
        source: SshTargetSource,
    ) -> Result<PasswordInteractiveSessionIntent> {
        if source.is_temporary() || source.is_route_bound() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(PasswordInteractiveSessionIntent {
            intent: self
                .action
                .prepare_interactive(store, source, self.deadline)
                .await?,
        })
    }
    /// Prepare only an explicitly selected temporary direct target; saved and route sources are rejected.
    pub async fn prepare_temporary(
        self,
        store: &StateStore,
        source: SshTargetSource,
    ) -> Result<PasswordInteractiveSessionIntent> {
        if !source.is_temporary() || source.is_route_bound() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(PasswordInteractiveSessionIntent {
            intent: self
                .action
                .prepare_interactive(store, source, self.deadline)
                .await?,
        })
    }
}
impl PasswordInteractiveSessionIntent {
    /// Bounded original metadata; does not grant credential use.
    pub fn selected_source(&self) -> (&SshTargetSource, (Uuid, Uuid)) {
        self.intent.selected_source()
    }
    /// Fixed deadline of the original choice.
    pub fn deadline(&self) -> Instant {
        self.intent.deadline()
    }
    /// Original page, epoch, source liveness and cancellation guard.
    pub fn authorization(&self) -> Authorize {
        self.intent.authorization()
    }
    /// Revoke this pending attempt independently of registered sessions.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.intent.cancellation()
    }
    /// Both factors must succeed before the supervised task can register a terminal.
    pub fn start<S: PasswordInteractiveSessionSink>(
        self,
        store: StateStore,
        sink: S,
        started: StartedProtocolSession,
    ) -> PasswordSessionTask {
        self.intent.start(store, sink, started)
    }
}
