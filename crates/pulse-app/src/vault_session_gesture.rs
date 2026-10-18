//! Key-free, network-neutral capture before a trusted GUI decision. No source/use/peer approval.
#[cfg(feature = "ssh-use")]
#[path = "vault_key_interactive_session.rs"]
mod key_interactive_session;
use super::{Authorize, Inner, PageVaultOwner, PasswordSessionAction, Result, VaultOwnerError};
use crate::registry::PageLaunchScope;
#[cfg(feature = "ssh-use")]
pub use key_interactive_session::{
    PrivateKeyInteractiveForwardSink, PrivateKeyInteractiveSessionAction,
    PrivateKeyInteractiveSessionGesture, PrivateKeyInteractiveSessionIntent,
    PrivateKeyInteractiveSessionSink,
};
use pulse_store::vault_lifecycle::AvailableKeyEpoch;
use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use uuid::Uuid;
#[path = "vault_key_session_gesture.rs"]
mod key;
pub use key::PrivateKeySessionGesture;
#[path = "vault_agent_session.rs"]
mod agent;
#[path = "vault_agent_interactive_session.rs"]
mod agent_interactive;
#[path = "vault_password_interactive_session.rs"]
mod password_interactive;
pub use agent::{
    AgentForwardSink, AgentSessionAction, AgentSessionGesture, AgentSessionIntent, AgentSessionSink,
};
pub use agent_interactive::{
    AgentInteractiveForwardSink, AgentInteractiveSessionAction, AgentInteractiveSessionGesture,
    AgentInteractiveSessionIntent, AgentInteractiveSessionSink,
};
pub use password_interactive::{
    PasswordInteractiveSessionAction, PasswordInteractiveSessionGesture,
    PasswordInteractiveSessionIntent, PasswordInteractiveSessionSink,
};

#[path = "vault_interactive_session.rs"]
mod interactive;
pub use interactive::{
    InteractiveForwardSink, InteractiveSessionAction, InteractiveSessionGesture,
    InteractiveSessionIntent, InteractiveSessionSink,
};
#[path = "vault_saved_session_gesture.rs"]
mod saved;
pub use saved::{
    ColdSavedAliasIntent, ColdSavedSessionIntent, RouteHopGesture, RouteHopSelection,
    RoutedSessionGesture, SavedAliasGesture, SavedSessionGesture, SavedSessionSelection,
};

struct ApprovedSessionSelection {
    page: Arc<PageLaunchScope>,
    selected: (Uuid, Uuid),
    epoch: AvailableKeyEpoch,
    guard: Authorize,
}

/// Original page/owner/selection/available epoch, never a secret or operation reservation.
/// The upper adapter must independently approve the action and freeze/verify its full source.
/// ```compile_fail
/// fn duplicate(v:pulse_app::vault_owner::PasswordSessionGesture){let _=v.clone();}
/// ```
/// ```compile_fail
/// fn dump(v:pulse_app::vault_owner::PasswordSessionGesture){println!("{v:?}");}
/// ```
#[must_use]
pub struct PasswordSessionGesture {
    owner: Weak<Inner>,
    page: Arc<PageLaunchScope>,
    selected: (Uuid, Uuid),
    epoch: AvailableKeyEpoch,
    original: Authorize,
    pub(super) expires: Instant,
}
impl PageVaultOwner {
    /// Capture only an already-confirmed original key epoch, without beginning CPU/SQL/network work.
    /// A locked/pending key cannot become authorized merely by later unlocking.
    pub fn begin_password_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<PasswordSessionGesture> {
        if host.is_nil() || identity.is_nil() || !original() {
            return Err(VaultOwnerError::Denied);
        }
        self.inner
            .registry
            .check_launch_page(&page)
            .map_err(|_| VaultOwnerError::Denied)?;
        let state = self.inner.guard()?;
        if state.closed {
            return Err(VaultOwnerError::Denied);
        }
        if state.pending.upgrade().is_some() {
            return Err(VaultOwnerError::Busy);
        }
        let epoch = state
            .scope
            .as_ref()
            .ok_or(VaultOwnerError::Denied)?
            .available_epoch()
            .map_err(|_| VaultOwnerError::Denied)?;
        Ok(PasswordSessionGesture {
            owner: Arc::downgrade(&self.inner),
            page,
            selected: (host, identity),
            epoch,
            original,
            expires: Instant::now() + Duration::from_secs(30),
        })
    }
}
impl PasswordSessionGesture {
    fn current(&self) -> bool {
        if !(self.original)() {
            return false;
        }
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        if owner.registry.check_launch_page(&self.page).is_err() {
            return false;
        }
        let Ok(state) = owner.guard() else {
            return false;
        };
        !state.closed
            && state
                .scope
                .as_ref()
                .is_some_and(|scope| self.epoch.is_current_for(scope).unwrap_or(false))
    }
    // This predicate can run while PageVaultOperation::bind holds the owner mutex.
    // Owner retirement closes the captured lifecycle; operation admission independently
    // checks owner.closed. Never re-enter the owner mutex from its external predicate.
    fn current_without_owner_lock(&self) -> bool {
        (self.original)()
            && self
                .owner
                .upgrade()
                .is_some_and(|owner| owner.registry.check_launch_page(&self.page).is_ok())
            && self.epoch.is_original_current().unwrap_or(false)
    }
    /// Compare bounded saved metadata only; this grants no page, source, key or target-use authority.
    pub fn matches_selection(&self, host: Uuid, identity: Uuid) -> bool {
        self.selected == (host, identity)
    }
    /// Consume only after a separate trusted GUI approval; possession grants no execution authority.
    /// Original owner/page/epoch remain in the native predicate throughout preparation and I/O.
    /// The GUI decision expiry cannot renew native deadlines; existing preparation starts its own 5s.
    pub fn begin_session_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        approved: Authorize,
    ) -> Result<PasswordSessionAction> {
        let selected = self.approve_selection(owner, page, approved)?;
        Ok(owner
            .begin_password_session_action_authorized(
                selected.page,
                selected.selected.0,
                selected.selected.1,
                selected.guard,
            )?
            .pin_original_epoch(selected.epoch))
    }
    fn approve_selection(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        approved: Authorize,
    ) -> Result<ApprovedSessionSelection> {
        if !Weak::ptr_eq(&self.owner, &Arc::downgrade(&owner.inner))
            || !Arc::ptr_eq(&self.page, page)
            || Instant::now() >= self.expires
            || !self.current()
            || !approved()
        {
            return Err(VaultOwnerError::Denied);
        }
        // Carry the epoch into prepare/borrow; do not lock it inside a native predicate while a secret borrow already holds its lock.
        let original = self.original;
        let guard: Authorize = Arc::new(move || approved() && original());
        Ok(ApprovedSessionSelection {
            page: self.page,
            selected: self.selected,
            epoch: self.epoch,
            guard,
        })
    }
}
