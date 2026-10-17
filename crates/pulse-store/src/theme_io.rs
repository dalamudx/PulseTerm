//! Backend theme IO. Owner revocation rejects queued/late results, never rolls back started saves.
//! Lost/uncertain receipts require deliberate reread, never automatic resubmission. No GUI authority.
use crate::{
    theme::ThemeDocument,
    theme_file::{ThemeFile, ThemeFileError, ThemeSnapshot},
};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use tokio::{
    runtime::Handle,
    sync::{oneshot, watch},
    task::JoinHandle,
};
static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Quota;
impl Quota {
    fn take() -> Result<Self, ThemeIoError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ThemeIoError::Busy)?;
        Ok(Self)
    }
}
impl Drop for Quota {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
/// Content-free scheduler failures, distinct from exact private-file failures.
#[derive(Debug, thiserror::Error)]
pub enum ThemeIoError {
    /// No runtime; nothing enqueued.
    #[error("theme.io.runtime_unavailable")]
    Runtime,
    /// Another queued/running job owns the single process-wide quota, even after cancellation.
    #[error("theme.io.busy")]
    Busy,
    /// Owner closed/dropped. Started writes may nevertheless have committed.
    #[error("theme.io.closed")]
    Closed,
    /// Worker ended without a usable receipt; never auto repeat a write.
    #[error("theme.io.worker_ended")]
    Worker,
    /// Static file failure without path/text/OS error disclosure.
    #[error(transparent)]
    File(#[from] ThemeFileError),
}
struct Shared {
    file: ThemeFile,
    closed: AtomicBool,
    closing: watch::Sender<bool>,
}
/// Non-Clone owner of an already-opened trusted fixed file. Construct ThemeFile off GUI threads.
/// This owns no window/session permission; close never aborts IO or prematurely releases admission.
pub struct ThemeIoOwner {
    shared: Arc<Shared>,
}
/// Weak access cannot keep the owner alive or reopen a revoked capability.
#[derive(Clone)]
pub struct ThemeIoHandle {
    shared: Weak<Shared>,
}
impl ThemeIoOwner {
    /// Bind an already opened backend file. Does not discover/open any caller-selected path.
    pub fn new(file: ThemeFile) -> Self {
        Self {
            shared: Arc::new(Shared {
                file,
                closed: AtomicBool::new(false),
                closing: watch::channel(false).0,
            }),
        }
    }
    /// Weak backend handle, not an IPC authorization token.
    pub fn handle(&self) -> ThemeIoHandle {
        ThemeIoHandle {
            shared: Arc::downgrade(&self.shared),
        }
    }
    /// Monotonic revocation; started writes may commit, so cancellation is not rollback evidence.
    pub fn close(&self) {
        if !self.shared.closed.swap(true, Ordering::AcqRel) {
            self.shared.closing.send_replace(true);
        }
    }
}
impl Drop for ThemeIoOwner {
    fn drop(&mut self) {
        self.close();
    }
}
/// Opaque result. Dropping it/its consuming wait cancels delivery, never running IO.
pub struct ThemeIoRequest {
    receiver: oneshot::Receiver<Result<ThemeSnapshot, ThemeIoError>>,
    task: JoinHandle<()>,
    shared: Weak<Shared>,
}
/// Observation only; dropping it does not release the closure's quota.
pub struct CancelledThemeIo {
    task: JoinHandle<()>,
}
impl CancelledThemeIo {
    /// Join actual closure exit; panics are not successful write receipts.
    pub async fn join(self) -> Result<(), ThemeIoError> {
        self.task.await.map_err(|_| ThemeIoError::Worker)
    }
}
impl ThemeIoRequest {
    /// Join computation before accepting a result from a still-open owner. This is a captured image,
    /// not authorization for later GUI publication and not proof of current file contents.
    pub async fn ready(self) -> Result<ThemeSnapshot, ThemeIoError> {
        let result = self.receiver.await;
        self.task.await.map_err(|_| ThemeIoError::Worker)?;
        let state = self.shared.upgrade().ok_or(ThemeIoError::Closed)?;
        if state.closed.load(Ordering::Acquire) {
            return Err(ThemeIoError::Closed);
        }
        result.map_err(|_| ThemeIoError::Worker)?
    }
    /// Suppress delivery. Never abort spawn_blocking or release quota early.
    pub fn cancel(self) -> CancelledThemeIo {
        drop(self.receiver);
        CancelledThemeIo { task: self.task }
    }
}
/// One-way content-free owner-close observation. Not an IO/cancellation receipt.
/// It retains neither the ThemeIoOwner nor a running operation's reservation.
pub struct ThemeIoClosing {
    receiver: watch::Receiver<bool>,
}
impl ThemeIoClosing {
    /// Completes also if the notifier disappears: observation always fails closed.
    pub async fn wait(mut self) {
        loop {
            let closed = *self.receiver.borrow_and_update();
            if closed || self.receiver.changed().await.is_err() {
                return;
            }
        }
    }
}
impl ThemeIoHandle {
    /// Observe owner closure without retaining its file, IO job, or admission quota.
    pub fn observe_close(&self) -> ThemeIoClosing {
        let receiver = self
            .shared
            .upgrade()
            .map_or_else(|| watch::channel(true).1, |s| s.closing.subscribe());
        ThemeIoClosing { receiver }
    }
    /// Point-in-time lifecycle check, not authorization or file freshness.
    pub fn is_open(&self) -> bool {
        self.shared
            .upgrade()
            .is_some_and(|s| !s.closed.load(Ordering::Acquire))
    }
    /// Exact bound owner identity, not merely a matching UUID or directory. Not a permission token.
    pub fn same_source(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.shared, &other.shared)
    }

    /// One off-thread read under the globally bounded quota. Invalid files never become defaults.
    pub fn begin_load(&self) -> Result<ThemeIoRequest, ThemeIoError> {
        dispatch(self, |s| Ok(s.file.load()?))
    }
    /// Retain an opaque trusted-backend lifetime guard in the actual blocking closure, even when
    /// its async supervisor/runtime exits first. Guard Drop must be short and nonpanicking.
    /// Failed admission drops it immediately; no job or UI permission is created by the guard.
    pub fn begin_load_retaining<G: Send + 'static>(
        &self,
        lifetime: G,
    ) -> Result<ThemeIoRequest, ThemeIoError> {
        dispatch_retaining(self, lifetime, |s| Ok(s.file.load()?))
    }
    /// Own expected CAS image/document through closure exit. Lost receipt is never auto-retried.
    pub fn begin_save(
        &self,
        expected: ThemeSnapshot,
        document: ThemeDocument,
    ) -> Result<ThemeIoRequest, ThemeIoError> {
        dispatch(self, move |s| Ok(s.file.save(&expected, &document)?))
    }
}
fn dispatch<F>(h: &ThemeIoHandle, operation: F) -> Result<ThemeIoRequest, ThemeIoError>
where
    F: FnOnce(&Shared) -> Result<ThemeSnapshot, ThemeIoError> + Send + 'static,
{
    dispatch_retaining(h, (), operation)
}
fn dispatch_retaining<F, G>(
    h: &ThemeIoHandle,
    lifetime: G,
    operation: F,
) -> Result<ThemeIoRequest, ThemeIoError>
where
    F: FnOnce(&Shared) -> Result<ThemeSnapshot, ThemeIoError> + Send + 'static,
    G: Send + 'static,
{
    let state = h.shared.upgrade().ok_or(ThemeIoError::Closed)?;
    if state.closed.load(Ordering::Acquire) {
        return Err(ThemeIoError::Closed);
    }
    let rt = Handle::try_current().map_err(|_| ThemeIoError::Runtime)?;
    let quota = Quota::take()?;
    let (sender, receiver) = oneshot::channel();
    let task = rt.spawn_blocking(move || {
        let _lifetime = lifetime;
        let _quota = quota;
        if sender.is_closed() || state.closed.load(Ordering::Acquire) {
            return;
        }
        // After admission, effects may become irreversible. Cancellation only suppresses the receipt.
        let result = operation(&state);
        if !state.closed.load(Ordering::Acquire) {
            let _undelivered = sender.send(result);
        }
    });
    Ok(ThemeIoRequest {
        receiver,
        task,
        shared: h.shared.clone(),
    })
}
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "theme_io_tests.rs"]
mod tests;
