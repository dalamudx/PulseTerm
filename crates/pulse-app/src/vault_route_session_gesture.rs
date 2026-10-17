//! One original page/epoch for ordered metadata issuance across a saved route.
#[path = "vault_route_read.rs"]
mod read;
use super::{
    Authorize, PasswordSessionGesture, Result, SavedSessionGesture, SavedSessionSelection,
    VaultOwnerError,
};
use pulse_store::{
    ssh_key_target_source::SshKeyTargetSource, ssh_route_source::SshRouteSource,
    ssh_target_source::SshTargetSource,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// Nonduplicable original click, not a connected route, peer approval or operation reservation.
/// Emitting a hop selection orders metadata only: the protocol driver must authenticate each
/// actual peer and retain all intermediate resources until confirmed closure.
/// ```compile_fail
/// fn copy(v:pulse_app::vault_owner::RoutedSessionGesture){let _=v.clone();}
/// ```
/// ```compile_fail
/// fn log(v:pulse_app::vault_owner::RoutedSessionGesture){println!("{v:?}");}
/// ```
#[must_use]
pub struct RoutedSessionGesture {
    capture: PasswordSessionGesture,
    source: SshRouteSource,
    next: usize,
    live: Arc<AtomicBool>,
}
/// Exact kind of one original-route hop. Not Clone, not a peer or execution approval.
pub enum RouteHopGesture {
    /// Password operation derived from the original route click.
    Password(PasswordSessionGesture),
    /// Private-key operation derived from the original route click.
    PrivateKey(crate::vault_owner::PrivateKeySessionGesture),
    /// Local Agent single-key operation derived from the original route click.
    Agent(crate::vault_owner::AgentSessionGesture),
    /// Metadata-only interactive operation, explicitly chosen at this hop's actual-peer confirmation.
    Interactive(crate::vault_owner::InteractiveSessionGesture),
    /// Explicit private-key plus MFA captured at this exact route hop.
    PrivateKeyInteractive(crate::vault_owner::PrivateKeyInteractiveSessionGesture),
}
/// Single-use typed gesture and its exact-hop source. No execution permission or plaintext.
#[must_use]
pub struct RouteHopSelection {
    selection: RouteHopGesture,
    source: SshKeyTargetSource,
}
impl RouteHopGesture {
    /// Consume only the original private-key capture; never renew its page, epoch or expiry.
    pub fn into_key_interactive(self) -> Result<Self> {
        let Self::PrivateKey(gesture) = self else {
            return Err(VaultOwnerError::Denied);
        };
        Ok(Self::PrivateKeyInteractive(
            crate::vault_owner::PrivateKeyInteractiveSessionGesture::from_saved_capture(
                gesture.capture,
            )?,
        ))
    }
    /// Consume the existing account capture; no fresh page, epoch or deadline.
    pub fn into_interactive(self) -> Result<Self> {
        let Self::Password(capture) = self else {
            return Err(VaultOwnerError::Denied);
        };
        Ok(Self::Interactive(
            crate::vault_owner::InteractiveSessionGesture::from_saved_capture(capture)?,
        ))
    }
}
impl RouteHopSelection {
    /// Choose both factors for this frozen private-key hop without reading a credential.
    pub fn into_key_interactive(self) -> Result<Self> {
        if !self.source.password_source().is_route_bound() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(Self {
            selection: self.selection.into_key_interactive()?,
            source: self.source,
        })
    }
    /// Consume only a password-account capture from this exact frozen route. No credential read.
    pub fn into_interactive(self) -> Result<Self> {
        let RouteHopGesture::Password(capture) = self.selection else {
            return Err(VaultOwnerError::Denied);
        };
        if !self
            .source
            .supports_route_interactive_selection(capture.selected.0, capture.selected.1)
        {
            return Err(VaultOwnerError::Denied);
        }
        Ok(Self {
            selection: RouteHopGesture::Interactive(
                crate::vault_owner::InteractiveSessionGesture::from_saved_capture(capture)?,
            ),
            source: self.source,
        })
    }
    /// Retain the parent route until all child I/O and protocol cleanup have exited.
    pub fn into_parts(self) -> (RouteHopGesture, SshKeyTargetSource) {
        (self.selection, self.source)
    }
}
impl SavedSessionGesture {
    /// Freeze the route against the already captured final saved-entry selection and original
    /// available epoch. Does not capture a newer epoch, begin a job or authorize a connection.
    pub fn select_route(self, source: SshRouteSource) -> Result<RoutedSessionGesture> {
        let last = source
            .hops()
            .len()
            .checked_sub(1)
            .ok_or(VaultOwnerError::Denied)?;
        let capture = match self.select_route_hop(
            &source
                .hop_source(last)
                .map_err(|_| VaultOwnerError::Denied)?,
        )? {
            RouteHopGesture::Password(capture) => capture,
            RouteHopGesture::PrivateKey(gesture) => gesture.capture,
            RouteHopGesture::Agent(gesture) => gesture.capture,
            RouteHopGesture::Interactive(_) | RouteHopGesture::PrivateKeyInteractive(_) => {
                return Err(VaultOwnerError::Denied);
            }
        };
        Ok(RoutedSessionGesture {
            capture,
            source,
            next: 0,
            live: Arc::new(AtomicBool::new(true)),
        })
    }
}
impl SavedSessionGesture {
    fn select_route_hop(self, source: &SshKeyTargetSource) -> Result<RouteHopGesture> {
        let (host, identity) = self.capture.selected;
        if source.supports_agent_selection(host, identity) {
            if !self.capture.current() || Instant::now() >= self.capture.expires {
                return Err(VaultOwnerError::Denied);
            }
            return Ok(RouteHopGesture::Agent(
                crate::vault_owner::AgentSessionGesture {
                    capture: self.capture,
                },
            ));
        }
        match self.select_source(source)? {
            SavedSessionSelection::Password(capture) => Ok(RouteHopGesture::Password(capture)),
            SavedSessionSelection::PrivateKey(gesture) => Ok(RouteHopGesture::PrivateKey(gesture)),
            SavedSessionSelection::Agent(gesture) => Ok(RouteHopGesture::Agent(gesture)),
        }
    }
}
impl RoutedSessionGesture {
    fn current(&self) -> bool {
        self.live.load(Ordering::Acquire)
            && Instant::now() < self.capture.expires
            && self.capture.current()
    }
    /// Backend-only immutable route metadata; no peer, credential or execution permission.
    pub fn source(&self) -> &SshRouteSource {
        &self.source
    }
    /// Key-free original page/epoch/deadline guard for dial and confirmation supervision.
    pub fn authorization(&self) -> Authorize {
        let capture = PasswordSessionGesture {
            owner: self.capture.owner.clone(),
            page: self.capture.page.clone(),
            selected: self.capture.selected,
            epoch: self.capture.epoch.clone(),
            original: self.capture.original.clone(),
            expires: self.capture.expires,
        };
        let live = self.live.clone();
        Arc::new(move || {
            live.load(Ordering::Acquire)
                && Instant::now() < capture.expires
                && capture.current_without_owner_lock()
        })
    }

    /// Revoke already-issued gestures and native guards; resource credit remains with cleanup.
    pub fn cancel(&self) {
        self.live.store(false, Ordering::Release);
    }
    /// Issue each frozen hop once, from outermost to leaf, using the original epoch and expiry.
    /// Issuance is not proof of preceding authentication and does not advance a network route.
    pub fn take_next_selection(&mut self) -> Result<RouteHopSelection> {
        if !self.current() {
            return Err(VaultOwnerError::Denied);
        }
        let hop = self
            .source
            .hops()
            .get(self.next)
            .ok_or(VaultOwnerError::Denied)?;
        let source = self
            .source
            .hop_source(self.next)
            .map_err(|_| VaultOwnerError::Denied)?;
        let original = self.capture.original.clone();
        let live = self.live.clone();
        let guard: Authorize = Arc::new(move || live.load(Ordering::Acquire) && original());
        let capture = PasswordSessionGesture {
            owner: self.capture.owner.clone(),
            page: self.capture.page.clone(),
            selected: (
                hop.entrance_id(),
                match hop.identity() {
                    pulse_store::ssh_route_source::SshRouteIdentity::Password(id)
                    | pulse_store::ssh_route_source::SshRouteIdentity::PrivateKey(id)
                    | pulse_store::ssh_route_source::SshRouteIdentity::Agent(id) => id,
                },
            ),
            epoch: self.capture.epoch.clone(),
            original: guard,
            expires: self.capture.expires,
        };
        let selection = SavedSessionGesture { capture }.select_route_hop(&source)?;
        self.next += 1;
        Ok(RouteHopSelection { selection, source })
    }
    /// Rebase only the last issued hop's confirmed first-pin storage reply. No other edits may
    /// acquire the authority of the original click; actual-peer approval remains independent.
    pub fn accept_first_pin(&mut self, updated: &SshTargetSource) -> Result<SshKeyTargetSource> {
        if !self.current() {
            return Err(VaultOwnerError::Denied);
        }
        let index = self.next.checked_sub(1).ok_or(VaultOwnerError::Denied)?;
        let source = self
            .source
            .after_first_pin(index, updated)
            .map_err(|_| VaultOwnerError::Denied)?;
        let child = source
            .hop_source(index)
            .map_err(|_| VaultOwnerError::Denied)?;
        self.source = source;
        Ok(child)
    }
}
impl Drop for RoutedSessionGesture {
    fn drop(&mut self) {
        self.cancel();
    }
}
