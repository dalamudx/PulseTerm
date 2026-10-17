//! Native window routing metadata. No frontend creation, execution or Vault authority.
//! Lock order is directory -> ProductDocuments -> SessionRegistry; never await under it.
use crate::{
    product_document::ProductDocuments,
    registry::{PageLaunchScope, RegistryError, SessionRegistry, WindowToken},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use uuid::{Uuid, Version};

const MAX_WINDOWS: usize = 32;
const PRIMARY: &str = "workspace";

/// Display/routing metadata only; an exact directory lookup is still required.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WindowLabel(String);
impl WindowLabel {
    /// Parse only the primary label or a canonical native-generated UUID label.
    pub fn parse(value: &str) -> Result<Self, RegistryError> {
        if value == PRIMARY {
            return Ok(Self(value.to_owned()));
        }
        let suffix = value
            .strip_prefix("workspace-")
            .ok_or(RegistryError::Forbidden)?;
        let id = Uuid::parse_str(suffix).map_err(|_| RegistryError::Forbidden)?;
        if id.get_version() != Some(Version::Random) || id.to_string() != suffix {
            return Err(RegistryError::Forbidden);
        }
        Ok(Self(value.to_owned()))
    }
    /// Exact label text for metadata display and native routing, never permission.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Building,
    Open,
    Retiring,
    Closing { claimed: bool },
}
struct Entry {
    owner: WindowToken,
    identity: Arc<()>,
    phase: Phase,
}
#[derive(Default)]
struct State {
    entries: HashMap<WindowLabel, Entry>,
    primary_used: bool,
    closed: bool,
}
struct Core {
    registry: SessionRegistry,
    documents: Arc<ProductDocuments>,
    identity: Arc<()>,
    state: Mutex<State>,
}
impl Core {
    fn retire(&self, state: &mut State, label: &WindowLabel) -> Result<(), RegistryError> {
        let entry = state
            .entries
            .get_mut(label)
            .ok_or(RegistryError::Forbidden)?;
        match entry.phase {
            Phase::Closing { .. } => return Ok(()),
            Phase::Retiring => return Err(RegistryError::Unavailable),
            _ => entry.phase = Phase::Retiring,
        }
        // Any failure leaves the slot Retiring and cannot be promoted to completion.
        let document = self.documents.remove(entry.owner);
        let registry = self.registry.close_window(entry.owner);
        document?;
        registry?;
        entry.phase = Phase::Closing { claimed: false };
        Ok(())
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        // Recover poisoned storage only to revoke existing owners, never to admit.
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for entry in state.entries.values() {
            let _ = self.documents.remove(entry.owner);
            let _ = self.registry.close_window(entry.owner);
        }
    }
}
/// Shares the application's registry, but mints and installs each window itself.
#[derive(Clone)]
pub struct WindowDirectory(Arc<Core>);
/// Owned by the native window supervisor. Drop revokes the page, retaining capacity.
pub struct WindowRegistration {
    directory: Weak<Core>,
    label: WindowLabel,
    identity: Arc<()>,
    owner: WindowToken,
}
impl WindowRegistration {
    /// Label allocated for this native window incarnation.
    pub fn label(&self) -> &WindowLabel {
        &self.label
    }
    /// Native-minted registry owner used to construct this window's page controllers.
    pub fn owner(&self) -> WindowToken {
        self.owner
    }
}
impl Drop for WindowRegistration {
    fn drop(&mut self) {
        if let Some(core) = self.directory.upgrade()
            && let Ok(mut state) = core.state.lock()
            && state
                .entries
                .get(&self.label)
                .is_some_and(|entry| Arc::ptr_eq(&entry.identity, &self.identity))
        {
            let _ = core.retire(&mut state, &self.label);
        }
    }
}
/// Original ready document selected by a native adapter; not an execution ticket.
pub struct WindowRoute {
    directory: Arc<()>,
    label: WindowLabel,
    identity: Arc<()>,
    nonce: Uuid,
    scope: Arc<PageLaunchScope>,
}
impl WindowRoute {
    /// Label of the original selection; does not select a later incarnation.
    pub fn label(&self) -> &WindowLabel {
        &self.label
    }
}
/// One cleanup waiter; dropping it cannot release the native window slot.
pub struct WindowDrain {
    directory: Weak<Core>,
    label: WindowLabel,
    identity: Arc<()>,
}
impl WindowDrain {
    /// Window whose native resources this cleanup waiter must drain.
    pub fn label(&self) -> &WindowLabel {
        &self.label
    }
}
impl Drop for WindowDrain {
    fn drop(&mut self) {
        if let Some(core) = self.directory.upgrade()
            && let Ok(mut state) = core.state.lock()
            && let Some(entry) = state.entries.get_mut(&self.label)
            && Arc::ptr_eq(&entry.identity, &self.identity)
            && entry.phase == (Phase::Closing { claimed: true })
        {
            entry.phase = Phase::Closing { claimed: false };
        }
    }
}
impl WindowDirectory {
    /// Create a directory and document controller against the shared app registry.
    pub fn new(registry: SessionRegistry) -> Self {
        Self(Arc::new(Core {
            documents: Arc::new(ProductDocuments::new(registry.clone())),
            registry,
            identity: Arc::new(()),
            state: Mutex::new(State::default()),
        }))
    }
    /// Native document controller shared by these independently owned windows.
    pub fn documents(&self) -> Arc<ProductDocuments> {
        self.0.documents.clone()
    }
    /// Validate the exact native reservation without publishing or granting page authority.
    pub fn registration_owner(
        &self,
        registration: &WindowRegistration,
    ) -> Result<WindowToken, RegistryError> {
        if !Weak::ptr_eq(&registration.directory, &Arc::downgrade(&self.0)) {
            return Err(RegistryError::Forbidden);
        }
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        let entry = state
            .entries
            .get(&registration.label)
            .ok_or(RegistryError::Forbidden)?;
        if state.closed
            || !Arc::ptr_eq(&entry.identity, &registration.identity)
            || !matches!(entry.phase, Phase::Building | Phase::Open)
        {
            return Err(RegistryError::Forbidden);
        }
        Ok(entry.owner)
    }
    /// Reserve before native creation. A cancelled creation still needs native cleanup.
    pub fn reserve(&self, primary: bool) -> Result<WindowRegistration, RegistryError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        if state.closed {
            return Err(RegistryError::Unavailable);
        }
        if primary && state.primary_used {
            return Err(RegistryError::Forbidden);
        }
        if state.entries.len() >= MAX_WINDOWS {
            return Err(RegistryError::Busy);
        }
        let label = if primary {
            WindowLabel(PRIMARY.to_owned())
        } else {
            WindowLabel(format!("workspace-{}", Uuid::new_v4()))
        };
        if state.entries.contains_key(&label) {
            return Err(RegistryError::Unavailable);
        }
        let owner = self.0.registry.open_window()?;
        if let Err(error) = self.0.documents.install(owner) {
            let _ = self.0.registry.close_window(owner);
            return Err(error);
        }
        let identity = Arc::new(());
        state.entries.insert(
            label.clone(),
            Entry {
                owner,
                identity: identity.clone(),
                phase: Phase::Building,
            },
        );
        state.primary_used |= primary;
        Ok(WindowRegistration {
            directory: Arc::downgrade(&self.0),
            label,
            identity,
            owner,
        })
    }
    /// Trusted native caller must first verify the actual local WebView and exact label.
    pub fn publish(&self, registration: &WindowRegistration) -> Result<(), RegistryError> {
        if !Weak::ptr_eq(&registration.directory, &Arc::downgrade(&self.0)) {
            return Err(RegistryError::Forbidden);
        }
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        if state.closed {
            return Err(RegistryError::Unavailable);
        }
        let entry = state
            .entries
            .get_mut(&registration.label)
            .ok_or(RegistryError::Forbidden)?;
        if !Arc::ptr_eq(&entry.identity, &registration.identity) || entry.phase != Phase::Building {
            return Err(RegistryError::Forbidden);
        }
        entry.phase = Phase::Open;
        Ok(())
    }
    /// List published labels only; readiness and authorization require a fresh route.
    pub fn list(&self) -> Result<Vec<WindowLabel>, RegistryError> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        if state.closed {
            return Err(RegistryError::Unavailable);
        }
        let mut labels: Vec<_> = state
            .entries
            .iter()
            .filter(|(_, entry)| entry.phase == Phase::Open)
            .map(|(label, _)| label.clone())
            .collect();
        labels.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(labels)
    }
    /// An omitted selector is valid only with exactly one published window, even if
    /// another published window is still loading. Never guess a focused/default window.
    pub fn select(&self, label: Option<&WindowLabel>) -> Result<WindowRoute, RegistryError> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        if state.closed {
            return Err(RegistryError::Unavailable);
        }
        let (label, entry) = if let Some(label) = label {
            let entry = state
                .entries
                .get(label)
                .filter(|entry| entry.phase == Phase::Open)
                .ok_or(RegistryError::Forbidden)?;
            (label, entry)
        } else {
            let mut open = state
                .entries
                .iter()
                .filter(|(_, entry)| entry.phase == Phase::Open);
            let first = open.next().ok_or(RegistryError::Forbidden)?;
            if open.next().is_some() {
                return Err(RegistryError::Forbidden);
            }
            first
        };
        let (nonce, scope) = self.0.documents.current_native_document(entry.owner)?;
        Ok(WindowRoute {
            directory: self.0.identity.clone(),
            label: label.clone(),
            identity: entry.identity.clone(),
            nonce,
            scope,
        })
    }
    /// Recheck after awaits and native-thread handoffs. Returned page scope still
    /// requires the ordinary Capability/launch/Vault admission at the effect boundary.
    pub fn current(&self, route: &WindowRoute) -> Result<Arc<PageLaunchScope>, RegistryError> {
        if !Arc::ptr_eq(&route.directory, &self.0.identity) {
            return Err(RegistryError::Forbidden);
        }
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        if state.closed {
            return Err(RegistryError::Unavailable);
        }
        let entry = state
            .entries
            .get(&route.label)
            .ok_or(RegistryError::Forbidden)?;
        if entry.phase != Phase::Open || !Arc::ptr_eq(&entry.identity, &route.identity) {
            return Err(RegistryError::Forbidden);
        }
        let scope = self.0.documents.current(entry.owner, route.nonce)?;
        if !Arc::ptr_eq(&scope, &route.scope) {
            return Err(RegistryError::Forbidden);
        }
        Ok(scope)
    }
    /// Revoke one window immediately. Another window's page remains untouched.
    /// May also reclaim a cleanup waiter dropped after registration cancellation.
    pub fn begin_close(&self, label: &WindowLabel) -> Result<WindowDrain, RegistryError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        self.0.retire(&mut state, label)?;
        let entry = state
            .entries
            .get_mut(label)
            .ok_or(RegistryError::Forbidden)?;
        if entry.phase != (Phase::Closing { claimed: false }) {
            return Err(RegistryError::Busy);
        }
        entry.phase = Phase::Closing { claimed: true };
        Ok(WindowDrain {
            directory: Arc::downgrade(&self.0),
            label: label.clone(),
            identity: entry.identity.clone(),
        })
    }
    /// Native-only completion: call ONLY after the actual window and its pending
    /// operations have drained. Permission revocation / waiter cancellation is not proof.
    pub fn complete_close(&self, drain: WindowDrain) -> Result<(), RegistryError> {
        if !Weak::ptr_eq(&drain.directory, &Arc::downgrade(&self.0)) {
            return Err(RegistryError::Forbidden);
        }
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        let entry = state
            .entries
            .get(&drain.label)
            .ok_or(RegistryError::Forbidden)?;
        if !Arc::ptr_eq(&entry.identity, &drain.identity)
            || entry.phase != (Phase::Closing { claimed: true })
        {
            return Err(RegistryError::Forbidden);
        }
        state.entries.remove(&drain.label);
        Ok(())
    }
    /// Deny new routing/creation, revoke all pages and retain every slot for drain.
    pub fn shutdown(&self) -> Result<(), RegistryError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| RegistryError::Unavailable)?;
        state.closed = true;
        let mut labels: Vec<_> = state.entries.keys().cloned().collect();
        labels.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let mut failure = None;
        for label in labels {
            if let Err(error) = self.0.retire(&mut state, &label)
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
#[cfg(test)]
#[path = "window_directory_tests.rs"]
mod tests;
