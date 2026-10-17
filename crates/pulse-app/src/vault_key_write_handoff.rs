//! One original-page private-key ciphertext write ticket; no password-ticket or SSH-use authority.
use crate::{
    registry::{PageLaunchScope, SessionRegistry},
    vault_owner::{
        PageVaultCancellation, PageVaultOwner, PrivateKeyIdentityWriteIntent, VaultControllerSource,
    },
};
use pulse_store::{StateStore, vault_crypto::SecretBytes, vault_database::VaultSnapshot};
use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use uuid::Uuid;
/// Static metadata failure, without any password, nonce or source details.
#[derive(Debug, thiserror::Error)]
#[error("vault.private_key_edit_denied")]
pub struct PrivateKeyEditError;
type Result<T> = std::result::Result<T, PrivateKeyEditError>;
struct Token {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    live: AtomicBool,
    deadline: Instant,
    cancel: PageVaultCancellation,
    wake: tokio::sync::watch::Sender<()>,
}
impl Token {
    fn current(&self) -> bool {
        self.live.load(Ordering::Acquire) && Instant::now() < self.deadline
    }
    fn revoke(&self) {
        self.live.store(false, Ordering::Release);
        self.cancel.cancel();
        self.wake.send_replace(());
    }
}
struct Slot {
    token: Arc<Token>,
    intent: Option<PrivateKeyIdentityWriteIntent>,
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
impl Inner {
    fn guard(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        match self.state.lock() {
            Ok(s) => Ok(s),
            Err(p) => {
                let mut s = p.into_inner();
                s.closed = true;
                s.slot.take();
                Err(PrivateKeyEditError)
            }
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        let s = self.state.get_mut().unwrap_or_else(|p| p.into_inner());
        s.closed = true;
        s.slot.take();
    }
}
/// One ready/active frontend ticket. Actual I/O capacity belongs to the original native owner,
/// and remains reserved after a waiter/ticket is cancelled until its predicates actually exit.
#[derive(Clone)]
pub struct PrivateKeyWriteBank {
    inner: Arc<Inner>,
}
/// Single-use typed native dispatch; drop revokes this operation, never a later epoch/master key.
pub struct PrivateKeyWritePermit {
    owner: Weak<Inner>,
    token: Arc<Token>,
    intent: Option<PrivateKeyIdentityWriteIntent>,
}
impl Drop for PrivateKeyWritePermit {
    fn drop(&mut self) {
        self.token.revoke();
        if let Some(owner) = self.owner.upgrade()
            && let Ok(mut state) = owner.guard()
            && state
                .slot
                .as_ref()
                .is_some_and(|s| Arc::ptr_eq(&s.token, &self.token))
        {
            state.slot.take();
        }
    }
}
impl PrivateKeyWriteBank {
    /// Derive registry/controller provenance only from the assigned native owner.
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
    /// Publish one intent for five seconds; runtime timer retires it even without another frontend call.
    pub fn publish(
        &self,
        page: Arc<PageLaunchScope>,
        intent: PrivateKeyIdentityWriteIntent,
    ) -> Result<Uuid> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| PrivateKeyEditError)?;
        let mut state = self.inner.guard()?;
        if state.closed
            || state.slot.is_some()
            || self.inner.registry.check_launch_page(&page).is_err()
            || !intent.bound_to(&self.inner.controller, &page)
        {
            return Err(PrivateKeyEditError);
        }
        let (wake, mut changed) = tokio::sync::watch::channel(());
        let token = Arc::new(Token {
            id: Uuid::new_v4(),
            page,
            live: AtomicBool::new(true),
            deadline: Instant::now() + Duration::from_secs(5),
            cancel: intent.cancellation(),
            wake,
        });
        let id = token.id;
        let deadline = token.deadline;
        state.slot = Some(Slot {
            token,
            intent: Some(intent),
        });
        drop(state);
        let weak = Arc::downgrade(&self.inner);
        runtime.spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                    if let Some(owner)=weak.upgrade() && let Ok(mut state)=owner.guard() && state.slot.as_ref().is_some_and(|s|s.token.id==id) {state.slot.take();}
                }
                _ = changed.changed() => {}
            }
        });
        Ok(id)
    }
    /// Take exactly once on the original page; malformed/foreign/duplicate tickets cannot consume another.
    pub fn take(&self, page: &Arc<PageLaunchScope>, id: Uuid) -> Result<PrivateKeyWritePermit> {
        self.inner
            .registry
            .check_launch_page(page)
            .map_err(|_| PrivateKeyEditError)?;
        let mut state = self.inner.guard()?;
        if state.closed {
            return Err(PrivateKeyEditError);
        }
        let slot = state.slot.as_mut().ok_or(PrivateKeyEditError)?;
        if slot.token.id != id || !Arc::ptr_eq(&slot.token.page, page) || !slot.token.current() {
            return Err(PrivateKeyEditError);
        }
        let intent = slot.intent.as_ref().ok_or(PrivateKeyEditError)?;
        if !intent.bound_to(&self.inner.controller, page) {
            return Err(PrivateKeyEditError);
        }
        let intent = slot.intent.take().ok_or(PrivateKeyEditError)?;
        Ok(PrivateKeyWritePermit {
            owner: Arc::downgrade(&self.inner),
            token: slot.token.clone(),
            intent: Some(intent),
        })
    }
    /// Current-page cancellation also revokes already taken native work; root I/O credit stays until exit.
    pub fn cancel(&self, page: &Arc<PageLaunchScope>, id: Uuid) -> Result<()> {
        self.inner
            .registry
            .check_launch_page(page)
            .map_err(|_| PrivateKeyEditError)?;
        let mut state = self.inner.guard()?;
        let slot = state.slot.as_ref().ok_or(PrivateKeyEditError)?;
        if slot.token.id != id || !Arc::ptr_eq(&slot.token.page, page) {
            return Err(PrivateKeyEditError);
        }
        state.slot.take();
        Ok(())
    }
    /// Native navigation retirement; no page request is needed to cancel ready/running work.
    pub fn retire(&self) -> Result<()> {
        self.inner.guard()?.slot.take();
        Ok(())
    }
    /// Root shutdown closes publication independently of the shared database/session lifetime.
    pub fn close(&self) -> Result<()> {
        let mut state = self.inner.guard()?;
        state.closed = true;
        state.slot.take();
        Ok(())
    }
}
impl PrivateKeyWritePermit {
    /// Recheck only the original page/controller/epoch and deadline; grants no new authority.
    pub fn check_admission(&self) -> Result<()> {
        let owner = self.owner.upgrade().ok_or(PrivateKeyEditError)?;
        owner
            .registry
            .check_launch_page(&self.token.page)
            .map_err(|_| PrivateKeyEditError)?;
        let intent = self.intent.as_ref().ok_or(PrivateKeyEditError)?;
        if !self.token.current() || !intent.bound_to(&owner.controller, &self.token.page) {
            return Err(PrivateKeyEditError);
        }
        Ok(())
    }
    /// Weak cancellation receipt for a supervised native waiter; holds no I/O permit or key material.
    pub fn cancellation(&self) -> PageVaultCancellation {
        self.token.cancel.clone()
    }
    /// Dispatch once using original source/epoch, enforcing original cutoff inside final SQL predicates.
    pub async fn write(
        mut self,
        store: &StateStore,
        key: SecretBytes,
        pass: Option<SecretBytes>,
    ) -> Result<VaultSnapshot> {
        let owner = self.owner.upgrade().ok_or(PrivateKeyEditError)?;
        owner
            .registry
            .check_launch_page(&self.token.page)
            .map_err(|_| PrivateKeyEditError)?;
        if !self.token.current() {
            return Err(PrivateKeyEditError);
        }
        let intent = self.intent.take().ok_or(PrivateKeyEditError)?;
        let token = self.token.clone();
        let result = intent
            .write(store, key, pass, Arc::new(move || token.current()))
            .await
            .map_err(|_| PrivateKeyEditError)?;
        if !self.token.current() || owner.registry.check_launch_page(&self.token.page).is_err() {
            return Err(PrivateKeyEditError);
        }
        Ok(result)
    }
}
