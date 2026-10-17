//! A cold saved-target request is metadata until a distinct original-page click.
use super::{Authorize, PageVaultOwner, Result, SavedSessionGesture, VaultOwnerError};
use crate::{registry::PageLaunchScope, vault_owner::Inner};
use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// A one-shot saved-ID intent. It captures neither a Vault epoch nor any I/O or use authority.
/// Unlocking cannot confirm it; the window must separately accept the original request.
/// ```compile_fail
/// fn clone(v: pulse_app::vault_owner::ColdSavedSessionIntent) { let _ = v.clone(); }
/// ```
/// ```compile_fail
/// fn log(v: pulse_app::vault_owner::ColdSavedSessionIntent) { println!("{v:?}"); }
/// ```
#[must_use]
pub struct ColdSavedSessionIntent {
    owner: Weak<Inner>,
    page: Arc<PageLaunchScope>,
    selected: (Uuid, Uuid),
    expires: Instant,
    original: Authorize,
}

impl PageVaultOwner {
    /// Register only the two saved IDs while locked or unlocked, without SQL/CPU/network work.
    /// The original page and cancellation predicate remain binding across a separate unlock.
    pub fn begin_cold_saved_session_intent_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<ColdSavedSessionIntent> {
        if host.is_nil() || identity.is_nil() || !original() {
            return Err(VaultOwnerError::Denied);
        }
        self.inner
            .registry
            .check_launch_page(&page)
            .map_err(|_| VaultOwnerError::Denied)?;
        if self.inner.guard()?.closed {
            return Err(VaultOwnerError::Denied);
        }
        Ok(ColdSavedSessionIntent {
            owner: Arc::downgrade(&self.inner),
            page,
            selected: (host, identity),
            expires: Instant::now() + Duration::from_secs(30),
            original,
        })
    }
}

impl ColdSavedSessionIntent {
    /// Tighten the original request deadline; this does not renew any request or permission.
    pub fn tighten_deadline(mut self, deadline: Instant) -> Self {
        self.expires = self.expires.min(deadline);
        self
    }

    /// Consume on a distinct explicit click from the original page. Capture the epoch now,
    /// preserve the exact saved IDs and original deadline, and retain both cancellation checks.
    /// Source, actual peer and credential use still require their independent approval chain.
    pub fn confirm_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        confirmation: Authorize,
    ) -> Result<SavedSessionGesture> {
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
        let guard: Authorize = Arc::new(move || original() && confirmation());
        owner
            .begin_saved_session_gesture_authorized(
                self.page,
                self.selected.0,
                self.selected.1,
                guard,
            )
            .map(|gesture| gesture.tighten_deadline(self.expires))
    }
}
