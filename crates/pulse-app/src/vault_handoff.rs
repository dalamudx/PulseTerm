//! Bounded current-page opaque handoff; native auth is not frontend acceptance.
#[cfg(test)]
#[path = "vault_handoff_tests.rs"]
mod tests;
use crate::registry::{PageLaunchScope, SessionRegistry};
use crate::vault_owner::{
    PageVaultActivation, PageVaultCancellation, PageVaultOwner, VaultControllerSource,
};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use uuid::Uuid;
/// Static handoff failures never include secrets, nonce or source identifiers.
#[derive(Debug, thiserror::Error)]
#[error("vault.handoff_denied")]
pub struct HandoffError;
type Result<T> = std::result::Result<T, HandoffError>;
/// Backend-selected operation kind; ticket cannot be reused for another intent.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HandoffKind {
    /// Initialize an absent Vault explicitly, leaving it locked.
    Create,
    /// Authenticate existing envelope then wait for frontend confirmation.
    Unlock,
}
#[derive(PartialEq, Eq)]
enum Phase {
    Prepared,
    Running,
    Ready,
    Confirming,
}
struct Token {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    kind: HandoffKind,
    live: AtomicBool,
    deadline: Mutex<Instant>,
    cancel: Mutex<Option<PageVaultCancellation>>,
    wake: tokio::sync::watch::Sender<()>,
}
impl Token {
    fn current(&self) -> bool {
        self.live.load(Ordering::Acquire)
            && self
                .deadline
                .lock()
                .is_ok_and(|deadline| Instant::now() < *deadline)
    }
    fn revoke(&self) {
        self.live.store(false, Ordering::Release);
        self.wake.send_replace(());
        if let Some(cancel) = self.cancel.lock().unwrap_or_else(|p| p.into_inner()).take() {
            cancel.cancel();
        }
    }
}
struct Slot {
    token: Arc<Token>,
    phase: Phase,
    activation: Option<PageVaultActivation>,
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.token.revoke();
    }
}
struct State {
    closed: bool,
    slot: Option<Slot>,
}
struct Inner {
    registry: SessionRegistry,
    controller: VaultControllerSource,
    state: Mutex<State>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        state.slot.take();
    }
}
impl Inner {
    fn guard(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        match self.state.lock() {
            Ok(state) => Ok(state),
            Err(poison) => {
                let mut state = poison.into_inner();
                state.closed = true;
                state.slot.take();
                Err(HandoffError)
            }
        }
    }
}
/// One root-owned slot; opaque IDs are page/registry bound, not secret-use authority.
#[derive(Clone)]
pub struct VaultHandoffBank {
    inner: Arc<Inner>,
}
/// Single-use Rust dispatch permit; drop cancels unless a ready receipt was published.
pub struct VaultHandoffPermit {
    owner: Weak<Inner>,
    token: Arc<Token>,
    published: bool,
}
impl Drop for VaultHandoffPermit {
    fn drop(&mut self) {
        if !self.published {
            self.token.revoke();
        }
    }
}
impl VaultHandoffBank {
    /// Trusted native registry provenance; no frontend IDs configure this owner.
    pub fn new(owner: &PageVaultOwner) -> Self {
        let (registry, controller) = owner.handoff_source();
        Self {
            inner: Arc::new(Inner {
                registry,
                controller,
                state: Mutex::new(State {
                    closed: false,
                    slot: None,
                }),
            }),
        }
    }
    /// Prepare one ticket for a confirmed page/intent; existing active ticket is never replaced.
    pub fn prepare(&self, page: Arc<PageLaunchScope>, kind: HandoffKind) -> Result<Uuid> {
        self.inner
            .registry
            .check_launch_page(&page)
            .map_err(|_| HandoffError)?;
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| HandoffError)?;
        let mut state = self.inner.guard()?;
        if state.closed {
            return Err(HandoffError);
        }
        if state.slot.as_ref().is_some_and(|slot| slot.token.current()) {
            return Err(HandoffError);
        }
        state.slot.take();
        let deadline = Instant::now() + Duration::from_secs(300);
        let token = Arc::new(Token {
            id: Uuid::now_v7(),
            page,
            kind,
            live: AtomicBool::new(true),
            deadline: Mutex::new(deadline),
            cancel: Mutex::new(None),
            wake: tokio::sync::watch::channel(()).0,
        });
        let id = token.id;
        state.slot = Some(Slot {
            token: token.clone(),
            phase: Phase::Prepared,
            activation: None,
        });
        drop(state);
        expire(&runtime, Arc::downgrade(&self.inner), token, deadline);
        Ok(id)
    }
    /// Consume prepared stage once; native active/ready phase shares original five-second cutoff.
    pub fn begin(
        &self,
        id: Uuid,
        page: &Arc<PageLaunchScope>,
        kind: HandoffKind,
    ) -> Result<VaultHandoffPermit> {
        self.inner
            .registry
            .check_launch_page(page)
            .map_err(|_| HandoffError)?;
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| HandoffError)?;
        let mut state = self.inner.guard()?;
        if state.closed {
            return Err(HandoffError);
        }
        let slot = state.slot.as_mut().ok_or(HandoffError)?;
        if slot.token.id != id
            || slot.token.kind != kind
            || !Arc::ptr_eq(page, &slot.token.page)
            || !slot.token.current()
            || slot.phase != Phase::Prepared
        {
            return Err(HandoffError);
        }
        slot.phase = Phase::Running;
        let token = slot.token.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        *token.deadline.lock().map_err(|_| HandoffError)? = deadline;
        token.wake.send_replace(());
        drop(state);
        expire(
            &runtime,
            Arc::downgrade(&self.inner),
            token.clone(),
            deadline,
        );
        Ok(VaultHandoffPermit {
            owner: Arc::downgrade(&self.inner),
            token,
            published: false,
        })
    }
    /// Frontend only confirms an already-source-authenticated ready ticket on same current page.
    pub fn confirm(&self, id: Uuid, page: &Arc<PageLaunchScope>) -> Result<()> {
        self.inner
            .registry
            .check_launch_page(page)
            .map_err(|_| HandoffError)?;
        let mut state = self.inner.guard()?;
        if state.closed {
            return Err(HandoffError);
        }
        let slot = state.slot.as_mut().ok_or(HandoffError)?;
        if slot.token.id != id
            || !Arc::ptr_eq(page, &slot.token.page)
            || !slot.token.current()
            || slot.phase != Phase::Ready
        {
            return Err(HandoffError);
        }
        let activation = slot.activation.take().ok_or(HandoffError)?;
        let token = slot.token.clone();
        slot.phase = Phase::Confirming;
        drop(state); // activation rechecks ticket guard; never hold bank lock across it.
        let result = activation.accept().map_err(|_| HandoffError);
        let mut state = self.inner.guard()?;
        if state
            .slot
            .as_ref()
            .is_some_and(|slot| Arc::ptr_eq(&slot.token, &token))
        {
            state.slot.take();
        }
        result
    }
    /// Current-page explicit abort; stale/wrong-page cancellation cannot target a replacement ticket.
    pub fn cancel(&self, id: Uuid, page: &Arc<PageLaunchScope>) -> Result<()> {
        self.inner
            .registry
            .check_launch_page(page)
            .map_err(|_| HandoffError)?;
        let mut state = self.inner.guard()?;
        let slot = state.slot.as_ref().ok_or(HandoffError)?;
        if slot.token.id != id || !Arc::ptr_eq(page, &slot.token.page) {
            return Err(HandoffError);
        }
        state.slot.take();
        Ok(())
    }
    /// Native navigation/destroy synchronously retires any ticket; no IPC result is needed.
    pub fn retire(&self) -> Result<()> {
        self.inner.guard()?.slot.take();
        Ok(())
    }
    /// App closure revokes ticket but does not close shared SQL or root PTY sessions.
    pub fn close(&self) -> Result<()> {
        let mut state = self.inner.guard()?;
        state.closed = true;
        state.slot.take();
        Ok(())
    }
}
impl VaultHandoffPermit {
    /// Retain ticket validity in actual CPU/SQL predicates, not only awaiting future.
    pub fn authorization(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let owner = self.owner.clone();
        let token = self.token.clone();
        Arc::new(move || {
            let Some(owner) = owner.upgrade() else {
                return false;
            };
            if !token.current() || owner.registry.check_launch_page(&token.page).is_err() {
                return false;
            }
            owner.guard().is_ok_and(|state| {
                !state.closed
                    && state.slot.as_ref().is_some_and(|slot| {
                        Arc::ptr_eq(&slot.token, &token) && slot.phase != Phase::Prepared
                    })
            })
        })
    }
    /// Register native operation cancellation before scheduling expensive work.
    pub fn register_cancel(&self, cancel: PageVaultCancellation) -> Result<()> {
        let owner = self.owner.upgrade().ok_or(HandoffError)?;
        if !owner.controller.matches_cancel(&cancel) {
            return Err(HandoffError);
        }
        if !(self.authorization())() {
            cancel.cancel();
            return Err(HandoffError);
        }
        let mut pending = self.token.cancel.lock().map_err(|_| HandoffError)?;
        if pending.is_some() {
            return Err(HandoffError);
        }
        *pending = Some(cancel);
        if !self.token.current() {
            if let Some(cancel) = pending.take() {
                cancel.cancel();
            }
            return Err(HandoffError);
        }
        Ok(())
    }
    /// Store keyless ready receipt; five-second expiry continues, no automatic activation acceptance.
    pub fn publish(mut self, activation: PageVaultActivation) -> Result<()> {
        let owner = self.owner.upgrade().ok_or(HandoffError)?;
        owner
            .registry
            .check_launch_page(&self.token.page)
            .map_err(|_| HandoffError)?;
        let mut state = owner.guard()?;
        if state.closed {
            return Err(HandoffError);
        }
        let slot = state.slot.as_mut().ok_or(HandoffError)?;
        if !self.token.current()
            || !activation.matches_page(&self.token.page)
            || !owner.controller.matches_activation(&activation)
            || !Arc::ptr_eq(&slot.token, &self.token)
            || slot.phase != Phase::Running
            || self.token.kind != HandoffKind::Unlock
        {
            return Err(HandoffError);
        }
        slot.activation = Some(activation);
        slot.phase = Phase::Ready;
        self.published = true;
        Ok(())
    }
    /// Complete initialized-but-locked result, never a key acceptance or unlock grant.
    pub fn finish_creation(mut self) -> Result<()> {
        if self.token.kind != HandoffKind::Create || !(self.authorization())() {
            return Err(HandoffError);
        }
        let owner = self.owner.upgrade().ok_or(HandoffError)?;
        owner.guard()?.slot.take();
        self.published = true;
        Ok(())
    }
}
fn expire(
    runtime: &tokio::runtime::Handle,
    owner: Weak<Inner>,
    token: Arc<Token>,
    deadline: Instant,
) {
    let mut wake = token.wake.subscribe();
    runtime.spawn(async move {
        tokio::select!{_=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>{},_=wake.changed()=>return,}
        let Some(owner) = owner.upgrade() else {
            return;
        };
        let Ok(mut state) = owner.guard() else {
            return;
        };
        if state
            .slot
            .as_ref()
            .is_some_and(|slot| Arc::ptr_eq(&slot.token, &token))
            && !token.current()
        {
            state.slot.take();
        }
    });
}
