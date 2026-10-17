//! Backend theme selection and explicitly confirmed read publication. No file paths, watcher or GUI.
//! Context cancellation retains its single capture slot until IO really exits (or candidate is dropped).
//! Closing this context never closes shared ThemeIoOwner instances. Source changes need trusted invalidation.
use pulse_store::{
    theme::ThemeDocument,
    theme_io::{ThemeIoError, ThemeIoHandle},
};
use std::sync::{Arc, Mutex, Weak};
use tokio::{
    runtime::Handle,
    sync::{oneshot, watch},
    task::JoinHandle,
};
/// Static lifecycle errors never format theme text, paths, names or identifiers.
#[derive(Debug, thiserror::Error)]
pub enum ThemeContextError {
    /// Owner closed/dropped.
    #[error("theme_context.closed")]
    Closed,
    /// Capture owns its slot through actual IO exit or explicit candidate disposal.
    #[error("theme_context.busy")]
    Busy,
    /// Selection/invalidation changed after admission.
    #[error("theme_context.stale")]
    Stale,
    /// Candidate belongs to another owner, even if both selected the same file.
    #[error("theme_context.foreign_candidate")]
    Foreign,
    /// Fail closed, never wrap a counter.
    #[error("theme_context.counter_exhausted")]
    Exhausted,
    /// Poisoning fails closed.
    #[error("theme_context.state_failed")]
    State,
    /// Selected file missing; no implicit default or replacement theme.
    #[error("theme_context.missing")]
    Missing,
    /// Supervisor exited without a receipt.
    #[error("theme_context.worker_ended")]
    Worker,
    /// Exact scheduler/private-file failure, no auto retry.
    #[error(transparent)]
    Io(#[from] ThemeIoError),
}
type Result<T> = std::result::Result<T, ThemeContextError>;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "theme_context_tests.rs"]
pub(crate) mod tests;
struct State {
    source: ThemeIoHandle,
    epoch: u64,
    next: u64,
    pending: Option<u64>,
    closed: bool,
    published: Option<PublishedTheme>,
    #[cfg(test)]
    slot_observer: Option<oneshot::Sender<()>>,
}
impl State {
    fn check_open(&self) -> Result<()> {
        if self.closed {
            return Err(ThemeContextError::Closed);
        }
        if !self.source.is_open() {
            return Err(ThemeIoError::Closed.into());
        }
        Ok(())
    }
    fn advance(&mut self) -> Result<()> {
        if let Some(p) = &self.published {
            p.revoke();
        }
        let Some(next) = self.epoch.checked_add(1) else {
            self.closed = true;
            self.published = None;
            return Err(ThemeContextError::Exhausted);
        };
        self.epoch = next;
        Ok(())
    }
}
/// Non-Clone context owner; not a window capability. Dropping it revokes publication, not shared IO.
/// ```compile_fail
/// fn duplicate(o: pulse_app::theme_context::ThemeContextOwner) { let _ = o.clone(); }
/// ```
pub struct ThemeContextOwner {
    state: Arc<Mutex<State>>,
}
impl ThemeContextOwner {
    /// Bind an explicitly chosen, still-open backend source. No discovery or file creation.
    pub fn new(source: ThemeIoHandle) -> Result<Self> {
        if !source.is_open() {
            return Err(ThemeIoError::Closed.into());
        }
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                source,
                epoch: 1,
                next: 0,
                pending: None,
                closed: false,
                published: None,
                #[cfg(test)]
                slot_observer: None,
            })),
        })
    }
    /// Weak access cannot keep the context alive.
    pub fn handle(&self) -> ThemeContextHandle {
        ThemeContextHandle {
            state: Arc::downgrade(&self.state),
        }
    }
    /// Same exact source is a no-op; different owner identity revokes old epochs even for same UUID.
    /// Old pending slots are retained until actual IO exits/candidate disposal. Different selection
    /// clears prior-source display values; explicit invalidation retains last-good as stale.
    pub fn select(&self, source: ThemeIoHandle) -> Result<()> {
        if !source.is_open() {
            return Err(ThemeIoError::Closed.into());
        }
        let mut s = self.state.lock().map_err(|_| ThemeContextError::State)?;
        if s.closed {
            return Err(ThemeContextError::Closed);
        }
        if s.source.same_source(&source) {
            return Ok(());
        }
        s.advance()?;
        s.source = source;
        s.published = None;
        Ok(())
    }
    /// Trusted signal only; no watcher installed and no file freshness guaranteed between signals.
    pub fn invalidate(&self) -> Result<()> {
        let mut s = self.state.lock().map_err(|_| ThemeContextError::State)?;
        s.check_open()?;
        s.advance()
    }
    /// Monotonic revocation. Shared IO/file and unrelated contexts remain alive.
    pub fn close(&self) {
        // Recover poisoning ONLY to revoke; no authorization path recovers a poisoned state.
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(p) = &s.published {
            p.revoke();
        }
        s.closed = true;
        s.published = None;
    }
}
impl Drop for ThemeContextOwner {
    fn drop(&mut self) {
        self.close();
    }
}
/// Cloneable weak handle; not a page/GUI authorization token.
#[derive(Clone)]
pub struct ThemeContextHandle {
    state: Weak<Mutex<State>>,
}
struct Ticket {
    owner: Weak<Mutex<State>>,
    epoch: u64,
    token: u64,
    source: ThemeIoHandle,
}
impl Ticket {
    fn check_locked(&self, s: &State) -> Result<()> {
        s.check_open()?;
        if s.epoch != self.epoch
            || s.pending != Some(self.token)
            || !s.source.same_source(&self.source)
        {
            return Err(ThemeContextError::Stale);
        }
        Ok(())
    }
    fn check(&self) -> Result<()> {
        let state = self.owner.upgrade().ok_or(ThemeContextError::Closed)?;
        let s = state.lock().map_err(|_| ThemeContextError::State)?;
        self.check_locked(&s)
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        if let Some(state) = self.owner.upgrade()
            && let Ok(mut s) = state.lock()
            && s.pending == Some(self.token)
        {
            s.pending = None;
            #[cfg(test)]
            if let Some(observer) = s.slot_observer.take() {
                let _sent = observer.send(());
            }
        }
    }
}
/// Opaque candidate; private source/epoch/token cannot be forged or rewritten. Drop releases its slot.
pub struct ThemeCandidate {
    ticket: Arc<Ticket>,
    document: ThemeDocument,
}
/// Joined read is still not published until current-context confirmation.
pub struct ThemeReadRequest {
    receiver: oneshot::Receiver<Result<ThemeCandidate>>,
    supervisor: JoinHandle<()>,
}
/// Real supervisor/IO exit observation, no abort/early release operation.
pub struct CancelledThemeRead {
    supervisor: JoinHandle<()>,
}
impl CancelledThemeRead {
    /// Dropping observation never aborts disk reads or releases their quota early.
    pub async fn join(self) -> Result<()> {
        self.supervisor.await.map_err(|_| ThemeContextError::Worker)
    }
}
impl ThemeReadRequest {
    /// Cancels delivery if dropped/timed out. Supervisor retains the ticket through actual IO exit.
    pub async fn ready(self) -> Result<ThemeCandidate> {
        let receipt = self.receiver.await;
        self.supervisor
            .await
            .map_err(|_| ThemeContextError::Worker)?;
        receipt.map_err(|_| ThemeContextError::Worker)?
    }
    /// Suppress publication, not an instruction to abort shared disk IO.
    pub fn cancel(self) -> CancelledThemeRead {
        drop(self.receiver);
        CancelledThemeRead {
            supervisor: self.supervisor,
        }
    }
}
impl ThemeContextHandle {
    /// Reserve the context slot and global IO admission before enqueueing a bounded backend read.
    /// No lock is held across await or disk access. Busy/runtime failures do not consume the slot.
    pub fn begin_load(&self) -> Result<ThemeReadRequest> {
        let state = self.state.upgrade().ok_or(ThemeContextError::Closed)?;
        let mut s = state.lock().map_err(|_| ThemeContextError::State)?;
        s.check_open()?;
        if s.pending.is_some() {
            return Err(ThemeContextError::Busy);
        }
        let Some(token) = s.next.checked_add(1) else {
            if let Some(p) = &s.published {
                p.revoke();
            }
            s.closed = true;
            s.published = None;
            return Err(ThemeContextError::Exhausted);
        };
        let rt = Handle::try_current().map_err(|_| ThemeIoError::Runtime)?;
        s.pending = Some(token);
        let ticket = Arc::new(Ticket {
            owner: self.state.clone(),
            epoch: s.epoch,
            token,
            source: s.source.clone(),
        });
        let source = s.source.clone();
        drop(s);
        // Never drop the final ticket while holding its mutex, including failed IO admission.
        let io = source.begin_load_retaining(ticket.clone())?;
        state.lock().map_err(|_| ThemeContextError::State)?.next = token;
        let (sender, receiver) = oneshot::channel();
        let supervisor = rt.spawn(async move {
            // Even cancelled delivery retains its ticket until IO exits; never abort spawn_blocking.
            if sender.is_closed() {
                let _joined = io.cancel().join().await;
                return;
            }
            if let Err(error) = ticket.check() {
                let result = io
                    .cancel()
                    .join()
                    .await
                    .map_err(ThemeContextError::Io)
                    .and(Err(error));
                let _undelivered = sender.send(result);
                return;
            }
            let result = async {
                let image = io.ready().await?;
                ticket.check()?;
                let document = image
                    .document()
                    .map_err(ThemeIoError::File)?
                    .ok_or(ThemeContextError::Missing)?;
                Ok(ThemeCandidate { ticket, document })
            }
            .await;
            let _undelivered = sender.send(result);
        });
        Ok(ThemeReadRequest {
            receiver,
            supervisor,
        })
    }
    /// Publish only this exact owner's current candidate. Foreign confirmation consumes/discards it.
    pub fn confirm(&self, candidate: ThemeCandidate) -> Result<PublishedTheme> {
        if !Weak::ptr_eq(&self.state, &candidate.ticket.owner) {
            return Err(ThemeContextError::Foreign);
        }
        let state = self.state.upgrade().ok_or(ThemeContextError::Closed)?;
        let mut s = state.lock().map_err(|_| ThemeContextError::State)?;
        candidate.ticket.check_locked(&s)?;
        let p = PublishedTheme {
            owner: self.state.clone(),
            epoch: s.epoch,
            token: candidate.ticket.token,
            source: s.source.clone(),
            document: candidate.document,
            revoked: watch::channel(false).0,
        };
        s.pending = None;
        if let Some(old) = &s.published {
            old.revoke();
        }
        s.published = Some(p.clone());
        drop(s);
        Ok(p)
    }
    /// Same-source last-good can be stale after invalidate/read failure. Not a GUI apply permission.
    pub fn last_good(&self) -> Result<Option<PublishedTheme>> {
        let state = self.state.upgrade().ok_or(ThemeContextError::Closed)?;
        let s = state.lock().map_err(|_| ThemeContextError::State)?;
        if s.closed {
            return Err(ThemeContextError::Closed);
        }
        Ok(s.published.clone())
    }
    /// Instantaneous validity, not atomic authorization for external GUI work or actual file freshness.
    pub fn is_current(&self, p: &PublishedTheme) -> bool {
        if !Weak::ptr_eq(&self.state, &p.owner) {
            return false;
        }
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let Ok(s) = state.lock() else {
            return false;
        };
        s.check_open().is_ok()
            && s.epoch == p.epoch
            && s.source.same_source(&p.source)
            && s.published.as_ref().is_some_and(|v| v.token == p.token)
    }
}
/// Immutable confirmed model, not a current-file guarantee or window permission.
/// ```compile_fail
/// fn forge(p: &mut pulse_app::theme_context::PublishedTheme) { p.epoch = 0; }
/// ```
#[derive(Clone)]
pub struct PublishedTheme {
    owner: Weak<Mutex<State>>,
    epoch: u64,
    token: u64,
    source: ThemeIoHandle,
    document: ThemeDocument,
    revoked: watch::Sender<bool>,
}
/// One-way publication/source invalidation notification, not permission or file freshness.
/// Only bool watches are retained: waiting cannot keep the context/IO owner or quota alive.
pub struct ThemeRevocation {
    context: watch::Receiver<bool>,
    source: pulse_store::theme_io::ThemeIoClosing,
}
impl ThemeRevocation {
    /// Wait for context/publication retirement or IO owner closure; dropping only stops observation.
    pub async fn wait(mut self) {
        let context = async {
            loop {
                let revoked = *self.context.borrow_and_update();
                if revoked || self.context.changed().await.is_err() {
                    return;
                }
            }
        };
        tokio::select! { _ = context => {}, _ = self.source.wait() => {} }
    }
}
impl PublishedTheme {
    fn revoke(&self) {
        self.revoked.send_if_modified(|value| {
            if *value {
                false
            } else {
                *value = true;
                true
            }
        });
    }
    /// Notification is observational only: it never authorizes GUI apply or new IO.
    pub fn observe_revocation(&self) -> ThemeRevocation {
        ThemeRevocation {
            context: self.revoked.subscribe(),
            source: self.source.observe_close(),
        }
    }
    /// Complete validated captured theme, not CSS or an applied stylesheet.
    pub fn document(&self) -> &ThemeDocument {
        &self.document
    }
}
