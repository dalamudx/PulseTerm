//! Bounded root storage, independent of document lifetime. This is not page authorization.
//! All consumers must separately verify WindowToken/current page/view generation.
use crate::{
    local_launch::{LocalStartError, RegisteredLocalSession, StoredProfileLaunch},
    local_session_owner::{CleanupState, LocalSessionCleanup, LocalSessionOwner},
    registry::{PageLaunchScope, RegistryError, SessionId, SessionRegistry},
};
use pulse_store::StateStore;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{runtime::Handle, sync::watch};

/// Maximum root entries, including closing, awaiting EOF consumption and failed cleanup.
/// Registry admission also remains bounded independently.
pub const MAX_ROOT_LOCAL_SESSIONS: usize = 64;

/// Runtime-owned collection. Protect short mutations in native state, never hold its
/// lock across native join or IPC. Dropping/closing a page must NOT drop this collection.
pub struct LocalSessionRoot {
    registry: SessionRegistry,
    runtime: Handle,
    owners: HashMap<SessionId, LocalSessionOwner>,
    // Shares occupied()/retiring and both existing registry budgets; not a second pool.
    protocols: HashMap<SessionId, crate::protocol_session_owner::ProtocolSessionOwner>,
    retiring: HashMap<SessionId, watch::Receiver<CleanupState>>,
    closing_handles: HashMap<SessionId, pulse_term::session::SessionHandle>,
    closed: bool,
    appearances: HashMap<SessionId, pulse_store::profile_editor::ProfileAppearance>,
}
impl LocalSessionRoot {
    /// Bind root ownership to the exact runtime and registry which created its sessions.
    pub fn new(registry: SessionRegistry, runtime: Handle) -> Self {
        Self {
            registry,
            runtime,
            owners: HashMap::new(),
            protocols: HashMap::new(),
            retiring: HashMap::new(),
            closing_handles: HashMap::new(),
            closed: false,
            appearances: HashMap::new(),
        }
    }
    /// Adopt a committed session. Rejection drops its same-source owner, closes the
    /// child and supervises cleanup rather than leaving an unowned running process.
    /// Success grants NO page authority. IDs are never accepted as capabilities.
    pub fn adopt(
        &mut self,
        registered: RegisteredLocalSession,
    ) -> Result<SessionId, RegistryError> {
        self.adopt_owned(LocalSessionOwner::adopt(self.runtime.clone(), registered))
    }
    /// Adopt an already-root-owned same-registry session. Rejected owners still Drop
    /// into supervised native cleanup; observations alone cannot keep them alive.
    pub fn adopt_owned(&mut self, owner: LocalSessionOwner) -> Result<SessionId, RegistryError> {
        if self.closed || !owner.is_managed_by(&self.registry) {
            return Err(RegistryError::Forbidden);
        }
        self.collect_reaped();
        let id = owner.id();
        if self.owners.contains_key(&id)
            || self.protocols.contains_key(&id)
            || self.retiring.contains_key(&id)
        {
            return Err(RegistryError::Stale);
        }
        if self.occupied() >= MAX_ROOT_LOCAL_SESSIONS {
            return Err(RegistryError::Busy);
        }
        self.owners.insert(id, owner);
        Ok(id)
    }
    /// Trusted adapter preflight, not a reservation or page/host/credential permission.
    /// Final adoption rechecks capacity/source; native ledger remains reserved through actual exit.
    pub fn preflight_protocol(&mut self, registry: &SessionRegistry) -> Result<(), RegistryError> {
        if self.closed || !self.registry.same_instance(registry) {
            return Err(RegistryError::Forbidden);
        }
        self.collect_reaped();
        if self.occupied() >= MAX_ROOT_LOCAL_SESSIONS {
            return Err(RegistryError::Busy);
        }
        Ok(())
    }
    /// Adopt a same-source protocol owner into the SAME root capacity and exit drain.
    /// This does not authorize a host, credential, page, view or terminal operation.
    pub fn adopt_protocol(
        &mut self,
        owner: crate::protocol_session_owner::ProtocolSessionOwner,
    ) -> Result<SessionId, RegistryError> {
        if self.closed || !owner.is_managed_by(&self.registry) {
            return Err(RegistryError::Forbidden);
        }
        self.collect_reaped();
        let id = owner.id();
        if self.owners.contains_key(&id)
            || self.protocols.contains_key(&id)
            || self.retiring.contains_key(&id)
        {
            return Err(RegistryError::Stale);
        }
        if self.occupied() >= MAX_ROOT_LOCAL_SESSIONS {
            return Err(RegistryError::Busy);
        }
        self.appearances.insert(id, owner.appearance().clone());
        self.protocols.insert(id, owner);
        Ok(id)
    }
    /// Trusted root-owned actor access, NOT a page/window/view/input permission.
    /// Frontend adapters must use current-page registry bindings, never expose this handle or callback.
    pub fn protocol_handle(
        &self,
        id: SessionId,
    ) -> Result<pulse_term::session::SessionHandle, RegistryError> {
        self.protocols
            .get(&id)
            .map(|owner| owner.handle().clone())
            .ok_or(RegistryError::Forbidden)
    }
    /// Backend ownership-kind metadata only, never permission to operate on the session.
    pub fn is_protocol(&self, id: SessionId) -> bool {
        self.protocols.contains_key(&id)
    }
    // A preflight only. Adoption must check again because another startup may finish first.
    fn check_admission(&mut self, registry: &SessionRegistry) -> Result<(), RegistryError> {
        if self.closed || !self.registry.same_instance(registry) {
            return Err(RegistryError::Forbidden);
        }
        self.collect_reaped();
        if self.occupied() >= MAX_ROOT_LOCAL_SESSIONS {
            return Err(RegistryError::Busy);
        }
        Ok(())
    }
    /// Root-owned identifiers only. This must never be returned to a page without
    /// filtering through registry ownership and current-document authorization.
    pub fn ids(&self) -> Vec<SessionId> {
        let mut ids: Vec<_> = self.owners.keys().copied().collect();
        ids.extend(self.protocols.keys().copied());
        ids.sort_by_key(|id| id.get());
        ids
    }
    /// Drawing metadata only. A native adapter must separately authorize the current page/view.
    pub fn profile_appearance(
        &self,
        id: SessionId,
    ) -> pulse_store::profile_editor::ProfileAppearance {
        self.appearances.get(&id).cloned().unwrap_or_default()
    }
    /// Occupied root slots. Closing, failed or unobserved native tasks retain capacity.
    /// This count is intentionally conservative until collect_reaped is called.
    pub fn occupied(&self) -> usize {
        self.owners.len() + self.protocols.len() + self.retiring.len()
    }
    /// Consume natural completion only AFTER view/EOF consumers finish. The returned
    /// owner must be finished or dropped: cancelling its future supervises close/join.
    /// Its slot stays occupied until successful native cleanup, even with no waiter.
    pub fn take_for_finish(&mut self, id: SessionId) -> Result<LocalSessionOwner, RegistryError> {
        let owner = self.owners.remove(&id).ok_or(RegistryError::Forbidden)?;
        self.retiring.insert(id, owner.observe_cleanup());
        self.closing_handles.insert(id, owner.handle().clone());
        Ok(owner)
    }
    /// Explicit root session close. Native join proceeds if the returned waiter drops.
    /// Callers still need separate current-page/session authorization before invoking.
    pub fn close(&mut self, id: SessionId) -> Result<LocalSessionCleanup, RegistryError> {
        Ok(self.take_for_finish(id)?.cancel())
    }
    /// Take a protocol owner while retaining the same root slot until actual successful reap.
    pub fn take_protocol_for_finish(
        &mut self,
        id: SessionId,
    ) -> Result<crate::protocol_session_owner::ProtocolSessionOwner, RegistryError> {
        let owner = self.protocols.remove(&id).ok_or(RegistryError::Forbidden)?;
        self.retiring.insert(id, owner.observe_cleanup());
        self.closing_handles.insert(id, owner.handle().clone());
        Ok(owner)
    }
    /// Explicit protocol close; callers must independently authorize the current page/session.
    pub fn close_protocol(
        &mut self,
        id: SessionId,
    ) -> Result<crate::protocol_session_owner::ProtocolSessionCleanup, RegistryError> {
        Ok(self.take_protocol_for_finish(id)?.cancel())
    }
    /// Remove only slots whose SAME owner proved successful native completion/reap.
    /// A failed/dropped watch or closed actor is not evidence of native cleanup.
    pub fn collect_reaped(&mut self) -> usize {
        let previous = self.retiring.len();
        self.retiring
            .retain(|_, observer| *observer.borrow() != CleanupState::Reaped);
        self.appearances.retain(|id, _| {
            self.owners.contains_key(id)
                || self.protocols.contains_key(id)
                || self.retiring.contains_key(id)
        });
        self.closing_handles
            .retain(|id, _| self.retiring.contains_key(id));
        previous - self.retiring.len()
    }
    /// Stop further adoption and request close on every owned session. The runtime must
    /// stay alive to join these waiters AND any already-retiring operations. This method
    /// does not itself prove application shutdown or drop failed-capacity records.
    pub fn close_all(&mut self) -> Vec<LocalSessionCleanup> {
        self.closed = true;
        for handle in self.closing_handles.values() {
            handle.close();
        }
        let protocols: Vec<_> = self.protocols.keys().copied().collect();
        for id in protocols {
            drop(self.close_protocol(id));
        }
        let ids: Vec<_> = self.owners.keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| self.close(id).ok())
            .collect()
    }
    /// Seal native admission BEFORE requesting close, including pending startups which
    /// have not yet been adopted. No mutex is held while awaiting this receipt.
    /// Cancelling the waiter does not undo sealing or abort actual cleanup.
    pub fn shutdown(&mut self) -> LocalRootDrain {
        let native = self.registry.seal_local_workers();
        drop(self.close_all());
        LocalRootDrain {
            native,
            owners: self.retiring.values().cloned().collect(),
        }
    }
}
/// Generic name for the same bounded root. Legacy local APIs remain source-compatible.
pub type SessionRoot = LocalSessionRoot;

/// Observe shutdown of this root's tracked local/protocol workers/owners. Caller owns a
/// finite deadline AND must retain the runtime on error/timeout. This does not
/// prove settings shutdown, descendant-tree kill, global application exit or OS cleanup.
#[must_use]
pub struct LocalRootDrain {
    native: crate::native_lifecycle::RegisteredNativeDrain,
    owners: Vec<watch::Receiver<CleanupState>>,
}
impl LocalRootDrain {
    /// Await native worker completion and every captured same-owner Reaped receipt.
    /// Failure or cancellation does not grant permission to tear down the runtime.
    pub async fn join(self) -> Result<(), LocalStartError> {
        self.native.join().await?;
        for mut owner in self.owners {
            loop {
                match *owner.borrow_and_update() {
                    CleanupState::Reaped => break,
                    CleanupState::Failed => return Err(LocalStartError::Worker),
                    CleanupState::Owned | CleanupState::Closing => {}
                }
                owner.changed().await.map_err(|_| LocalStartError::Worker)?;
            }
        }
        Ok(())
    }
}
/// Fresh private preferences → current page → native startup → same-registry root.
/// Candidates and environment must be backend-owned; this is NOT an IPC entry point.
/// No root lock is held across storage or native work. Dropping the future before
/// handoff cancels pending startup; after adoption the effect is root-owned even if
/// navigation races its response. The native adapter must reauthorize that response.
/// Root preflight is not a reservation: a concurrent full/closed root rejects and
/// supervises the newly committed child. Native registry capacity remains occupied
/// until real cleanup; this function's error is not a cleanup-completion receipt.
pub async fn start_stored_page_local(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    store: &StateStore,
    root: &Arc<Mutex<LocalSessionRoot>>,
    spec: StoredProfileLaunch<'_>,
) -> Result<SessionId, LocalStartError> {
    start_stored_page_local_with_preset(
        registry,
        page,
        store,
        root,
        spec,
        pulse_store::settings::TerminalPreset::Classic,
    )
    .await
}

/// Native-only initial terminal selection, frozen before the child can answer color queries.
pub async fn start_stored_page_local_with_preset(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    store: &StateStore,
    root: &Arc<Mutex<LocalSessionRoot>>,
    spec: StoredProfileLaunch<'_>,
    preset: impl Into<pulse_store::settings::TerminalStartup>,
) -> Result<SessionId, LocalStartError> {
    registry.check_launch_page(page)?;
    let runtime = {
        let mut state = root.lock().map_err(|_| LocalStartError::Worker)?;
        state.check_admission(registry)?;
        state.runtime.clone()
    };
    let (pending, appearance) =
        crate::local_launch::start_page_stored_configured_local_with_preset(
            registry, page, store, spec, preset,
        )
        .await?;
    let registered = pending.wait().await?;
    // No await after accepting registration and before transferring its owner.
    let owner = LocalSessionOwner::adopt(runtime, registered);
    let mut state = root.lock().map_err(|_| LocalStartError::Worker)?;
    let id = state.adopt_owned(owner)?;
    state.appearances.insert(id, appearance);
    Ok(id)
}
/// Start only a saved custom Profile under the original page and the shared root exit barrier.
/// The native adapter must separately authorize publication of the returned session ID.
pub async fn start_custom_page_local_with_preset(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    store: &StateStore,
    root: &Arc<Mutex<LocalSessionRoot>>,
    spec: crate::local_launch::CustomProfileLaunch<'_>,
    startup: pulse_store::settings::TerminalStartup,
) -> Result<SessionId, LocalStartError> {
    registry.check_launch_page(page)?;
    let runtime = {
        let mut state = root.lock().map_err(|_| LocalStartError::Worker)?;
        state.check_admission(registry)?;
        state.runtime.clone()
    };
    let (pending, appearance) = crate::local_launch::start_page_custom_local_with_preset(
        registry, page, store, spec, startup,
    )
    .await?;
    let registered = pending.wait().await?;
    let owner = LocalSessionOwner::adopt(runtime, registered);
    let mut state = root.lock().map_err(|_| LocalStartError::Worker)?;
    let id = state.adopt_owned(owner)?;
    state.appearances.insert(id, appearance);
    Ok(id)
}
#[cfg(all(test, unix))]
#[path = "local_session_shutdown_tests.rs"]
mod shutdown_tests;
#[cfg(all(test, unix))]
#[path = "local_session_root_stored_tests.rs"]
mod stored_tests;
#[cfg(all(test, unix))]
#[path = "local_session_root_tests.rs"]
mod tests;
