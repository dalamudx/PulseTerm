//! Backend-only original-page action preparation, not a CLI authenticator or an executor.
//! The trusted adapter must authenticate callers and resolve target windows before preparation.
//! Effect adapters still recheck their own native/session/source authority at the actual commit.
use crate::registry::{PageLaunchScope, SessionRegistry};
use pulse_cap::{Catalogue, Risk, Scope, Surface};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
/// Static preparation failures never contain action parameters or client source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Catalogue visibility or typed parameter checks failed.
    #[error("cap.invalid_request")]
    Request,
    /// This original window/page is retired, foreign, wrong or outside the window scope.
    #[error("cap.forbidden")]
    Forbidden,
    /// Required original-page GUI confirmation is missing.
    #[error("cap.confirmation_required")]
    Confirmation,
    /// The original preparation deadline expired; it cannot be renewed by confirmation.
    #[error("cap.expired")]
    Expired,
}
/// Single-use private original request, never serialized, cloned, or Debug-formatted.
/// A successful preparation or confirmation grants no native execution permission.
pub struct PreparedWindowAction {
    page: Arc<PageLaunchScope>,
    registry: SessionRegistry,
    id: String,
    args: Value,
    risk: Risk,
    confirmed: bool,
    deadline: Instant,
}
impl PreparedWindowAction {
    /// Trusted backend preparation only; callers must separately authenticate the entry surface
    /// and resolve the actual original page. No client-provided window token or callback is accepted.
    pub fn prepare(
        catalogue: &Catalogue,
        registry: &SessionRegistry,
        page: Arc<PageLaunchScope>,
        id: &str,
        surface: Surface,
        args: Value,
    ) -> Result<Self, Error> {
        let (scope, risk) = catalogue.policy(id, surface).map_err(|_| Error::Request)?;
        if scope != Scope::Window {
            return Err(Error::Forbidden);
        }
        registry
            .check_launch_page(&page)
            .map_err(|_| Error::Forbidden)?;
        let args = catalogue
            .checked_args(id, surface, args)
            .map_err(|_| Error::Request)?;
        registry
            .check_launch_page(&page)
            .map_err(|_| Error::Forbidden)?;
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(300))
            .ok_or(Error::Expired)?;
        Ok(Self {
            page,
            registry: registry.clone(),
            id: id.into(),
            args,
            risk,
            confirmed: risk == Risk::Safe,
            deadline,
        })
    }
    fn check(&self, original: &Arc<PageLaunchScope>) -> Result<(), Error> {
        if !Arc::ptr_eq(&self.page, original) {
            return Err(Error::Forbidden);
        }
        if Instant::now() >= self.deadline {
            return Err(Error::Expired);
        }
        self.registry
            .check_launch_page(&self.page)
            .map_err(|_| Error::Forbidden)
    }
    /// Consume one trusted GUI confirmation of this exact original request and page.
    /// The adapter must verify real local WebView source/current nonce before calling this.
    /// Safe actions and duplicate confirmation cannot enter this path.
    pub fn confirm(mut self, original: &Arc<PageLaunchScope>) -> Result<Self, Error> {
        self.check(original)?;
        if self.risk == Risk::Safe || self.confirmed {
            return Err(Error::Confirmation);
        }
        self.confirmed = true;
        Ok(self)
    }
    /// Consume arguments for this exact original request after a final point-in-time page check.
    /// This is not a launch, input, native I/O or durable session permission: the actual effect
    /// adapter must use this same original scope and its existing commit-time authority checks.
    pub fn take_args(
        self,
        original: &Arc<PageLaunchScope>,
        expected_id: &str,
    ) -> Result<Value, Error> {
        self.check(original)?;
        if self.id != expected_id {
            return Err(Error::Forbidden);
        }
        if !self.confirmed {
            return Err(Error::Confirmation);
        }
        Ok(self.args)
    }
}
#[cfg(test)]
mod tests;
