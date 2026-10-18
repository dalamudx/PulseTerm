//! One key-free original capture, before saved directories decide the credential type.
use super::{
    AgentSessionGesture, Authorize, PageVaultOwner, PasswordSessionGesture,
    PrivateKeySessionGesture, Result, VaultOwnerError,
};
use crate::registry::PageLaunchScope;
use pulse_store::ssh_key_target_source::SshKeyTargetSource;
use std::{sync::Arc, time::Instant};
use uuid::Uuid;
#[path = "vault_route_session_gesture.rs"]
mod route;
pub use route::{RouteHopGesture, RouteHopSelection, RoutedSessionGesture};

#[path = "vault_cold_session_intent.rs"]
mod cold;
pub use cold::ColdSavedSessionIntent;
#[path = "vault_saved_alias_gesture.rs"]
mod alias;
pub use alias::{ColdSavedAliasIntent, SavedAliasGesture};
/// Original page and available epoch, without a credential type or use permission.
/// ```compile_fail
/// fn clone(v:pulse_app::vault_owner::SavedSessionGesture){let _=v.clone();}
/// ```
/// ```compile_fail
/// fn log(v:pulse_app::vault_owner::SavedSessionGesture){println!("{v:?}");}
/// ```
#[must_use]
pub struct SavedSessionGesture {
    capture: PasswordSessionGesture,
}

/// Consume once to select bounded metadata. These gestures still require independent GUI approval,
/// full-source preparation and connection-bound peer authorization before credential use.
/// ```compile_fail
/// fn log(v:pulse_app::vault_owner::SavedSessionSelection){println!("{v:?}");}
/// ```
#[must_use]
pub enum SavedSessionSelection {
    /// A unique saved password identity, still without connection or credential-use authority.
    Password(PasswordSessionGesture),
    /// A unique saved private-key identity, still without connection or credential-use authority.
    PrivateKey(PrivateKeySessionGesture),
    /// A unique saved Agent identity; listing and signing still require independent authorization.
    Agent(AgentSessionGesture),
}
impl PageVaultOwner {
    /// Original route click with a bounded five-minute multi-hop confirmation window.
    /// Capture the available epoch now; this never extends or renews an existing gesture.
    pub fn begin_routed_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<SavedSessionGesture> {
        let mut capture =
            self.begin_password_session_gesture_authorized(page, host, identity, original)?;
        capture.expires = Instant::now() + std::time::Duration::from_secs(300);
        Ok(SavedSessionGesture { capture })
    }
    /// Capture the original epoch exactly once, without SQL, CPU or network work.
    pub fn begin_saved_session_gesture_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        original: Authorize,
    ) -> Result<SavedSessionGesture> {
        Ok(SavedSessionGesture {
            capture: self
                .begin_password_session_gesture_authorized(page, host, identity, original)?,
        })
    }
}
impl SavedSessionGesture {
    /// Shorten a captured deadline. Never renews a page, epoch or permission.
    pub fn tighten_deadline(mut self, deadline: std::time::Instant) -> Self {
        self.capture.expires = self.capture.expires.min(deadline);
        self
    }
    /// Tighten an original capture with another route cancellation predicate. Never renews its
    /// page, owner, Vault epoch, saved selection or deadline, and grants no peer/use authority.
    pub fn restrict_route_authorization(
        mut self,
        page: &Arc<PageLaunchScope>,
        authorized: Authorize,
    ) -> Result<Self> {
        if !self.permits_route_read(page) || !authorized() {
            return Err(VaultOwnerError::Denied);
        }
        let original = self.capture.original;
        self.capture.original = Arc::new(move || original() && authorized());
        Ok(self)
    }
    /// Consume the original metadata capture for explicit keyboard-interactive use.
    /// This preserves its page, epoch, selected IDs, deadline and cancellation; no secret is read.
    pub fn into_interactive(self) -> Result<super::InteractiveSessionGesture> {
        super::InteractiveSessionGesture::from_saved_capture(self.capture)
    }
    /// Consume an explicit CLI key-plus-MFA metadata choice, retaining the original capture.
    /// No key-only use intent can be upgraded through this conversion.
    pub fn into_key_interactive(self) -> Result<super::PrivateKeyInteractiveSessionGesture> {
        super::PrivateKeyInteractiveSessionGesture::from_saved_capture(self.capture)
    }
    /// Consume an explicit saved-Agent plus MFA choice without renewing its original authority.
    /// Agent-only gestures cannot be converted into a joint authentication choice.
    pub fn into_agent_interactive(self) -> Result<super::AgentInteractiveSessionGesture> {
        super::AgentInteractiveSessionGesture::from_saved_capture(self.capture)
    }
    /// Consume explicit CLI metadata for password plus MFA, preserving original authority.
    /// Ordinary password-use intentions have no joint conversion entry point.
    pub fn into_password_interactive(self) -> Result<super::PasswordInteractiveSessionGesture> {
        super::PasswordInteractiveSessionGesture::from_saved_capture(self.capture)
    }
    /// Choose only the unique active saved identity in a single bounded transaction capture.
    /// A snapshot is not a source-verification receipt or credential-use authority.
    pub fn select_source(self, source: &SshKeyTargetSource) -> Result<SavedSessionSelection> {
        let (host, identity) = self.capture.selected;
        if Instant::now() >= self.capture.expires
            || !self.capture.current()
            || !(source
                .password_source()
                .supports_password_selection(host, identity)
                || source.supports_private_key_selection(host, identity)
                || source.supports_agent_selection(host, identity))
        {
            return Err(VaultOwnerError::Denied);
        }
        let password = source
            .password_source()
            .identity_vault()
            .identities()
            .records()
            .find(|v| v.id() == identity);
        let key = source.identities().records().find(|v| v.id() == identity);
        let agent = source
            .password_source()
            .agent_identities()
            .records()
            .find(|v| v.id() == identity);
        match (password, key, agent) {
            (Some(row), None, None) if row.identity().password().has_value() => {
                Ok(SavedSessionSelection::Password(self.capture))
            }
            (None, Some(row), None)
                if row.identity().private_key().has_value()
                    && row.identity().passphrase().is_none_or(|v| v.has_value()) =>
            {
                Ok(SavedSessionSelection::PrivateKey(
                    PrivateKeySessionGesture {
                        capture: self.capture,
                    },
                ))
            }
            (None, None, Some(_)) => Ok(SavedSessionSelection::Agent(AgentSessionGesture {
                capture: self.capture,
            })),
            _ => Err(VaultOwnerError::Denied),
        }
    }
}
#[cfg(test)]
impl SavedSessionGesture {
    pub(crate) fn fixture_expire(&mut self) {
        self.capture.expires = Instant::now();
    }
}
