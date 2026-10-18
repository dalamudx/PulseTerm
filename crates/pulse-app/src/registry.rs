//! Bounded application metadata and runtime-window authorization. No terminal lock.
//! Tokens are minted by trusted adapters, never accepted from frontend arguments.
/// Keyless current-page admission for a separately confirmed public host-key action.
#[path = "registry_trust_action.rs"]
pub mod trust_action;
use crate::attachment::{
    self, AttachmentConfig, AttachmentError, AttachmentHandle, AttachmentReceipt,
};
use pulse_core::TermSize;
pub use pulse_term::session::RepairAdmission;
use pulse_term::session::{SessionError, SessionHandle, SessionState};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore, mpsc, oneshot};

static NEXT_WINDOW: AtomicU64 = AtomicU64::new(1);
/// In-process capability for one runtime window incarnation; not serializable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowToken(u64);
/// Registry-local session id. A session id is never an authorization capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);
impl SessionId {
    /// Adapter-facing integer; serialize as a decimal string, not JS number.
    pub fn get(self) -> u64 {
        self.0
    }
}
/// Stable admission failures; no host/session content is exposed.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// Missing session and wrong owner intentionally share one response.
    #[error("session access denied")]
    Forbidden,
    /// No current binding with this exact generation; no driver credit is changed.
    #[error("stale binding")]
    Stale,
    /// Bounded registry or concurrent capture admission is full.
    #[error("registry busy")]
    Busy,
    /// Exhaustion/poison/shutdown fails closed.
    #[error("registry unavailable")]
    Unavailable,
    /// Native/actor input validation or lifecycle error.
    #[error(transparent)]
    Session(#[from] SessionError),
    /// Restore can explicitly defer; unsuccessful capture preserves the old binding.
    #[error(transparent)]
    Attachment(#[from] AttachmentError),
    /// Bounded read-only history query or an expired history cursor.
    #[error(transparent)]
    History(#[from] pulse_term::HistoryPageError),
}
struct Entry {
    owner: WindowToken,
    handle: SessionHandle,
    capture: Arc<AsyncMutex<()>>,
    comparison: Arc<AsyncMutex<Option<tokio::time::Instant>>>,
    marker: Arc<AsyncMutex<Option<tokio::time::Instant>>>,
    repair: Arc<AsyncMutex<Option<tokio::time::Instant>>>,
    shell_markers: Arc<AsyncMutex<Option<tokio::time::Instant>>>,
}
impl Entry {
    fn with_owner(&self, owner: WindowToken) -> Arc<Self> {
        Arc::new(Self {
            owner,
            handle: self.handle.clone(),
            capture: self.capture.clone(),
            comparison: self.comparison.clone(),
            marker: self.marker.clone(),
            repair: self.repair.clone(),
            shell_markers: self.shell_markers.clone(),
        })
    }
}
struct Binding {
    generation: u64,
    page: Option<u64>,
    handle: AttachmentHandle,
    settings: Option<crate::settings_context::SettingsContextOwner>,
    theme: Option<crate::theme_context::ThemeContextOwner>,
}
struct PendingLaunch {
    owner: WindowToken,
    page: Option<u64>,
    revoked: bool,
}
struct State {
    windows: HashSet<WindowToken>,
    sessions: HashMap<SessionId, Arc<Entry>>,
    bindings: HashMap<SessionId, Binding>,
    next_session: u64,
    next_launch: u64,
    launches: HashMap<u64, PendingLaunch>,
    pages: HashMap<WindowToken, u64>,
    next_page: u64,
    next_generation: u64,
    closed: bool,
}
impl State {
    fn launch_current(&self, owner: WindowToken, serial: u64) -> bool {
        !self.closed
            && self.windows.contains(&owner)
            && self.launches.get(&serial).is_some_and(|launch| {
                launch.owner == owner
                    && !launch.revoked
                    && launch
                        .page
                        .is_none_or(|page| self.pages.get(&owner) == Some(&page))
            })
    }

    fn retire_page(&mut self, owner: WindowToken, page: u64) {
        if self.pages.get(&owner) != Some(&page) {
            return;
        }
        self.pages.remove(&owner);
        let views: Vec<_> = self
            .bindings
            .iter()
            .filter(|(id, binding)| {
                binding.page == Some(page)
                    && self
                        .sessions
                        .get(id)
                        .is_some_and(|entry| entry.owner == owner)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in views {
            if let Some(binding) = self.bindings.remove(&id) {
                binding.handle.detach();
            }
        }
        for launch in self.launches.values_mut() {
            if launch.owner == owner && launch.page == Some(page) {
                launch.revoked = true;
            }
        }
    }

    fn shutdown(&mut self) {
        self.closed = true;
        self.windows.clear();
        self.pages.clear();
        for launch in self.launches.values_mut() {
            launch.revoked = true;
        }
        for (_, binding) in self.bindings.drain() {
            binding.handle.detach();
        }
        for (_, entry) in self.sessions.drain() {
            entry.handle.close();
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.shutdown();
    }
}
/// Owns registered handles, not native join tasks. Registrars must supervise the
/// corresponding SessionTask/spawn_local receipt until shutdown completes.
#[derive(Clone)]
pub struct SessionRegistry {
    state: Arc<Mutex<State>>,
    monitors: Arc<Semaphore>,
    pub(crate) native_workers: crate::native_lifecycle::RegisteredNativeWorkers,
}
impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}
/// Backend-only current document lifetime, minted only AFTER a native adapter has verified
/// the real WindowToken, local origin, and the current document nonce. This Rust value is
/// not proof that the adapter performed those checks and must never be minted by page IPC.
#[must_use]
pub struct PageLaunchScope {
    state: Weak<Mutex<State>>,
    owner: WindowToken,
    page: u64,
}
impl PageLaunchScope {
    /// Backend origin metadata only, not current-page authority or permission to mint a scope.
    pub fn owner(&self) -> WindowToken {
        self.owner
    }
}
impl Drop for PageLaunchScope {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade()
            && let Ok(mut state) = state.lock()
        {
            state.retire_page(self.owner, self.page);
        }
    }
}

/// A revocable, backend-only pre-spawn slot. Not a page permission or process owner.
/// Drop releases quota; closing a window or registry revokes but does not free it.
/// The supervising adapter must keep this ticket alive through blocking spawn/cleanup;
/// dropping an in-flight spawn future alone is NOT a safe process cancellation strategy.
#[must_use]
pub struct LocalLaunchTicket {
    state: Weak<Mutex<State>>,
    owner: WindowToken,
    serial: u64,
    active: bool,
}
impl LocalLaunchTicket {
    // Unknown cleanup must not silently release the logical startup slot.
    pub(crate) fn retain_failed(&mut self) {
        self.active = false;
    }
    /// Cheap pre-spawn snapshot check. This cannot replace commit: a window can be
    /// retired after the check, while native setup runs without the registry lock.
    pub(crate) fn require_current(&self) -> Result<(), RegistryError> {
        let state = self.state.upgrade().ok_or(RegistryError::Forbidden)?;
        let state = state.lock().map_err(|_| RegistryError::Unavailable)?;
        if !self.active || !state.launch_current(self.owner, self.serial) {
            return Err(RegistryError::Forbidden);
        }
        Ok(())
    }
}
impl Drop for LocalLaunchTicket {
    fn drop(&mut self) {
        if self.active
            && let Some(state) = self.state.upgrade()
            && let Ok(mut state) = state.lock()
        {
            state.launches.remove(&self.serial);
        }
    }
}
/// Wire and completion observation for one generation. Dropping wire detaches it;
/// dropping completion alone does not revoke it. Errors require surface cancellation.
pub struct RegisteredAttachment {
    /// Globally monotonic within this registry, including failed capture attempts.
    pub generation: u64,
    /// Same binary/credit stream as the window-neutral attachment driver.
    pub wire: mpsc::Receiver<Vec<u8>>,
    /// Supervised terminal result; old completion cannot remove a replacement.
    pub completion: oneshot::Receiver<Result<AttachmentReceipt, AttachmentError>>,
}
impl SessionRegistry {
    /// Prevent new registered native work, then observe its real physical completion.
    /// This does NOT close root owners/actors or cover unregistered native adapters.
    pub fn seal_native_workers(&self) -> crate::native_lifecycle::RegisteredNativeDrain {
        self.native_workers.seal()
    }
    /// Compatibility spelling; the shared ledger includes protocol adapters as well as local PTYs.
    pub fn seal_local_workers(&self) -> crate::native_lifecycle::RegisteredNativeDrain {
        self.seal_native_workers()
    }
    pub(crate) fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
    /// Bounds: 32 window incarnations, 64 sessions and at most one binding/session.
    pub fn new() -> Self {
        Self {
            monitors: Arc::new(Semaphore::new(64)),
            native_workers: crate::native_lifecycle::RegisteredNativeWorkers::new(),
            state: Arc::new(Mutex::new(State {
                windows: HashSet::new(),
                sessions: HashMap::new(),
                bindings: HashMap::new(),
                next_session: 1,
                next_launch: 1,
                launches: HashMap::new(),
                pages: HashMap::new(),
                next_page: 1,
                next_generation: 1,
                closed: false,
            })),
        }
    }
    fn lock(&self) -> Result<MutexGuard<'_, State>, RegistryError> {
        self.state.lock().map_err(|_| RegistryError::Unavailable)
    }
    /// Called only from runtime window creation. Labels must not identify ownership:
    /// a recreated window with the same label must receive a fresh token.
    pub fn open_window(&self) -> Result<WindowToken, RegistryError> {
        let mut state = self.lock()?;
        if state.closed {
            return Err(RegistryError::Unavailable);
        }
        if state.windows.len() >= 32 {
            return Err(RegistryError::Busy);
        }
        let token = WindowToken(
            NEXT_WINDOW
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| RegistryError::Unavailable)?,
        );
        state.windows.insert(token);
        Ok(token)
    }
    fn entry(
        state: &State,
        owner: WindowToken,
        id: SessionId,
    ) -> Result<Arc<Entry>, RegistryError> {
        if state.closed || !state.windows.contains(&owner) {
            return Err(RegistryError::Forbidden);
        }
        state
            .sessions
            .get(&id)
            .filter(|e| e.owner == owner)
            .cloned()
            .ok_or(RegistryError::Forbidden)
    }
    /// Activate a fresh page generation only after the native adapter has verified the
    /// current document. Rotating invalidates pending launches, not registered sessions.
    pub fn activate_launch_page(
        &self,
        owner: WindowToken,
    ) -> Result<PageLaunchScope, RegistryError> {
        let mut state = self.lock()?;
        if state.closed || !state.windows.contains(&owner) {
            return Err(RegistryError::Forbidden);
        }
        let page = state.next_page;
        let next = page.checked_add(1).ok_or(RegistryError::Unavailable)?;
        if let Some(old) = state.pages.get(&owner).copied() {
            state.retire_page(owner, old);
        }
        state.next_page = next;
        state.pages.insert(owner, page);
        Ok(PageLaunchScope {
            state: Arc::downgrade(&self.state),
            owner,
            page,
        })
    }

    /// Reserve one of 64 slots BEFORE spawning a child, for a trusted runtime window.
    /// The native adapter must independently authorize the current page/nonce and Profile;
    /// this reservation alone grants no page, path or executable permission.
    pub fn begin_local_launch(
        &self,
        owner: WindowToken,
    ) -> Result<LocalLaunchTicket, RegistryError> {
        self.begin_launch(owner, None)
    }

    /// Require exactly the current native-confirmed page generation at reservation time.
    pub fn begin_page_local_launch(
        &self,
        page: &PageLaunchScope,
    ) -> Result<LocalLaunchTicket, RegistryError> {
        self.begin_launch(page.owner, Some(page))
    }

    /// Read-only generation check for a backend-owned page projection after an await.
    /// This takes no launch credit and grants no durable permission: callers must
    /// validate again when they actually reserve a child or publish a result.
    pub fn check_launch_page(&self, page: &PageLaunchScope) -> Result<(), RegistryError> {
        self.check_page_locked(&*self.lock()?, page)
    }
    /// Project a native Root snapshot to sessions owned by this exact current page's window.
    /// Candidates are backend metadata, never frontend IDs; the result grants no view,
    /// close, transfer or input permission. Existing effect admission remains mandatory.
    pub fn owned_page_sessions(
        &self,
        page: &PageLaunchScope,
        candidates: &[SessionId],
    ) -> Result<Vec<SessionId>, RegistryError> {
        let state = self.lock()?;
        self.check_page_locked(&state, page)?;
        Ok(candidates
            .iter()
            .copied()
            .filter(|id| {
                state
                    .sessions
                    .get(id)
                    .is_some_and(|entry| entry.owner == page.owner)
            })
            .collect())
    }
    #[cfg(feature = "ssh-use")]
    pub(crate) fn check_protocol_ticket_page(
        &self,
        ticket: &LocalLaunchTicket,
        page: &PageLaunchScope,
    ) -> Result<(), RegistryError> {
        if !ticket.active
            || !ticket
                .state
                .upgrade()
                .is_some_and(|s| Arc::ptr_eq(&s, &self.state))
        {
            return Err(RegistryError::Forbidden);
        }
        let state = self.lock()?;
        self.check_page_locked(&state, page)?;
        if ticket.owner != page.owner
            || !state.launch_current(ticket.owner, ticket.serial)
            || state
                .launches
                .get(&ticket.serial)
                .is_none_or(|launch| launch.page != Some(page.page))
        {
            return Err(RegistryError::Forbidden);
        }
        Ok(())
    }
    fn check_page_locked(
        &self,
        state: &State,
        page: &PageLaunchScope,
    ) -> Result<(), RegistryError> {
        if !page
            .state
            .upgrade()
            .is_some_and(|s| Arc::ptr_eq(&s, &self.state))
            || state.closed
            || !state.windows.contains(&page.owner)
            || state.pages.get(&page.owner) != Some(&page.page)
        {
            return Err(RegistryError::Forbidden);
        }
        Ok(())
    }

    fn begin_launch(
        &self,
        owner: WindowToken,
        page: Option<&PageLaunchScope>,
    ) -> Result<LocalLaunchTicket, RegistryError> {
        if page.is_some_and(|page| {
            !page
                .state
                .upgrade()
                .is_some_and(|state| Arc::ptr_eq(&state, &self.state))
        }) {
            return Err(RegistryError::Forbidden);
        }
        let mut state = self.lock()?;
        if state.closed
            || !state.windows.contains(&owner)
            || page.is_some_and(|page| state.pages.get(&owner) != Some(&page.page))
        {
            return Err(RegistryError::Forbidden);
        }
        if state.sessions.len() + state.launches.len() >= 64 {
            return Err(RegistryError::Busy);
        }
        let serial = state.next_launch;
        state.next_launch = serial.checked_add(1).ok_or(RegistryError::Unavailable)?;
        state.launches.insert(
            serial,
            PendingLaunch {
                owner,
                page: page.map(|scope| scope.page),
                revoked: false,
            },
        );
        Ok(LocalLaunchTicket {
            state: Arc::downgrade(&self.state),
            owner,
            serial,
            active: true,
        })
    }
    /// Commit a reserved slot after supervised spawn, rechecking ownership atomically.
    /// Failure retains the reservation: the caller must close and join the native
    /// child BEFORE dropping the ticket, so cleanup cannot free spawn capacity early.
    pub fn commit_local_launch(
        &self,
        ticket: &mut LocalLaunchTicket,
        handle: SessionHandle,
    ) -> Result<SessionId, RegistryError> {
        if !ticket.active
            || !ticket
                .state
                .upgrade()
                .is_some_and(|state| Arc::ptr_eq(&state, &self.state))
        {
            return Err(RegistryError::Forbidden);
        }
        let mut state = self.lock()?;
        if !state.launch_current(ticket.owner, ticket.serial) {
            return Err(RegistryError::Forbidden);
        }
        let id = Self::register_locked(&mut state, ticket.owner, handle, Some(ticket.serial))?;
        ticket.active = false;
        Ok(id)
    }
    /// Register an already supervised session. Failure does not close caller's handle.
    /// Duplicate actor registration is rejected; a second id must not bypass
    /// the single-lease/owner/capture gate. Use one registry per application.
    pub fn register(
        &self,
        owner: WindowToken,
        handle: SessionHandle,
    ) -> Result<SessionId, RegistryError> {
        let mut state = self.lock()?;
        Self::register_locked(&mut state, owner, handle, None)
    }
    fn register_locked(
        state: &mut State,
        owner: WindowToken,
        handle: SessionHandle,
        reserved: Option<u64>,
    ) -> Result<SessionId, RegistryError> {
        if state.closed || !state.windows.contains(&owner) {
            return Err(RegistryError::Forbidden);
        }
        if state
            .sessions
            .values()
            .any(|entry| entry.handle.same_actor(&handle))
        {
            return Err(RegistryError::Forbidden);
        }
        let occupied = state.sessions.len() + state.launches.len();
        if occupied > 64 || (reserved.is_none() && occupied >= 64) {
            return Err(RegistryError::Busy);
        }
        let id = SessionId(state.next_session);
        let next = state
            .next_session
            .checked_add(1)
            .ok_or(RegistryError::Unavailable)?;
        // All fallible validation has completed. Replace this one reserved slot with
        // the registered actor under the same short lock, never freeing quota early.
        if let Some(serial) = reserved {
            debug_assert!(state.launches.remove(&serial).is_some());
        }
        state.next_session = next;
        state.sessions.insert(
            id,
            Arc::new(Entry {
                owner,
                handle,
                capture: Arc::new(AsyncMutex::new(())),
                comparison: Arc::new(AsyncMutex::new(None)),
                marker: Arc::new(AsyncMutex::new(None)),
                repair: Arc::new(AsyncMutex::new(None)),
                shell_markers: Arc::new(AsyncMutex::new(None)),
            }),
        );
        Ok(id)
    }
    /// Trusted backend ownership handoff between two live window incarnations.
    /// Never expose runtime tokens as frontend arguments. Target UI confirmation and
    /// geometry/attach coordination belong to the adapter, not this transaction.
    /// Capture already in progress returns Busy with no effects; same-owner is a no-op.
    /// Old bindings are revoked atomically, but already admitted native writes/resizes
    /// cannot be rolled back. No session close, spawn, capture, input or ACK occurs here.
    pub fn transfer(
        &self,
        source: WindowToken,
        target: WindowToken,
        id: SessionId,
    ) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        let entry = Self::entry(&state, source, id)?;
        if !state.windows.contains(&target) {
            return Err(RegistryError::Forbidden);
        }
        if source == target {
            return Ok(());
        }
        let _capture = entry
            .capture
            .clone()
            .try_lock_owned()
            .map_err(|_| RegistryError::Busy)?;
        if entry.handle.state.borrow().lifecycle != pulse_term::session::Lifecycle::Running {
            return Err(RegistryError::Session(SessionError::Closed));
        }
        let next = entry.with_owner(target);
        if let Some(binding) = state.bindings.remove(&id) {
            binding.handle.detach();
        }
        state.sessions.insert(id, next);
        Ok(())
    }
    /// Trusted navigation teardown: keep window capability/session ownership and
    /// actors, but revoke every current view and invalidate pending captures.
    /// The adapter must first invalidate the old page's nonce/epoch so requests not
    /// yet admitted to this registry cannot start after teardown. Not a frontend RPC.
    /// Native writes already admitted are not retractable; a new page must attach.
    /// Unlike transfer this never waits for capture: old Entry identities expire
    /// while the shared try-lock continues bounding old and new capture work.
    pub fn retire_window_views(&self, owner: WindowToken) -> Result<usize, RegistryError> {
        let mut state = self.lock()?;
        if !state.windows.contains(&owner) {
            return Err(RegistryError::Forbidden);
        }
        if let Some(page) = state.pages.get(&owner).copied() {
            state.retire_page(owner, page);
        }
        let entries: Vec<_> = state
            .sessions
            .iter()
            .filter(|(_, e)| e.owner == owner)
            .map(|(id, e)| (*id, e.with_owner(owner)))
            .collect();
        let mut retired = 0;
        for (id, next) in entries {
            if let Some(binding) = state.bindings.remove(&id) {
                binding.handle.detach();
                retired += 1;
            }
            state.sessions.insert(id, next);
        }
        Ok(retired)
    }

    fn current_entry(
        state: &State,
        owner: WindowToken,
        id: SessionId,
        expected: &Arc<Entry>,
    ) -> Result<(), RegistryError> {
        let current = Self::entry(state, owner, id)?;
        if !Arc::ptr_eq(&current, expected) {
            return Err(RegistryError::Forbidden);
        }
        Ok(())
    }

    /// Resolve a frontend decimal id only after trusted runtime ownership checks.
    pub fn resolve(&self, owner: WindowToken, raw: u64) -> Result<SessionId, RegistryError> {
        let id = SessionId(raw);
        Self::entry(&*self.lock()?, owner, id)?;
        Ok(id)
    }
    /// Snapshot watch metadata, not mutable terminal state.
    pub fn state(&self, owner: WindowToken, id: SessionId) -> Result<SessionState, RegistryError> {
        Ok(Self::entry(&*self.lock()?, owner, id)?
            .handle
            .state
            .borrow()
            .clone())
    }
    /// Generation-bound viewer input. Revocation before actor admission forbids it;
    /// success is NOT a physical-write receipt, nor rollback on later replacement.
    pub async fn input_bound(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        bytes: Vec<u8>,
    ) -> Result<(), RegistryError> {
        let binding = {
            let state = self.lock()?;
            Self::entry(&state, owner, id)?;
            state
                .bindings
                .get(&id)
                .filter(|b| b.generation == generation)
                .ok_or(RegistryError::Stale)?
                .handle
                .clone()
        };
        binding.input(bytes).await?;
        let state = self.lock()?;
        Self::entry(&state, owner, id)?;
        if state
            .bindings
            .get(&id)
            .is_none_or(|b| b.generation != generation)
        {
            return Err(RegistryError::Stale);
        }
        Ok(())
    }

    /// Admit input on behalf of the trusted current runtime window.
    pub async fn input(
        &self,
        owner: WindowToken,
        id: SessionId,
        bytes: Vec<u8>,
    ) -> Result<(), RegistryError> {
        let entry = Self::entry(&*self.lock()?, owner, id)?;
        entry.handle.input(bytes).await?;
        Ok(())
    }
    /// Generation-bound native resize. Success revokes the old lease, so callers
    /// must attach a fresh generation; no post-await old-binding equality is required.
    /// Owner remains checked; admitted native effects are not rollback-capable.
    pub async fn resize_bound(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        size: TermSize,
    ) -> Result<(), RegistryError> {
        let binding = {
            let state = self.lock()?;
            Self::entry(&state, owner, id)?;
            state
                .bindings
                .get(&id)
                .filter(|b| b.generation == generation)
                .ok_or(RegistryError::Stale)?
                .handle
                .clone()
        };
        binding.resize(size).await?;
        Self::entry(&*self.lock()?, owner, id)?;
        Ok(())
    }

    /// Native resize invalidates the view generation through the driver's status.
    pub async fn resize(
        &self,
        owner: WindowToken,
        id: SessionId,
        size: TermSize,
    ) -> Result<(), RegistryError> {
        let entry = Self::entry(&*self.lock()?, owner, id)?;
        entry.handle.resize(size).await?;
        Ok(())
    }
    /// No metadata mutex is held across await. Per-session try-lock bounds concurrent
    /// capture admission, and an ownership recheck handles close during capture.
    pub async fn attach(
        &self,
        owner: WindowToken,
        id: SessionId,
        config: AttachmentConfig,
    ) -> Result<RegisteredAttachment, RegistryError> {
        self.attach_version(owner, id, config, false, None, false)
            .await
    }
    /// Explicit dual-coordinate codec; identical trusted-owner and generation admission.
    pub async fn attach_v2(
        &self,
        owner: WindowToken,
        id: SessionId,
        config: AttachmentConfig,
    ) -> Result<RegisteredAttachment, RegistryError> {
        self.attach_version(owner, id, config, true, None, false)
            .await
    }
    /// Product-document capture: check the backend page scope before admission and
    /// atomically with binding installation after capture. Not a frontend capability.
    pub async fn attach_page(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        config: AttachmentConfig,
    ) -> Result<RegisteredAttachment, RegistryError> {
        self.attach_version(page.owner, id, config, false, Some(page), false)
            .await
    }
    /// Dual-coordinate version of `attach_page`; does not accept a page-supplied owner.
    pub async fn attach_page_v2(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        config: AttachmentConfig,
    ) -> Result<RegisteredAttachment, RegistryError> {
        self.attach_version(page.owner, id, config, true, Some(page), false)
            .await
    }
    pub(crate) async fn attach_page_v2_retaining_ended(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        config: AttachmentConfig,
    ) -> Result<RegisteredAttachment, RegistryError> {
        self.attach_version(page.owner, id, config, true, Some(page), true)
            .await
    }
    async fn attach_version(
        &self,
        owner: WindowToken,
        id: SessionId,
        config: AttachmentConfig,
        v2: bool,
        page: Option<&PageLaunchScope>,
        retain_ended: bool,
    ) -> Result<RegisteredAttachment, RegistryError> {
        let entry = {
            let state = self.lock()?;
            if let Some(page) = page {
                self.check_page_locked(&state, page)?;
            }
            Self::entry(&state, owner, id)?
        };
        let monitor = self
            .monitors
            .clone()
            .try_acquire_owned()
            .map_err(|_| RegistryError::Busy)?;
        let _capture = entry
            .capture
            .clone()
            .try_lock_owned()
            .map_err(|_| RegistryError::Busy)?;
        let generation = {
            let mut state = self.lock()?;
            Self::current_entry(&state, owner, id, &entry)?;
            if let Some(page) = page {
                self.check_page_locked(&state, page)?;
            }
            let n = state.next_generation;
            state.next_generation = n.checked_add(1).ok_or(RegistryError::Unavailable)?;
            n
        };
        let (handle, wire, task) = if v2 {
            attachment::attach_v2(&entry.handle, generation, config).await?
        } else {
            attachment::attach(&entry.handle, generation, config).await?
        };
        // A closed window cannot resurrect a view even if capture was already admitted.
        let installed = {
            let mut state = self.lock()?;
            if Self::current_entry(&state, owner, id, &entry).is_ok()
                && page.is_none_or(|p| self.check_page_locked(&state, p).is_ok())
            {
                if let Some(old) = state.bindings.insert(
                    id,
                    Binding {
                        generation,
                        page: page.map(|p| p.page),
                        handle: handle.clone(),
                        settings: None,
                        theme: None,
                    },
                ) {
                    old.handle.detach();
                }
                true
            } else {
                false
            }
        };
        if !installed {
            handle.detach();
            let _ = task.join().await;
            return Err(RegistryError::Forbidden);
        }
        let weak = Arc::downgrade(&self.state);
        let (finished, completion) = oneshot::channel();
        tokio::spawn(async move {
            let _monitor = monitor;
            let result = task.join().await;
            if let Some(state) = weak.upgrade()
                && let Ok(mut state) = state.lock()
                && state
                    .bindings
                    .get(&id)
                    .is_some_and(|b| b.generation == generation)
                && !(retain_ended
                    && result.as_ref().is_ok_and(|r| {
                        r.boundary.lifecycle == pulse_term::session::Lifecycle::Ended
                    }))
            {
                state.bindings.remove(&id);
            }
            let _ = finished.send(result);
        });
        Ok(RegisteredAttachment {
            generation,
            wire,
            completion,
        })
    }
    /// Read-only runtime admission check. Does not reserve a lease, acknowledge
    /// output, send input, or promise that asynchronous work will remain current.
    pub fn check_binding(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<(), RegistryError> {
        let state = self.lock()?;
        Self::entry(&state, owner, id)?;
        if state
            .bindings
            .get(&id)
            .is_some_and(|b| b.generation == generation)
        {
            Ok(())
        } else {
            Err(RegistryError::Stale)
        }
    }

    /// Wrong-window/stale ACK is rejected BEFORE forwarding; it cannot kill the new
    /// driver. Duplicate/invalid current ACK remains a protocol error supervised there.
    pub fn acknowledge(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        sequence: u32,
    ) -> Result<(), RegistryError> {
        let state = self.lock()?;
        Self::entry(&state, owner, id)?;
        let binding = state
            .bindings
            .get(&id)
            .filter(|b| b.generation == generation)
            .ok_or(RegistryError::Stale)?;
        binding.handle.acknowledge(generation, sequence)?;
        Ok(())
    }
    /// Only this generation may detach; a delayed old detach cannot revoke the new one.
    pub fn detach(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        Self::entry(&state, owner, id)?;
        if state
            .bindings
            .get(&id)
            .is_none_or(|b| b.generation != generation)
        {
            return Err(RegistryError::Stale);
        }
        if let Some(binding) = state.bindings.remove(&id) {
            binding.handle.detach();
        }
        Ok(())
    }
    /// Explicit session close is different from view/window close.
    pub fn close_session(&self, owner: WindowToken, id: SessionId) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        let entry = Self::entry(&state, owner, id)?;
        if let Some(binding) = state.bindings.remove(&id) {
            binding.handle.detach();
        }
        state.sessions.remove(&id);
        entry.handle.close();
        Ok(())
    }
    /// Revoke this incarnation's capabilities and bindings, NOT its sessions. Handles
    /// remain owned by the application for shutdown. Explicit transfer requires both
    /// windows still alive BEFORE closing the source. Already admitted input is not retracted.
    pub fn close_window(&self, owner: WindowToken) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        state.windows.remove(&owner);
        state.pages.remove(&owner);
        for launch in state.launches.values_mut() {
            if launch.owner == owner {
                launch.revoked = true;
            }
        }
        let ids: Vec<_> = state
            .sessions
            .iter()
            .filter(|(_, e)| e.owner == owner)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(binding) = state.bindings.remove(&id) {
                binding.handle.detach();
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn test_registered_session_count(&self) -> Result<usize, RegistryError> {
        Ok(self.lock()?.sessions.len())
    }
    /// Trusted registrar cleanup after joining a terminated actor, including after
    /// its owner window was revoked. Never expose this as a frontend command.
    /// Running sessions must survive window destruction; they cannot be reaped.
    pub fn reap_closed(&self, id: SessionId) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        let entry = state.sessions.get(&id).ok_or(RegistryError::Forbidden)?;
        if entry.handle.state.borrow().lifecycle == pulse_term::session::Lifecycle::Running {
            return Err(RegistryError::Busy);
        }
        if let Some(binding) = state.bindings.remove(&id) {
            binding.handle.detach();
        }
        state.sessions.remove(&id);
        Ok(())
    }
    /// Application shutdown revokes bindings and closes all session handles. Registrar
    /// supervisors still own native worker joins; this call is not an OS-tree join.
    pub fn shutdown(&self) -> Result<(), RegistryError> {
        self.lock()?.shutdown();
        Ok(())
    }
}
#[path = "registry_comparison.rs"]
mod comparison;
#[path = "registry_history.rs"]
mod history;
#[cfg(test)]
#[path = "registry_history_tests.rs"]
mod history_tests;
#[path = "registry_page_binding.rs"]
mod page_binding;
#[path = "registry_palette.rs"]
mod palette;
#[cfg(test)]
#[path = "registry_reap_tests.rs"]
mod reap_tests;
#[path = "registry_settings.rs"]
mod settings;
#[path = "registry_shell.rs"]
mod shell;
#[path = "registry_theme.rs"]
mod theme;
pub use comparison::{RenderedScreen, VisibleComparison};
pub use palette::CapturedQueryPalette;
pub use settings::{
    RegisteredSettingsCandidate, RegisteredSettingsCapture, RegisteredSettingsSnapshot,
    RegistrySettingsError, SourceBoundSettingsSnapshot,
};
pub use theme::{
    RegisteredThemeCandidate, RegisteredThemeRead, RegisteredThemeSnapshot, RegistryThemeError,
};
#[cfg(test)]
#[path = "registry_launch_tests.rs"]
mod launch_tests;
#[cfg(test)]
#[path = "registry_page_attach_tests.rs"]
mod page_attach_tests;
#[cfg(test)]
#[path = "registry_page_launch_tests.rs"]
mod page_launch_tests;
#[cfg(test)]
#[path = "registry_page_sessions_tests.rs"]
mod page_sessions_tests;
#[cfg(test)]
#[path = "registry_retire_tests.rs"]
mod retire_tests;
#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "registry_transfer_tests.rs"]
mod transfer_tests;
#[cfg(test)]
#[path = "registry_v2_tests.rs"]
mod v2_tests;
