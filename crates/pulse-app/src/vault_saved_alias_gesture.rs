//! One-shot literal saved alias selection, frozen before a supervised metadata read.
use super::{
    Authorize, PageVaultOwner, PasswordSessionGesture, Result, SavedSessionGesture, VaultOwnerError,
};
use crate::{registry::PageLaunchScope, vault_owner::Inner};
use pulse_store::{
    StateStore, ssh_key_target_source::SshKeyTargetSource,
    ssh_route_source::SavedSshAliasSelection, vault_lifecycle::AvailableKeyEpoch,
};
use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

/// Original owner/page/epoch and alias. No secret, I/O credit, target or use permission.
/// ```compile_fail
/// fn copy(v: pulse_app::vault_owner::SavedAliasGesture) { let _ = v.clone(); }
/// ```
/// ```compile_fail
/// fn log(v: pulse_app::vault_owner::SavedAliasGesture) { println!("{v:?}"); }
/// ```
#[must_use]
pub struct SavedAliasGesture {
    owner: Weak<Inner>,
    page: Arc<PageLaunchScope>,
    epoch: AvailableKeyEpoch,
    original: Authorize,
    expires: Instant,
    alias: String,
}
/// Cold argv metadata only. Unlocking cannot confirm or select it.
/// ```compile_fail
/// fn copy(v: pulse_app::vault_owner::ColdSavedAliasIntent) { let _ = v.clone(); }
/// ```
/// ```compile_fail
/// fn log(v: pulse_app::vault_owner::ColdSavedAliasIntent) { println!("{v:?}"); }
/// ```
#[must_use]
pub struct ColdSavedAliasIntent {
    owner: Weak<Inner>,
    page: Arc<PageLaunchScope>,
    original: Authorize,
    expires: Instant,
    alias: String,
}
fn valid_alias(alias: &str) -> bool {
    pulse_core::ssh_endpoint::SshEndpoint::new(alias, "alias.invalid", 22, "metadata").is_ok()
}
impl PageVaultOwner {
    /// Freeze the already available original epoch before reading alias/identity directories.
    pub fn begin_saved_alias_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        alias: &str,
        original: Authorize,
    ) -> Result<SavedAliasGesture> {
        if !valid_alias(alias) || !original() {
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
        Ok(SavedAliasGesture {
            owner: Arc::downgrade(&self.inner),
            page,
            epoch,
            original,
            expires: Instant::now() + Duration::from_secs(300),
            alias: alias.to_owned(),
        })
    }
    /// Register an alias while locked, without capturing an epoch or starting work.
    pub fn begin_cold_saved_alias_intent_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        alias: &str,
        original: Authorize,
    ) -> Result<ColdSavedAliasIntent> {
        if !valid_alias(alias) || !original() || self.inner.guard()?.closed {
            return Err(VaultOwnerError::Denied);
        }
        self.inner
            .registry
            .check_launch_page(&page)
            .map_err(|_| VaultOwnerError::Denied)?;
        Ok(ColdSavedAliasIntent {
            owner: Arc::downgrade(&self.inner),
            page,
            original,
            expires: Instant::now() + Duration::from_secs(30),
            alias: alias.to_owned(),
        })
    }
}
impl SavedAliasGesture {
    /// Narrow a deadline; never extend an original request or inherit a new epoch.
    pub fn tighten_deadline(mut self, deadline: Instant) -> Self {
        self.expires = self.expires.min(deadline);
        self
    }
    /// Resolve the original literal alias once, preserving the original epoch in the saved gesture.
    /// Existing target approval, source CAS and actual-peer authorization still apply.
    pub fn select_source(
        self,
        store: &StateStore,
        source: &SshKeyTargetSource,
    ) -> Result<(SavedSessionGesture, SavedSshAliasSelection)> {
        let selected = store
            .resolve_ssh_alias_source(source, &self.alias)
            .map_err(|_| VaultOwnerError::Denied)?;
        let capture = PasswordSessionGesture {
            owner: self.owner,
            page: self.page,
            selected: (selected.host_id(), selected.identity_id()),
            epoch: self.epoch,
            original: self.original,
            expires: self.expires,
        };
        if Instant::now() >= capture.expires || !capture.current() {
            return Err(VaultOwnerError::Denied);
        }
        Ok((SavedSessionGesture { capture }, selected))
    }
    #[cfg(test)]
    pub(crate) fn fixture_expire(&mut self) {
        self.expires = Instant::now();
    }
}
impl ColdSavedAliasIntent {
    /// Confirmation must come from the exact original owner/page on a distinct authenticated click.
    pub fn confirm_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        confirmation: Authorize,
    ) -> Result<SavedAliasGesture> {
        if Instant::now() >= self.expires
            || !Arc::ptr_eq(&self.page, page)
            || !self
                .owner
                .upgrade()
                .is_some_and(|inner| Arc::ptr_eq(&inner, &owner.inner))
            || !(self.original)()
            || !confirmation()
        {
            return Err(VaultOwnerError::Denied);
        }
        let original = self.original;
        owner
            .begin_saved_alias_gesture_authorized(
                self.page,
                &self.alias,
                Arc::new(move || original() && confirmation()),
            )
            .map(|gesture| gesture.tighten_deadline(self.expires))
    }
    #[cfg(test)]
    pub(crate) fn fixture_expire(&mut self) {
        self.expires = Instant::now();
    }
}
