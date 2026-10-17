//! Current-page keymap tickets and root-owned IO. No caller paths or execution authority.
use super::*;
use pulse_app::registry::PageLaunchScope;
use pulse_store::{
    keymap::{KeymapAction, KeymapOverride, KeymapProjection},
    keymap_file::{KeymapFile, KeymapFileError, KeymapMutation, KeymapSnapshot},
    keymap_io::{KeymapIoError, KeymapIoOwner},
};
use std::{
    future::Future,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::{
    runtime::Handle,
    sync::{Semaphore, oneshot, watch},
    task::JoinHandle,
};
#[derive(serde::Serialize)]
pub(super) struct Projection {
    pub(super) version: u16,
    pub(super) token: Uuid,
    pub(super) confirmed: bool,
    pub(super) values: KeymapProjection,
}
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    Set {
        action: KeymapAction,
        keys: Vec<String>,
        when: String,
    },
    Restore {
        action: KeymapAction,
    },
}
impl Change {
    fn mutation(self) -> KeymapMutation {
        match self {
            Self::Set { action, keys, when } => {
                KeymapMutation::Set(KeymapOverride { action, keys, when })
            }
            Self::Restore { action } => KeymapMutation::Restore(action),
        }
    }
}
struct Source {
    directory: PathBuf,
    io: Arc<KeymapIoOwner>,
}
struct Job {
    task: JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
    page: Arc<PageLaunchScope>,
}
pub(super) struct KeymapOwner {
    registry: SessionRegistry,
    runtime: Handle,
    source: Mutex<Option<Source>>,
    pending: metadata_captures::MetadataCaptures<KeymapSnapshot>,
    confirmed: metadata_captures::MetadataCaptures<KeymapSnapshot>,
    closed: AtomicBool,
    failed: AtomicBool,
    credit: Arc<Semaphore>,
    jobs: Mutex<Vec<Job>>,
    draining: AtomicBool,
    drained: watch::Sender<Option<Result<()>>>,
}
struct WaitGuard {
    cancelled: Arc<AtomicBool>,
    armed: bool,
}
impl Drop for WaitGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cancelled.store(true, Ordering::Release);
        }
    }
}
struct JobGuard {
    owner: Arc<KeymapOwner>,
    finished: bool,
}
impl Drop for JobGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.owner.failed.store(true, Ordering::Release);
            self.owner.close();
        }
    }
}
impl KeymapOwner {
    pub(super) fn new(registry: SessionRegistry, runtime: Handle) -> Arc<Self> {
        Arc::new(Self {
            registry,
            runtime,
            source: Mutex::new(None),
            pending: metadata_captures::MetadataCaptures::new("workspace.keymap_failed"),
            confirmed: metadata_captures::MetadataCaptures::new("workspace.keymap_failed"),
            closed: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            credit: Arc::new(Semaphore::new(1)),
            jobs: Mutex::new(Vec::new()),
            draining: AtomicBool::new(false),
            drained: watch::channel(None).0,
        })
    }
    fn check(&self, page: &PageLaunchScope, cancelled: &AtomicBool) -> Result<()> {
        if self.closed.load(Ordering::Acquire) || cancelled.load(Ordering::Acquire) {
            return Err("workspace.denied");
        }
        self.registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")
    }
    // The supervisor, not the RPC waiter, owns every native join. No job is aborted on retirement.
    async fn run<F, Fut>(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        work: F,
    ) -> Result<Projection>
    where
        F: FnOnce(Arc<Self>, Arc<AtomicBool>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Projection>> + Send + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        self.check(&page, &cancelled)?;
        let mut guard = WaitGuard {
            cancelled: cancelled.clone(),
            armed: true,
        };
        let permit = self
            .credit
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        let (sender, receiver) = oneshot::channel();
        {
            let mut jobs = self.jobs.lock().map_err(|_| "workspace.keymap_failed")?;
            self.check(&page, &cancelled)?;
            jobs.retain(|job| !job.task.is_finished());
            // At most one active job plus a completed native operation's finishing supervisor.
            if jobs.len() >= 2 {
                return Err("workspace.busy");
            }
            let owner = self.clone();
            let cancel = cancelled.clone();
            let finalizer = JobGuard {
                owner: owner.clone(),
                finished: false,
            };
            let task = self.runtime.spawn(async move {
                let _permit = permit;
                let mut finalizer = finalizer;
                let result = work(owner.clone(), cancel.clone()).await;
                let undelivered = cancel.load(Ordering::Acquire)
                    || sender.is_closed()
                    || sender.send(result).is_err();
                if undelivered {
                    owner.pending.retire();
                    owner.confirmed.retire();
                }
                finalizer.finished = true;
            });
            jobs.push(Job {
                task,
                cancelled: cancelled.clone(),
                page: page.clone(),
            });
        }
        let result = receiver.await.map_err(|_| {
            if cancelled.load(Ordering::Acquire) {
                "workspace.denied"
            } else {
                "workspace.keymap_failed"
            }
        })?;
        guard.armed = false;
        self.check(&page, &cancelled)?;
        result
    }
    async fn source(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Arc<KeymapIoOwner>> {
        self.check(&page, &cancelled)?;
        {
            let source = self.source.lock().map_err(|_| "workspace.keymap_failed")?;
            if let Some(source) = source.as_ref() {
                if source.directory != directory {
                    return Err("workspace.denied");
                }
                return Ok(source.io.clone());
            }
        }
        let owner = self.clone();
        let captured_directory = directory.clone();
        let runtime = self.runtime.clone();
        let io = self
            .runtime
            .spawn_blocking(move || {
                owner.check(&page, &cancelled)?;
                let admitted = pulse_store::state_directory::prepare(&captured_directory)
                    .map_err(|_| "workspace.keymap_unavailable")?;
                owner.check(&page, &cancelled)?;
                let file = KeymapFile::open_private(admitted)
                    .map_err(|_| "workspace.keymap_unavailable")?;
                owner.check(&page, &cancelled)?;
                Ok::<_, &'static str>(Arc::new(KeymapIoOwner::new(file, runtime)))
            })
            .await
            .map_err(|_| "workspace.keymap_failed")??;
        if self.closed.load(Ordering::Acquire) {
            io.close();
            return Err("workspace.denied");
        }
        let mut source = self.source.lock().map_err(|_| "workspace.keymap_failed")?;
        if self.closed.load(Ordering::Acquire) {
            io.close();
            return Err("workspace.denied");
        }
        *source = Some(Source {
            directory,
            io: io.clone(),
        });
        Ok(io)
    }
    fn publish(
        &self,
        page: Arc<PageLaunchScope>,
        snapshot: KeymapSnapshot,
        confirmed: bool,
        cancelled: &AtomicBool,
    ) -> Result<Projection> {
        self.check(&page, cancelled)?;
        let values = snapshot
            .projection()
            .map_err(|_| "workspace.keymap_invalid")?;
        let captures = if confirmed {
            &self.confirmed
        } else {
            &self.pending
        };
        let token = captures.publish(&self.registry, page, snapshot)?;
        Ok(Projection {
            version: 1,
            token,
            confirmed,
            values,
        })
    }
    pub(super) async fn read(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
    ) -> Result<Projection> {
        let scope = page.clone();
        self.run(page, move |owner, cancelled| async move {
            owner.check(&scope, &cancelled)?;
            owner.pending.retire();
            owner.confirmed.retire();
            let io = owner
                .source(scope.clone(), directory, cancelled.clone())
                .await?;
            let snapshot = io
                .handle()
                .begin_load_retaining(scope.clone())
                .map_err(io_error)?
                .ready()
                .await
                .map_err(io_error)?;
            owner.publish(scope, snapshot, false, &cancelled)
        })
        .await
    }
    pub(super) async fn confirm(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        token: Uuid,
    ) -> Result<Projection> {
        let scope = page.clone();
        self.run(page, move |owner, cancelled| async move {
            owner.check(&scope, &cancelled)?;
            let expected = owner.pending.take(&owner.registry, &scope, token)?;
            let io = owner
                .source
                .lock()
                .map_err(|_| "workspace.keymap_failed")?
                .as_ref()
                .ok_or("workspace.denied")?
                .io
                .clone();
            let checking = owner.clone();
            let original = scope.clone();
            let cancellation = cancelled.clone();
            let snapshot = io
                .handle()
                .begin_confirm_retaining(
                    expected,
                    move || checking.check(&original, &cancellation).is_ok(),
                    scope.clone(),
                )
                .map_err(io_error)?
                .ready()
                .await
                .map_err(io_error)?;
            owner.publish(scope, snapshot, true, &cancelled)
        })
        .await
    }
    pub(super) async fn save(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        token: Uuid,
        change: Change,
        consent: bool,
    ) -> Result<Projection> {
        if !consent {
            return Err("workspace.keymap_rewrite_confirmation_required");
        }
        let scope = page.clone();
        self.run(page, move |owner, cancelled| async move {
            owner.check(&scope, &cancelled)?;
            let expected = owner.confirmed.take(&owner.registry, &scope, token)?;
            let io = owner
                .source
                .lock()
                .map_err(|_| "workspace.keymap_failed")?
                .as_ref()
                .ok_or("workspace.denied")?
                .io
                .clone();
            let checking = owner.clone();
            let original = scope.clone();
            let cancellation = cancelled.clone();
            let snapshot = io
                .handle()
                .begin_save_retaining(
                    expected,
                    change.mutation(),
                    move || checking.check(&original, &cancellation).is_ok(),
                    scope.clone(),
                )
                .map_err(io_error)?
                .ready()
                .await
                .map_err(io_error)?;
            owner.publish(scope, snapshot, false, &cancelled)
        })
        .await
    }
    pub(super) fn cancel_page(&self, page: &Arc<PageLaunchScope>) -> Result<()> {
        self.check(page, &AtomicBool::new(false))?;
        let jobs = self.jobs.lock().map_err(|_| "workspace.keymap_failed")?;
        for job in jobs.iter() {
            if Arc::ptr_eq(&job.page, page) {
                job.cancelled.store(true, Ordering::Release);
            }
        }
        self.pending.retire();
        self.confirmed.retire();
        Ok(())
    }

    pub(super) fn retire(&self) {
        self.pending.retire();
        self.confirmed.retire();
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.iter() {
                job.cancelled.store(true, Ordering::Release);
            }
        }
    }
    pub(super) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.credit.close();
        self.retire();
        if let Ok(source) = self.source.lock()
            && let Some(source) = source.as_ref()
        {
            source.io.close();
        }
    }
    // Shared drain outlives each observer; cancellation cannot lose the only native join.
    pub(super) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        self.close();
        if !self.draining.swap(true, Ordering::AcqRel) {
            let owner = self.clone();
            self.runtime.spawn(async move {
                let jobs = match owner.jobs.lock() {
                    Ok(mut jobs) => std::mem::take(&mut *jobs),
                    Err(_) => {
                        owner
                            .drained
                            .send_replace(Some(Err("workspace.keymap_failed")));
                        return;
                    }
                };
                let mut result = Ok(());
                for job in jobs {
                    if job.task.await.is_err() {
                        result = Err("workspace.keymap_failed");
                    }
                }
                if owner.failed.load(Ordering::Acquire) {
                    result = Err("workspace.keymap_failed");
                }
                if let Ok(mut source) = owner.source.lock() {
                    source.take();
                } else {
                    result = Err("workspace.keymap_failed");
                }
                owner.drained.send_replace(Some(result));
            });
        }
        let mut finished = self.drained.subscribe();
        loop {
            let result = *finished.borrow_and_update();
            if let Some(result) = result {
                return result;
            }
            finished
                .changed()
                .await
                .map_err(|_| "workspace.keymap_failed")?;
        }
    }
}
fn io_error(error: KeymapIoError) -> &'static str {
    match error {
        KeymapIoError::Busy => "workspace.busy",
        KeymapIoError::Closed | KeymapIoError::File(KeymapFileError::Authorization) => {
            "workspace.denied"
        }
        KeymapIoError::File(KeymapFileError::Conflict) => "workspace.keymap_stale",
        KeymapIoError::File(KeymapFileError::Document(_)) => "workspace.keymap_invalid",
        _ => "workspace.keymap_unconfirmed",
    }
}
fn token(value: &str) -> Result<Uuid> {
    let token = Uuid::parse_str(value).map_err(|_| "workspace.denied")?;
    if token.get_version_num() != 4
        || token.get_variant() != uuid::Variant::RFC4122
        || value.len() != 36
        || token.to_string() != value
    {
        return Err("workspace.denied");
    }
    Ok(token)
}
#[tauri::command]
pub(super) async fn workspace_keymap_read(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Projection> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    let directory = window
        .app_handle()
        .path()
        .app_config_dir()
        .map_err(|_| "workspace.keymap_unavailable")?;
    eprintln!("workspace.keymap_read_requested");
    let result = state.0.keymaps.read(page, directory).await;
    match &result {
        Ok(_) => eprintln!("workspace.keymap_read_returned"),
        Err(code) => eprintln!("workspace.keymap_read_failed.{code}"),
    }
    result
}
#[tauri::command]
pub(super) async fn workspace_keymap_confirm(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
) -> Result<Projection> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    eprintln!("workspace.keymap_confirm_requested");
    let result = state.0.keymaps.confirm(page, token(&ticket)?).await;
    match &result {
        Ok(_) => eprintln!("workspace.keymap_confirm_returned"),
        Err(code) => eprintln!("workspace.keymap_confirm_failed.{code}"),
    }
    result
}
#[tauri::command]
pub(super) async fn workspace_keymap_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
    change: Change,
    consent: bool,
) -> Result<Projection> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    let result = state
        .0
        .keymaps
        .save(page, token(&ticket)?, change, consent)
        .await;
    match &result {
        Ok(_) => eprintln!("workspace.keymap_save_returned"),
        Err(code) => eprintln!("workspace.keymap_save_failed.{code}"),
    }
    result
}
#[tauri::command]
pub(super) fn workspace_keymap_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    state.0.keymaps.cancel_page(&page)
}

#[cfg(all(test, unix))]
#[path = "product_keymap_tests.rs"]
mod tests;
