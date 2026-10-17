//! Keyless action lifetime, not a public key, trust decision, credential or connection grant.
use super::{PageLaunchScope, SessionRegistry, WindowToken};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use tokio::{
    sync::watch,
    time::{Duration, Instant},
};
use uuid::Uuid;
/// Static errors disclose no page, host, target or storage metadata.
#[derive(Debug, thiserror::Error)]
pub enum TrustActionError {
    /// Wrong/retired page, owner or host identity.
    #[error("trust_action.denied")]
    Denied,
    /// An earlier action or its actual native reservation is still alive.
    #[error("trust_action.busy")]
    Busy,
    /// The controller lock is unavailable.
    #[error("trust_action.unavailable")]
    Unavailable,
}
type Result<T> = std::result::Result<T, TrustActionError>;
struct Inner {
    registry: SessionRegistry,
    window: WindowToken,
    closed: AtomicBool,
    slot: Mutex<Weak<Token>>,
    changed: watch::Sender<()>,
}
struct Token {
    owner: Weak<Inner>,
    page: Arc<PageLaunchScope>,
    host: Uuid,
    expires: Instant,
    live: AtomicBool,
    cancelled: watch::Sender<bool>,
}
impl Drop for Token {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            owner.changed.send_replace(());
        }
    }
}
impl Token {
    fn current(&self) -> bool {
        self.live.load(Ordering::Acquire)
            && Instant::now() < self.expires
            && self.owner.upgrade().is_some_and(|owner| {
                !owner.closed.load(Ordering::Acquire)
                    && !owner.slot.is_poisoned()
                    && self.page.owner == owner.window
                    && owner.registry.check_launch_page(&self.page).is_ok()
            })
    }
    fn cancel(&self) {
        self.live.store(false, Ordering::Release);
        self.cancelled.send_replace(true);
    }
}
/// One fixed native window, one outstanding action across page generations.
/// A native adapter must verify origin, WindowToken and document nonce before supplying a scope.
/// Navigation must call `retire` synchronously; this owner never closes SQL or sessions.
/// No Vault is required and no key/controller is retained.
pub struct PageTrustActions {
    inner: Arc<Inner>,
}
impl PageTrustActions {
    /// Create a backend-owned controller. A wrong registry/window cannot admit a page.
    pub fn new(registry: SessionRegistry, window: WindowToken) -> Self {
        Self {
            inner: Arc::new(Inner {
                registry,
                window,
                closed: AtomicBool::new(false),
                slot: Mutex::new(Weak::new()),
                changed: watch::channel(()).0,
            }),
        }
    }
    /// Begin one backend-selected host action. Host ID is not endpoint or credential authority.
    /// Five-minute expiry starts before any source read or dialing; it cannot be extended by UI.
    pub fn begin(&self, page: Arc<PageLaunchScope>, host: Uuid) -> Result<TrustActionOperation> {
        if host.get_version_num() != 7 || host.get_variant() != uuid::Variant::RFC4122 {
            return Err(TrustActionError::Denied);
        }
        let mut slot = self
            .inner
            .slot
            .lock()
            .map_err(|_| TrustActionError::Unavailable)?;
        if self.inner.closed.load(Ordering::Acquire)
            || page.owner != self.inner.window
            || self.inner.registry.check_launch_page(&page).is_err()
        {
            return Err(TrustActionError::Denied);
        }
        if slot.strong_count() != 0 {
            return Err(TrustActionError::Busy);
        }
        let (cancelled, _) = watch::channel(false);
        let token = Arc::new(Token {
            owner: Arc::downgrade(&self.inner),
            page,
            host,
            expires: Instant::now() + Duration::from_secs(300),
            live: AtomicBool::new(true),
            cancelled,
        });
        *slot = Arc::downgrade(&token);
        Ok(TrustActionOperation { token })
    }
    /// Revoke but retain occupied admission until all actual native guards exit.
    /// Signals wake only after this controller's lock has been released.
    pub fn retire(&self) {
        let token = {
            self.inner
                .slot
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .upgrade()
        };
        if let Some(token) = token {
            token.cancel();
        }
    }
    /// Wait for all owned actions and actual native predicates to drop. Close first
    /// when using this as an exit barrier; cancellation signals alone are not idle.
    /// The adapter must attach authorization to the actual stream/blocking closure.
    pub async fn wait_idle(&self) -> Result<()> {
        let mut changed = self.inner.changed.subscribe();
        loop {
            drop(changed.borrow_and_update());
            let idle = self
                .inner
                .slot
                .lock()
                .map_err(|_| TrustActionError::Unavailable)?
                .strong_count()
                == 0;
            if idle {
                return Ok(());
            }
            changed
                .changed()
                .await
                .map_err(|_| TrustActionError::Unavailable)?;
        }
    }
    /// Permanently revoke this owner without affecting its registry's root sessions.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        self.retire();
    }
}
impl Drop for PageTrustActions {
    fn drop(&mut self) {
        self.close();
    }
}
/// Original page/host action. Dropping it revokes work, not merely the awaiting request.
/// The adapter must keep it through observation, explicit decision and native confirmation.
/// ```compile_fail
/// fn dump(v:pulse_app::registry::trust_action::TrustActionOperation){println!("{v:?}");}
/// ```
pub struct TrustActionOperation {
    token: Arc<Token>,
}
impl TrustActionOperation {
    /// Backend-selected saved host identity; never a connection permission.
    pub fn host(&self) -> Uuid {
        self.token.host
    }
    /// Check original page and action at a short non-awaiting admission boundary.
    pub fn is_current(&self) -> bool {
        self.token.current()
    }
    /// Native predicate owns admission until its actual I/O/SQL closure/stream exits.
    /// It retains only the keyless token and a weak owner, not registry/key controllers.
    pub fn authorization(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let token = self.token.clone();
        Arc::new(move || token.current())
    }
    /// Metadata-only wait handle; retaining it does not retain admission or this owner.
    pub fn cancellation(&self) -> TrustActionCancellation {
        TrustActionCancellation {
            receiver: self.token.cancelled.subscribe(),
            expires: self.token.expires,
        }
    }
    /// Weak-only caller cancellation for a separately supervised operation.
    /// Holding this handle never retains admission or the owner.
    pub fn revocation(&self) -> TrustActionRevocation {
        TrustActionRevocation {
            token: Arc::downgrade(&self.token),
        }
    }
    /// Explicit cancellation does not free a native reservation prematurely.
    pub fn cancel(&self) {
        self.token.cancel();
    }
}
impl Drop for TrustActionOperation {
    fn drop(&mut self) {
        self.token.cancel();
    }
}
/// Weak-only revocation handle; never an approval or native completion receipt.
#[derive(Clone)]
pub struct TrustActionRevocation {
    token: Weak<Token>,
}
impl TrustActionRevocation {
    /// Cancel the original action if it still exists; cannot affect a newer token.
    pub fn cancel(&self) {
        if let Some(token) = self.token.upgrade() {
            token.cancel();
        }
    }
}
/// Cancellation/expiry evidence only, not a source check or actual native-exit receipt.
#[derive(Clone)]
pub struct TrustActionCancellation {
    receiver: watch::Receiver<bool>,
    expires: Instant,
}
impl TrustActionCancellation {
    /// Await cancel, owner/token destruction or original fixed deadline without polling.
    /// Dropping this waiter alone does not revoke an operation.
    pub async fn wait(mut self) {
        loop {
            if *self.receiver.borrow() {
                return;
            }
            tokio::select! {
                _=tokio::time::sleep_until(self.expires)=>return,
                changed=self.receiver.changed()=>if changed.is_err(){return;},
            }
        }
    }
}
#[cfg(test)]
#[path = "registry_trust_action_tests.rs"]
mod tests;
