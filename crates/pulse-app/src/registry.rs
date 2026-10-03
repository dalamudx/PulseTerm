//! Bounded application metadata and runtime-window authorization. No terminal lock.
//! Tokens are minted by trusted adapters, never accepted from frontend arguments.
use crate::attachment::{
    self, AttachmentConfig, AttachmentError, AttachmentHandle, AttachmentReceipt,
};
use pulse_core::TermSize;
pub use pulse_term::session::RepairAdmission;
use pulse_term::session::{SessionError, SessionHandle, SessionState};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, MutexGuard,
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
}
struct Entry {
    owner: WindowToken,
    handle: SessionHandle,
    capture: Arc<AsyncMutex<()>>,
    comparison: AsyncMutex<Option<tokio::time::Instant>>,
    marker: AsyncMutex<Option<tokio::time::Instant>>,
    repair: AsyncMutex<Option<tokio::time::Instant>>,
}
struct Binding {
    generation: u64,
    handle: AttachmentHandle,
}
struct State {
    windows: HashSet<WindowToken>,
    sessions: HashMap<SessionId, Arc<Entry>>,
    bindings: HashMap<SessionId, Binding>,
    next_session: u64,
    next_generation: u64,
    closed: bool,
}
impl State {
    fn shutdown(&mut self) {
        self.closed = true;
        self.windows.clear();
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
}
impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
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
    /// Bounds: 32 window incarnations, 64 sessions and at most one binding/session.
    pub fn new() -> Self {
        Self {
            monitors: Arc::new(Semaphore::new(64)),
            state: Arc::new(Mutex::new(State {
                windows: HashSet::new(),
                sessions: HashMap::new(),
                bindings: HashMap::new(),
                next_session: 1,
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
    /// Register an already supervised session. Failure does not close caller's handle.
    /// Duplicate actor registration is rejected; a second id must not bypass
    /// the single-lease/owner/capture gate. Use one registry per application.
    pub fn register(
        &self,
        owner: WindowToken,
        handle: SessionHandle,
    ) -> Result<SessionId, RegistryError> {
        let mut state = self.lock()?;
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
        if state.sessions.len() >= 64 {
            return Err(RegistryError::Busy);
        }
        let id = SessionId(state.next_session);
        state.next_session = state
            .next_session
            .checked_add(1)
            .ok_or(RegistryError::Unavailable)?;
        state.sessions.insert(
            id,
            Arc::new(Entry {
                owner,
                handle,
                capture: Arc::new(AsyncMutex::new(())),
                comparison: AsyncMutex::new(None),
                marker: AsyncMutex::new(None),
                repair: AsyncMutex::new(None),
            }),
        );
        Ok(id)
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
        let entry = Self::entry(&*self.lock()?, owner, id)?;
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
            Self::entry(&state, owner, id)?;
            let n = state.next_generation;
            state.next_generation = n.checked_add(1).ok_or(RegistryError::Unavailable)?;
            n
        };
        let (handle, wire, task) = attachment::attach(&entry.handle, generation, config).await?;
        // A closed window cannot resurrect a view even if capture was already admitted.
        let installed = {
            let mut state = self.lock()?;
            if Self::entry(&state, owner, id).is_ok() {
                if let Some(old) = state.bindings.insert(
                    id,
                    Binding {
                        generation,
                        handle: handle.clone(),
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
    /// remain owned by the application for explicit migration/shutdown, which is not
    /// exposed by this single-owner API yet. Previously admitted input is not retracted.
    pub fn close_window(&self, owner: WindowToken) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        state.windows.remove(&owner);
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
#[cfg(test)]
#[path = "registry_reap_tests.rs"]
mod reap_tests;
pub use comparison::{RenderedScreen, VisibleComparison};
#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
