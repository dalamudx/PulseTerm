//! Background keymap IO. Native closures retain quota and guards through actual exit.
use crate::keymap_file::{KeymapFile, KeymapFileError, KeymapMutation, KeymapSnapshot};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use tokio::{runtime::Handle, sync::oneshot, task::JoinHandle};
/// Trusted native file adapter. Implementations retain their own private handles and exact CAS.
/// This is not a serialized interface or an execution/page authorization grant.
pub trait KeymapBackend: Send + Sync + 'static {
    /// Opaque exact source image; no Debug/serde bound permits accidental content logging.
    type Snapshot: Clone + Send + Sync + 'static;
    /// Read and validate the bounded complete document.
    fn load(&self) -> Result<Self::Snapshot, KeymapFileError>;
    /// Confirm the original image and caller authority under the cooperating lock.
    fn confirm_current_authorized(
        &self,
        expected: &Self::Snapshot,
        authorize: impl Fn() -> bool,
    ) -> Result<(), KeymapFileError>;
    /// Apply one typed edit with CAS and commit-boundary authorization checks.
    fn save_authorized(
        &self,
        expected: &Self::Snapshot,
        mutation: KeymapMutation,
        authorize: impl Fn() -> bool,
    ) -> Result<Self::Snapshot, KeymapFileError>;
}
impl KeymapBackend for KeymapFile {
    type Snapshot = KeymapSnapshot;
    fn load(&self) -> Result<KeymapSnapshot, KeymapFileError> {
        self.load()
    }
    fn confirm_current_authorized(
        &self,
        expected: &KeymapSnapshot,
        authorize: impl Fn() -> bool,
    ) -> Result<(), KeymapFileError> {
        self.confirm_current_authorized(expected, authorize)
    }
    fn save_authorized(
        &self,
        expected: &KeymapSnapshot,
        mutation: KeymapMutation,
        authorize: impl Fn() -> bool,
    ) -> Result<KeymapSnapshot, KeymapFileError> {
        self.save_authorized(expected, mutation, authorize)
    }
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Quota;
impl Quota {
    fn take() -> Result<Self, KeymapIoError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| KeymapIoError::Busy)?;
        Ok(Self)
    }
}
impl Drop for Quota {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
/// Content-free lifecycle failures, separate from exact file failures.
#[derive(Debug, thiserror::Error)]
pub enum KeymapIoError {
    /// Another queued/running operation retains the single process-wide IO quota.
    #[error("keymap.io.busy")]
    Busy,
    /// Original owner is closed/dropped; this does not prove started writes rolled back.
    #[error("keymap.io.closed")]
    Closed,
    /// Worker panicked or disappeared without a usable receipt; do not replay a write.
    #[error("keymap.io.worker_ended")]
    Worker,
    /// Bounded fixed-file failure without paths or document contents.
    #[error(transparent)]
    File(#[from] KeymapFileError),
}
struct Shared<B: KeymapBackend> {
    file: B,
    runtime: Handle,
    closed: AtomicBool,
}
/// Non-Clone owner of an already-opened backend file and explicit application runtime.
/// Does not discover paths, grant page permission, or abort active native operations on close.
pub struct KeymapIoOwner<B: KeymapBackend = KeymapFile> {
    shared: Arc<Shared<B>>,
}
/// Weak backend-only handle; cannot retain or resurrect the owner.
pub struct KeymapIoHandle<B: KeymapBackend = KeymapFile> {
    shared: Weak<Shared<B>>,
}
impl<B: KeymapBackend> KeymapIoOwner<B> {
    /// Bind a file opened on supervised blocking work; no implicit Tokio context is required.
    pub fn new(file: B, runtime: Handle) -> Self {
        Self {
            shared: Arc::new(Shared {
                file,
                runtime,
                closed: AtomicBool::new(false),
            }),
        }
    }
    /// Weak backend handle, never an IPC authorization token.
    pub fn handle(&self) -> KeymapIoHandle<B> {
        KeymapIoHandle {
            shared: Arc::downgrade(&self.shared),
        }
    }
    /// Revoke queued/late delivery. Quota and lifetime guards still belong to native closures.
    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
    }
}
impl<B: KeymapBackend> Drop for KeymapIoOwner<B> {
    fn drop(&mut self) {
        self.close();
    }
}
/// Single-use receipt. Dropping a wait suppresses delivery, never releases active native quota.
pub struct KeymapIoRequest<B: KeymapBackend = KeymapFile> {
    receiver: oneshot::Receiver<Result<B::Snapshot, KeymapIoError>>,
    task: JoinHandle<()>,
    shared: Weak<Shared<B>>,
}
/// Join observation only. Dropping it detaches the worker, not its reservation.
pub struct CancelledKeymapIo {
    task: JoinHandle<()>,
}
impl CancelledKeymapIo {
    /// Wait for actual closure exit; success is resource completion, not rollback evidence.
    pub async fn wait_exit(self) -> Result<(), KeymapIoError> {
        self.task.await.map_err(|_| KeymapIoError::Worker)
    }
}
impl<B: KeymapBackend> KeymapIoRequest<B> {
    /// Deliver only after closure exit and original-owner liveness validation.
    pub async fn ready(self) -> Result<B::Snapshot, KeymapIoError> {
        let result = self.receiver.await;
        self.task.await.map_err(|_| KeymapIoError::Worker)?;
        let state = self.shared.upgrade().ok_or(KeymapIoError::Closed)?;
        if state.closed.load(Ordering::Acquire) {
            return Err(KeymapIoError::Closed);
        }
        result.map_err(|_| KeymapIoError::Worker)?
    }
    /// Cancel publication and pre-commit authorization; still observe actual IO exit separately.
    pub fn cancel(self) -> CancelledKeymapIo {
        drop(self.receiver);
        CancelledKeymapIo { task: self.task }
    }
}
impl<B: KeymapBackend> KeymapIoHandle<B> {
    /// Identity only, not a permission or proof of source freshness.
    pub fn same_source(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.shared, &other.shared)
    }
    /// Snapshot lifecycle only, not page or source authorization.
    pub fn is_open(&self) -> bool {
        self.shared
            .upgrade()
            .is_some_and(|state| !state.closed.load(Ordering::Acquire))
    }
    /// Load using an explicit runtime; invalid existing files do not become defaults.
    pub fn begin_load(&self) -> Result<KeymapIoRequest<B>, KeymapIoError> {
        self.begin_load_retaining(())
    }
    /// Keep a trusted caller's resource guard in the blocking closure until actual exit.
    pub fn begin_load_retaining<G: Send + 'static>(
        &self,
        guard: G,
    ) -> Result<KeymapIoRequest<B>, KeymapIoError> {
        dispatch(self, guard, |state, _| Ok(state.file.load()?))
    }
    /// Jointly check a captured file image and trusted context inside the native file lock.
    /// Returns the same captured snapshot after actual exit; never a page or Capability grant.
    pub fn begin_confirm_retaining<G, F>(
        &self,
        expected: B::Snapshot,
        authorize: F,
        guard: G,
    ) -> Result<KeymapIoRequest<B>, KeymapIoError>
    where
        G: Send + 'static,
        F: Fn() -> bool + Send + 'static,
    {
        dispatch(self, guard, move |state, sender| {
            state.file.confirm_current_authorized(&expected, || {
                !state.closed.load(Ordering::Acquire) && !sender.is_closed() && authorize()
            })?;
            Ok(expected)
        })
    }

    /// Own the expected snapshot/edit/authorization and caller guard through native exit.
    /// Authorization is backend-only and rechecked before install; a committed write may lose its receipt.
    pub fn begin_save_retaining<G, F>(
        &self,
        expected: B::Snapshot,
        mutation: KeymapMutation,
        authorize: F,
        guard: G,
    ) -> Result<KeymapIoRequest<B>, KeymapIoError>
    where
        G: Send + 'static,
        F: Fn() -> bool + Send + 'static,
    {
        dispatch(self, guard, move |state, sender| {
            Ok(state.file.save_authorized(&expected, mutation, || {
                !state.closed.load(Ordering::Acquire) && !sender.is_closed() && authorize()
            })?)
        })
    }
}
type Sender<B> = oneshot::Sender<Result<<B as KeymapBackend>::Snapshot, KeymapIoError>>;
fn dispatch<G, F, B: KeymapBackend>(
    handle: &KeymapIoHandle<B>,
    guard: G,
    operation: F,
) -> Result<KeymapIoRequest<B>, KeymapIoError>
where
    G: Send + 'static,
    F: FnOnce(&Shared<B>, &Sender<B>) -> Result<B::Snapshot, KeymapIoError> + Send + 'static,
{
    let state = handle.shared.upgrade().ok_or(KeymapIoError::Closed)?;
    if state.closed.load(Ordering::Acquire) {
        return Err(KeymapIoError::Closed);
    }
    let quota = Quota::take()?;
    let (sender, receiver) = oneshot::channel();
    let task = state.runtime.clone().spawn_blocking(move || {
        let _guard = guard;
        let _quota = quota;
        if sender.is_closed() || state.closed.load(Ordering::Acquire) {
            return;
        }
        let result = operation(&state, &sender);
        if !state.closed.load(Ordering::Acquire) {
            let _undelivered = sender.send(result);
        }
    });
    Ok(KeymapIoRequest {
        receiver,
        task,
        shared: handle.shared.clone(),
    })
}
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "keymap_io_tests.rs"]
mod tests;

impl<B: KeymapBackend> Clone for KeymapIoHandle<B> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}
