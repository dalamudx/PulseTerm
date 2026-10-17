//! Private-key selection captures only an original page and available epoch, never password authority.
use super::{Authorize, PageVaultOwner, PasswordSessionGesture, Result};
use crate::{registry::PageLaunchScope, vault_owner::PrivateKeySessionAction};
use std::sync::Arc;
use uuid::Uuid;

/// Key-free original selection, distinct from password and private-key use or source authorization.
/// ```compile_fail
/// fn duplicate(v:pulse_app::vault_owner::PrivateKeySessionGesture){let _=v.clone();}
/// ```
/// ```compile_fail
/// fn log(v:pulse_app::vault_owner::PrivateKeySessionGesture){println!("{v:?}");}
/// ```
/// ```compile_fail
/// fn convert(v:pulse_app::vault_owner::PrivateKeySessionGesture)->pulse_app::vault_owner::PasswordSessionGesture{v.into()}
/// ```
#[must_use]
pub struct PrivateKeySessionGesture {
    // The shared capture contains no credential kind or permission; approval enters only the key action.
    pub(super) capture: PasswordSessionGesture,
}
impl PageVaultOwner {
    /// Freeze only an already-confirmed page/owner/key epoch and selected saved identifiers.
    /// No operation, source read, KDF, connection, key decode or authentication starts here.
    pub fn begin_private_key_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<PrivateKeySessionGesture> {
        Ok(PrivateKeySessionGesture {
            capture: self
                .begin_password_session_gesture_authorized(page, host, identity, original)?,
        })
    }
}
impl PrivateKeySessionGesture {
    /// Bounded metadata comparison only; not permission to use a credential or connection.
    pub fn matches_selection(&self, host: Uuid, identity: Uuid) -> bool {
        self.capture.matches_selection(host, identity)
    }
    /// Consume only after an independent trusted user action with the same page, owner and epoch.
    /// Original decision expiry never extends the subsequent five-second native startup deadline.
    pub fn begin_session_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        approved: Authorize,
    ) -> Result<PrivateKeySessionAction> {
        let selection = self.capture.approve_selection(owner, page, approved)?;
        Ok(owner
            .begin_private_key_session_action_authorized(
                selection.page,
                selection.selected.0,
                selection.selected.1,
                selection.guard,
            )?
            .pin_original_epoch(selection.epoch))
    }
}
#[cfg(test)]
impl PrivateKeySessionGesture {
    pub(crate) fn fixture_expire(&mut self) {
        self.capture.expires = std::time::Instant::now();
    }
}
