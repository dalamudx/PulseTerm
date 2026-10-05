//! One process-wide I/O owner, one dirty flag, no watcher or caller-selected file path.
use crate::{
    settings::SettingsPatch,
    settings_file::SettingsFile,
    settings_reload::{SettingsReloadCache, SettingsReloadError},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{runtime::Handle, sync::watch, task::JoinHandle};
static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Quota;
impl Quota {
    fn take() -> Result<Self, SettingsSourceError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| SettingsSourceError::Busy)?;
        Ok(Self)
    }
}
impl Drop for Quota {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
/// Stable status contains no path or document. Rejection never replaces last-good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSourceStatus {
    /// Verified model, not a permanent file lease.
    Ready,
    /// Coalesced signal/read pending.
    Pending,
    /// Last read rejected; another explicit signal is required.
    Rejected(SettingsReloadError),
    /// Explicit owner revocation.
    Closed,
    /// Worker/state failed closed.
    Failed,
}
/// Static failures do not include user document content.
#[derive(Debug, thiserror::Error)]
pub enum SettingsSourceError {
    /// Only one source initialization/worker may be alive in this process.
    #[error("settings.source.busy")]
    Busy,
    /// No runtime; no file opened or work queued.
    #[error("settings.source.runtime")]
    Runtime,
    /// Owner closed; weak handles cannot resurrect it.
    #[error("settings.source.closed")]
    Closed,
    /// Worker ended unexpectedly; no retry.
    #[error("settings.source.worker")]
    Worker,
    /// Internal synchronization failed closed.
    #[error("settings.source.state")]
    State,
    /// Monotonic counter exhausted; owner closes rather than wrapping.
    #[error("settings.source.exhausted")]
    Exhausted,
    /// Initial read/validation failed; no fallback source is created.
    #[error(transparent)]
    Reload(#[from] SettingsReloadError),
}
struct State {
    closed: bool,
    dirty: bool,
    reading: bool,
    requested: u64,
    verified: u64,
    revision: u64,
    patch: SettingsPatch,
    status: SettingsSourceStatus,
    #[cfg(test)]
    pause: Option<Pause>,
    #[cfg(test)]
    reads: u64,
}
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    status: watch::Sender<SettingsSourceStatus>,
}
/// Parsed non-secret sparse values only; no file text, path or permission token.
#[derive(Clone)]
pub struct UserSettingsSnapshot {
    owner: Weak<Shared>,
    revision: u64,
    verified: u64,
    patch: SettingsPatch,
}
impl UserSettingsSnapshot {
    /// Validated sparse layer, including explicit defaults/false.
    pub fn patch(&self) -> &SettingsPatch {
        &self.patch
    }
    /// Semantic model revision; comments do not advance it.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
/// Weak client; dropping the owner closes even if clients/observers remain.
#[derive(Clone)]
pub struct SettingsSourceHandle {
    shared: Weak<Shared>,
}
/// Owns supervisor and file lifetime; Drop revokes publication, never aborts active I/O.
pub struct SettingsSourceTask {
    shared: Arc<Shared>,
    task: Option<JoinHandle<Result<(), SettingsSourceError>>>,
}
impl SettingsSourceHandle {
    /// One dirty flag coalesces signals. Initial/active jobs retain their quota until actual exit.
    pub fn invalidate(&self) -> Result<(), SettingsSourceError> {
        let shared = self.shared.upgrade().ok_or(SettingsSourceError::Closed)?;
        let mut state = shared
            .state
            .lock()
            .map_err(|_| SettingsSourceError::State)?;
        if state.closed {
            return Err(SettingsSourceError::Closed);
        }
        let Some(next) = state.requested.checked_add(1) else {
            state.closed = true;
            state.dirty = false;
            state.status = SettingsSourceStatus::Closed;
            shared.status.send_replace(state.status);
            shared.wake.notify_all();
            return Err(SettingsSourceError::Exhausted);
        };
        state.requested = next;
        state.dirty = true;
        if state.status != SettingsSourceStatus::Pending {
            state.status = SettingsSourceStatus::Pending;
            shared.status.send_replace(state.status);
        }
        shared.wake.notify_one();
        Ok(())
    }
    /// Last accepted layer; status/current must be checked before new application.
    pub fn snapshot(&self) -> Result<UserSettingsSnapshot, SettingsSourceError> {
        let shared = self.shared.upgrade().ok_or(SettingsSourceError::Closed)?;
        let state = shared
            .state
            .lock()
            .map_err(|_| SettingsSourceError::State)?;
        if state.closed {
            return Err(SettingsSourceError::Closed);
        }
        Ok(UserSettingsSnapshot {
            owner: self.shared.clone(),
            revision: state.revision,
            verified: state.verified,
            patch: state.patch.clone(),
        })
    }
    /// Instantaneous source identity/model/verification test, not GUI or host authorization.
    pub fn is_current(&self, snapshot: &UserSettingsSnapshot) -> bool {
        if !self.shared.ptr_eq(&snapshot.owner) {
            return false;
        }
        let Some(shared) = self.shared.upgrade() else {
            return false;
        };
        let Ok(state) = shared.state.lock() else {
            return false;
        };
        !state.closed
            && !state.dirty
            && !state.reading
            && state.status == SettingsSourceStatus::Ready
            && state.requested == state.verified
            && snapshot.verified == state.verified
            && snapshot.revision == state.revision
    }
    /// Watch channel holds only the latest static status, not a queue of file contents.
    pub fn subscribe(&self) -> Result<watch::Receiver<SettingsSourceStatus>, SettingsSourceError> {
        let shared = self.shared.upgrade().ok_or(SettingsSourceError::Closed)?;
        let state = shared
            .state
            .lock()
            .map_err(|_| SettingsSourceError::State)?;
        if state.closed {
            return Err(SettingsSourceError::Closed);
        }
        Ok(shared.status.subscribe())
    }
}
fn close(shared: &Shared, status: SettingsSourceStatus) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.closed && status == SettingsSourceStatus::Closed {
        return;
    }
    state.closed = true;
    state.dirty = false;
    state.status = status;
    shared.status.send_replace(status);
    shared.wake.notify_all();
}
impl SettingsSourceTask {
    /// Revokes synchronously. Active bounded I/O still owns its quota/file.
    pub fn close(&self) {
        close(&self.shared, SettingsSourceStatus::Closed);
    }
    /// Closes and supervises actual worker exit; caller may time out, never auto retry.
    pub async fn shutdown(mut self) -> Result<(), SettingsSourceError> {
        self.close();
        self.task
            .take()
            .ok_or(SettingsSourceError::Worker)?
            .await
            .map_err(|_| SettingsSourceError::Worker)?
    }
}
impl Drop for SettingsSourceTask {
    fn drop(&mut self) {
        self.close();
    }
}
/// Trusted Rust-only private directory admission and initial read run off the async/GUI thread.
/// A failed/cancelled initial read cannot create a fallback or background source.
pub async fn open_user_settings_source(
    directory: PathBuf,
) -> Result<(SettingsSourceHandle, SettingsSourceTask), SettingsSourceError> {
    let runtime = Handle::try_current().map_err(|_| SettingsSourceError::Runtime)?;
    let quota = Quota::take()?;
    let (cache, quota) = runtime
        .spawn_blocking(move || {
            let quota = quota;
            let file = SettingsFile::open_private(directory).map_err(SettingsReloadError::from)?;
            let mut cache = SettingsReloadCache::new(file);
            cache.refresh()?;
            Ok::<_, SettingsSourceError>((cache, quota))
        })
        .await
        .map_err(|_| SettingsSourceError::Worker)??;
    let (status, _) = watch::channel(SettingsSourceStatus::Ready);
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            closed: false,
            dirty: false,
            reading: false,
            requested: 0,
            verified: 0,
            revision: 1,
            patch: cache.user_patch().clone(),
            status: SettingsSourceStatus::Ready,
            #[cfg(test)]
            pause: None,
            #[cfg(test)]
            reads: 1,
        }),
        wake: Condvar::new(),
        status,
    });
    let state = shared.clone();
    let worker = runtime.spawn_blocking(move || {
        let _quota = quota;
        run(cache, &state)
    });
    let supervisor = shared.clone();
    let task = runtime.spawn(async move {
        match worker.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                close(&supervisor, SettingsSourceStatus::Failed);
                Err(error)
            }
            Err(_) => {
                close(&supervisor, SettingsSourceStatus::Failed);
                Err(SettingsSourceError::Worker)
            }
        }
    });
    Ok((
        SettingsSourceHandle {
            shared: Arc::downgrade(&shared),
        },
        SettingsSourceTask {
            shared,
            task: Some(task),
        },
    ))
}
fn run(mut cache: SettingsReloadCache, shared: &Shared) -> Result<(), SettingsSourceError> {
    loop {
        let version;
        #[cfg(test)]
        let pause;
        {
            let mut state = shared
                .state
                .lock()
                .map_err(|_| SettingsSourceError::State)?;
            while !state.closed && !state.dirty {
                state = shared
                    .wake
                    .wait(state)
                    .map_err(|_| SettingsSourceError::State)?;
            }
            if state.closed {
                cache.close();
                return Ok(());
            }
            version = state.requested;
            state.dirty = false;
            state.reading = true;
            #[cfg(test)]
            {
                pause = state.pause.take();
            }
        }
        #[cfg(test)]
        if let Some(gate) = &pause {
            gate.wait_at(PausePhase::BeforeRead)?;
        }
        {
            let state = shared
                .state
                .lock()
                .map_err(|_| SettingsSourceError::State)?;
            if state.closed {
                cache.close();
                return Ok(());
            }
            #[cfg(test)]
            {
                let mut state = state;
                state.reads += 1;
            }
        }
        cache.invalidate()?;
        let result = cache.refresh();
        #[cfg(test)]
        if let Some(gate) = &pause {
            gate.wait_at(PausePhase::AfterRead)?;
        }
        let mut state = shared
            .state
            .lock()
            .map_err(|_| SettingsSourceError::State)?;
        state.reading = false;
        if state.closed {
            cache.close();
            return Ok(());
        }
        if state.requested != version {
            continue;
        }
        match result {
            Ok(_) => {
                let patch = cache.user_patch();
                if patch != &state.patch {
                    let revision = state
                        .revision
                        .checked_add(1)
                        .ok_or(SettingsSourceError::Exhausted)?;
                    state.patch = patch.clone();
                    state.revision = revision;
                }
                state.verified = version;
                state.status = SettingsSourceStatus::Ready;
            }
            Err(error) => {
                state.status = SettingsSourceStatus::Rejected(error);
            }
        }
        shared.status.send_replace(state.status);
    }
}
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "settings_source_tests.rs"]
mod tests;

#[cfg(test)]
#[derive(PartialEq, Eq)]
enum PausePhase {
    BeforeRead,
    AfterRead,
    Crash,
}
#[cfg(test)]
struct Pause {
    phase: PausePhase,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
#[cfg(test)]
impl Pause {
    fn wait_at(&self, phase: PausePhase) -> Result<(), SettingsSourceError> {
        if phase == self.phase
            || (phase == PausePhase::AfterRead && self.phase == PausePhase::Crash)
        {
            self.entered
                .send(())
                .map_err(|_| SettingsSourceError::Worker)?;
            self.release
                .recv_timeout(std::time::Duration::from_secs(3))
                .map_err(|_| SettingsSourceError::Worker)?;
            if self.phase == PausePhase::Crash {
                panic!("controlled settings source worker failure");
            }
        }
        Ok(())
    }
}
