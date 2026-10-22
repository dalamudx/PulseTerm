//! Single-flight, supervised Profile reads. All paths and discovery inputs are backend-owned.
use super::{ProfileOfferDto, Result};
use pulse_app::registry::{PageLaunchScope, SessionRegistry};
use pulse_pty::profiles::ShellCandidate;
use pulse_store::{StateStore, StateStoreTask};
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{OnceCell, OwnedSemaphorePermit, Semaphore, watch};

struct StoreState {
    directory: PathBuf,
    store: StateStore,
    task: Mutex<Option<StateStoreTask>>,
}
/// Discovery and startup parameters belong solely to the native adapter.
pub(super) struct ProfileStart {
    pub preset: pulse_store::settings::TerminalStartup,
    pub directory: PathBuf,
    pub path: OsString,
    pub shell: Option<PathBuf>,
    pub kind: pulse_pty::profiles::ShellKind,
}

pub(super) struct ProfileReader {
    default_ticket: Mutex<Option<default_service::DefaultTicket>>,
    default_active: Mutex<Option<default_service::DefaultControl>>,
    editor_ticket: Mutex<Option<editor_service::EditorTicket>>,
    editor_active: Mutex<Option<editor_service::EditorControl>>,
    custom_editor: custom_service::CustomEditor,
    lifecycle_ticket: Mutex<Option<lifecycle_service::LifecycleTicket>>,
    lifecycle_active: Mutex<Option<lifecycle_service::LifecycleControl>>,
    storage: Arc<storage::ProfileStorage>,
    credit: Arc<Semaphore>,
    read_waiter: Arc<Semaphore>,
    read_retired: watch::Sender<()>,
    closed: AtomicBool,
    idle: watch::Sender<bool>,
    shutdown: OnceLock<watch::Receiver<Option<Result<()>>>>,
}
// Real-auth fixtures share production single-job KDF admission, not private independent quotas.
#[cfg(test)]
static VAULT_TEST_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
type Credit = Arc<ReadCredit>;
impl std::ops::Deref for ProfileReader {
    type Target = storage::ProfileStorage;
    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}
pub(super) struct ReadCredit {
    permit: Option<OwnedSemaphorePermit>,
    storage_credit: Option<storage::StorageCredit>,
    idle: watch::Sender<bool>,
}
impl Drop for ReadCredit {
    fn drop(&mut self) {
        drop(self.storage_credit.take());
        drop(self.permit.take());
        self.idle.send_replace(true);
    }
}

// The blocking closure, not its awaiting future, owns credit until it really exits.
async fn blocking<T: Send + 'static>(
    credit: Credit,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(move || {
        let _credit = credit;
        work()
    })
    .await
    .map_err(|_| "workspace.unavailable")?
}
fn identity_error(error: pulse_store::StoreError) -> &'static str {
    match error {
        pulse_store::StoreError::IdentityAuthorization => "workspace.denied",
        pulse_store::StoreError::IdentityConflict => "workspace.identities_conflict",
        pulse_store::StoreError::IdentityExhausted => "workspace.identities_limit",
        pulse_store::StoreError::Busy => "workspace.busy",
        _ => "workspace.identities_failed",
    }
}
pub(super) use storage::ProfileStorage;
impl ProfileReader {
    pub(super) fn new() -> Self {
        Self::with_storage(Arc::new(storage::ProfileStorage::new()))
    }
    pub(super) fn with_storage(storage: Arc<storage::ProfileStorage>) -> Self {
        Self {
            default_ticket: Mutex::new(None),
            default_active: Mutex::new(None),
            editor_ticket: Mutex::new(None),
            editor_active: Mutex::new(None),
            custom_editor: custom_service::CustomEditor::default(),
            lifecycle_ticket: Mutex::new(None),
            lifecycle_active: Mutex::new(None),
            storage,
            credit: Arc::new(Semaphore::new(1)),
            read_waiter: Arc::new(Semaphore::new(1)),
            read_retired: watch::channel(()).0,
            closed: AtomicBool::new(false),
            idle: watch::channel(true).0,
            shutdown: OnceLock::new(),
        }
    }
    pub(super) fn shared_storage(&self) -> Arc<storage::ProfileStorage> {
        self.storage.clone()
    }
    pub(super) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Ok(mut ticket) = self.default_ticket.lock() {
            ticket.take();
        }
        if let Ok(mut active) = self.default_active.lock() {
            active.take();
        }
        self.credit.close();
        self.read_waiter.close();
        self.retire_read_waiters();
    }
    pub(super) fn retire_read_waiters(&self) {
        self.custom_editor.revoke();
        if let Ok(mut ticket) = self.editor_ticket.lock() {
            ticket.take();
        }
        if let Ok(mut active) = self.editor_active.lock() {
            active.take();
        }
        if let Ok(mut ticket) = self.lifecycle_ticket.lock() {
            ticket.take();
        }
        if let Ok(mut active) = self.lifecycle_active.lock() {
            active.take();
        }
        self.read_retired.send_replace(());
    }
    fn reserve_local(&self) -> Result<ReadCredit> {
        if self.closed.load(Ordering::SeqCst) || self.storage.is_closed() {
            return Err("workspace.denied");
        }
        let permit = self
            .credit
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        self.idle.send_replace(false);
        Ok(ReadCredit {
            permit: Some(permit),
            storage_credit: None,
            idle: self.idle.clone(),
        })
    }
    async fn reserve_list(
        &self,
        registry: &SessionRegistry,
        page: &PageLaunchScope,
    ) -> Result<Credit> {
        self.check(registry, page)?;
        let _single_waiter = self
            .read_waiter
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        let mut retired = self.read_retired.subscribe();
        self.check(registry, page)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let permit = tokio::select! {biased;
            _=retired.changed()=>return Err("workspace.denied"),
            _=tokio::time::sleep_until(deadline)=>return Err("workspace.read_admission_timeout"),
            permit=self.credit.clone().acquire_owned()=>permit.map_err(|_| "workspace.denied")?,
        };
        self.idle.send_replace(false);
        let mut credit = ReadCredit {
            permit: Some(permit),
            storage_credit: None,
            idle: self.idle.clone(),
        };
        credit.storage_credit = Some(tokio::select! {biased;
            _=retired.changed()=>return Err("workspace.denied"),
            _=tokio::time::sleep_until(deadline)=>return Err("workspace.read_admission_timeout"),
            permit=self.storage.reserve_wait()=>permit?,
        });
        self.check(registry, page)?;
        Ok(Arc::new(credit))
    }
    pub(super) fn reserve(&self) -> Result<Credit> {
        let mut credit = self.reserve_local()?;
        credit.storage_credit = Some(self.storage.reserve()?);
        if self.closed.load(Ordering::SeqCst) {
            return Err("workspace.denied");
        }
        Ok(Arc::new(credit))
    }
    /// Single-reader fixture compatibility; production drains every window before application exit.
    #[cfg(test)]
    pub(super) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        let (window, storage) = tokio::join!(self.drain_window(), self.storage.shutdown());
        window?;
        storage
    }
    /// Retire only this window, preserving the shared worker for other readers.
    pub(super) async fn drain_window(self: &Arc<Self>) -> Result<()> {
        shutdown::drain(self).await
    }
    fn check(&self, registry: &SessionRegistry, page: &PageLaunchScope) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) || self.storage.is_closed() {
            return Err("workspace.denied");
        }
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")
    }
    pub(super) async fn identities(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
    ) -> Result<pulse_store::ssh_identities::IdentitySnapshot> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let snapshot = store
                .load_ssh_identities(gate)
                .await
                .map_err(identity_error)?;
            reader.check(&registry, &page)?;
            Ok(snapshot)
        })
        .await
        .map_err(|_| "workspace.identities_failed")?
    }
    pub(super) async fn edit_identity(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        expected: pulse_store::ssh_identities::IdentitySnapshot,
        change: pulse_store::ssh_identities::IdentityEdit,
    ) -> Result<pulse_store::ssh_identities::IdentitySnapshot> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let snapshot = store
                .edit_ssh_identities(expected, change, gate)
                .await
                .map_err(identity_error)?;
            reader.check(&registry, &page)?;
            Ok(snapshot)
        })
        .await
        .map_err(|_| "workspace.identities_failed")?
    }

    // Root-supervised metadata-only read: no shell discovery, startup, trust or dial.
    pub(super) async fn hosts(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
    ) -> Result<pulse_store::ssh_hosts::HostSnapshot> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let snapshot =
                store
                    .load_ssh_hosts_authorized(gate)
                    .await
                    .map_err(|error| match error {
                        pulse_store::StoreError::HostAuthorization => "workspace.denied",
                        pulse_store::StoreError::Busy => "workspace.busy",
                        _ => "workspace.hosts_failed",
                    })?;
            reader.check(&registry, &page)?;
            Ok(snapshot)
        })
        .await
        .map_err(|_| "workspace.hosts_failed")?
    }
    pub(super) async fn edit_host(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        expected: pulse_store::ssh_hosts::HostSnapshot,
        change: pulse_store::ssh_hosts::HostEdit,
    ) -> Result<pulse_store::ssh_hosts::HostSnapshot> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_actual_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let snapshot = store
                .edit_ssh_hosts(expected, change, gate)
                .await
                .map_err(|error| match error {
                    pulse_store::StoreError::HostAuthorization => "workspace.denied",
                    pulse_store::StoreError::HostConflict => "workspace.hosts_conflict",
                    pulse_store::StoreError::Busy => "workspace.busy",
                    _ => "workspace.hosts_failed",
                })?;
            reader.check(&registry, &page)?;
            Ok(snapshot)
        })
        .await
        .map_err(|_| "workspace.hosts_failed")?
    }
    pub(super) async fn recent_hosts(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
    ) -> Result<Vec<pulse_store::recent_hosts::RecentHost>> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let result = store
                .load_recent_ssh_hosts(gate)
                .await
                .map_err(|_| "workspace.recent_hosts_failed")?;
            reader.check(&registry, &page)?;
            Ok(result)
        })
        .await
        .map_err(|_| "workspace.recent_hosts_failed")?
    }
    pub(super) async fn ssh_profiles(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        change: Option<(
            pulse_store::ssh_profiles::ProfileSnapshot,
            pulse_store::ssh_profiles::ProfileEdit,
            Option<Option<uuid::Uuid>>,
        )>,
    ) -> Result<pulse_store::ssh_profiles::ProfileSnapshot> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let result = match change {
                None => store.load_ssh_profiles_authorized(gate).await,
                Some((expected, edit, Some(via))) => {
                    store
                        .edit_ssh_profile_with_jump(expected, edit, via, gate)
                        .await
                }
                Some((expected, edit, None)) => store.edit_ssh_profile(expected, edit, gate).await,
            }
            .map_err(|error| match error {
                pulse_store::StoreError::HostAuthorization => "workspace.denied",
                pulse_store::StoreError::HostConflict => "workspace.ssh_profiles_conflict",
                pulse_store::StoreError::Busy => "workspace.busy",
                _ => "workspace.ssh_profiles_failed",
            })?;
            reader.check(&registry, &page)?;
            Ok(result)
        })
        .await
        .map_err(|_| "workspace.ssh_profiles_failed")?
    }
    pub(super) async fn behavior(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        scope: pulse_store::settings_scope::SettingsScope,
        change: Option<(
            pulse_store::organization_behavior::Snapshot,
            pulse_store::organization_behavior::Overrides,
        )>,
    ) -> Result<pulse_store::organization_behavior::Snapshot> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let (expected, replacement) =
                change.map_or((None, None), |(source, patch)| (Some(source), Some(patch)));
            let result = store
                .organization_behavior(scope, expected, replacement, gate)
                .await;
            reader.check(&registry, &page)?;
            result.map_err(|_| "workspace.organization_conflict")
        })
        .await
        .map_err(|_| "workspace.organization_failed")?
    }
    pub(super) async fn organization(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        change: Option<(
            pulse_store::catalogue_organization::Snapshot,
            pulse_store::catalogue_organization::Edit,
        )>,
    ) -> Result<pulse_store::catalogue_organization::Snapshot> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let result = match change {
                None => store.load_catalogue_organization(gate).await,
                Some((expected, edit)) => {
                    store
                        .edit_catalogue_organization(expected, edit, gate)
                        .await
                }
            }
            .map_err(|error| match error {
                pulse_store::StoreError::OrganizationAuthorization => "workspace.denied",
                pulse_store::StoreError::OrganizationConflict => "workspace.organization_conflict",
                pulse_store::StoreError::Busy => "workspace.busy",
                _ => "workspace.organization_failed",
            })?;
            reader.check(&registry, &page)?;
            Ok(result)
        })
        .await
        .map_err(|_| "workspace.organization_failed")?
    }
    pub(super) async fn start(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        root: Arc<Mutex<pulse_app::local_session_root::LocalSessionRoot>>,
        source: ProfileStart,
    ) -> Result<pulse_app::registry::SessionId> {
        self.start_interruptible(registry, page, root, source, std::future::pending())
            .await
    }
    pub(super) async fn start_interruptible(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        root: Arc<Mutex<pulse_app::local_session_root::LocalSessionRoot>>,
        source: ProfileStart,
        stopped: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<pulse_app::registry::SessionId> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        // Detached supervision owns both I/O credit and native startup. Navigation
        // revokes admission; an already-adopted session belongs to the root.
        tokio::spawn(async move {
            let (store, candidates) = reader
                .prepare(
                    &registry,
                    &page,
                    source.directory,
                    source.path,
                    source.shell,
                    credit.clone(),
                )
                .await?;
            reader.check(&registry, &page)?;
            let context = pulse_pty::launch::LocalSessionEnvironment {
                application_version: env!("CARGO_PKG_VERSION").into(),
                pane: None,
            };
            let spec = pulse_app::local_launch::StoredProfileLaunch {
                candidates: &candidates,
                kind: source.kind,
                context: &context,
                size: pulse_core::TermSize::new(80, 24).map_err(|_| "workspace.unavailable")?,
                history: 10_000,
            };
            let id = tokio::select! { biased;
                _ = stopped => return Err("workspace.denied"),
                result = pulse_app::local_session_root::start_stored_page_local_with_preset(
                    &registry, &page, &store, &root, spec, source.preset) => result.map_err(|_| "workspace.unavailable")?,
            };
            drop(credit);
            Ok(id)
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    pub(super) async fn set_visibility(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        source: ProfileStart,
        hidden: bool,
    ) -> Result<()> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        // The supervisor outlives an IPC waiter; the worker also owns a credit clone
        // in its execution predicate so runtime shutdown cannot free active SQLite I/O.
        tokio::spawn(async move {
            let (store, candidates) = reader
                .prepare(
                    &registry,
                    &page,
                    source.directory,
                    source.path,
                    source.shell,
                    credit.clone(),
                )
                .await?;
            // Discovery supplies identity only. Current preference validity and deletion
            // tombstones are checked by the same guarded IMMEDIATE edit transaction.
            if !candidates
                .iter()
                .any(|candidate| candidate.kind() == source.kind)
            {
                return Err("workspace.denied");
            }
            let shell = match source.kind {
                pulse_pty::profiles::ShellKind::Bash => pulse_store::profiles::AutomaticShell::Bash,
                pulse_pty::profiles::ShellKind::Zsh => pulse_store::profiles::AutomaticShell::Zsh,
                pulse_pty::profiles::ShellKind::Fish => pulse_store::profiles::AutomaticShell::Fish,
                pulse_pty::profiles::ShellKind::Sh => pulse_store::profiles::AutomaticShell::Sh,
                pulse_pty::profiles::ShellKind::Pwsh => pulse_store::profiles::AutomaticShell::Pwsh,
                pulse_pty::profiles::ShellKind::PowerShell => {
                    pulse_store::profiles::AutomaticShell::PowerShell
                }
                pulse_pty::profiles::ShellKind::Cmd => pulse_store::profiles::AutomaticShell::Cmd,
                pulse_pty::profiles::ShellKind::Wsl => pulse_store::profiles::AutomaticShell::Wsl,
            };
            reader.check(&registry, &page)?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate_credit = credit.clone();
            store
                .edit_auto_profiles_authorized(
                    pulse_store::profiles::AutoProfileEdit::Visibility { shell, hidden },
                    move || {
                        let _retain_until_worker_exit = &gate_credit;
                        gate_reader.check(&gate_registry, &gate_page).is_ok()
                    },
                )
                .await
                .map_err(|error| match error {
                    pulse_store::StoreError::ProfileAuthorization => "workspace.denied",
                    pulse_store::StoreError::Busy => "workspace.busy",
                    _ => "workspace.unavailable",
                })?;
            // A committed edit is not undone by retirement; a stale page cannot receive
            // a success receipt or use it as launch/configuration authority.
            reader.check(&registry, &page)?;
            Ok(())
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }

    pub(super) async fn list(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        path: OsString,
        shell: Option<PathBuf>,
    ) -> Result<Vec<ProfileOfferDto>> {
        self.check(&registry, &page)?;
        let credit = self.reserve_list(&registry, &page).await?;
        let reader = self.clone();
        // Dropping the IPC waiter does not drop the supervisor or free its credit.
        tokio::spawn(async move {
            reader
                .read(registry, page, directory, path, shell, credit)
                .await
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    async fn read(
        &self,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        path: OsString,
        shell: Option<PathBuf>,
        credit: Credit,
    ) -> Result<Vec<ProfileOfferDto>> {
        let (store, candidates) = self
            .prepare(&registry, &page, directory, path, shell, credit.clone())
            .await?;
        let offers = pulse_app::local_launch::list_page_profile_offers(
            &registry,
            &page,
            &store,
            &candidates,
        )
        .await
        .map_err(|_| "workspace.unavailable")?;
        self.check(&registry, &page)?;
        Ok(offers
            .into_iter()
            .map(|p| ProfileOfferDto {
                kind: p.kind.executable_name(),
                hidden: p.hidden,
            })
            .collect())
    }
    // Backend-owned store and discovery ONLY, not a permission or preference snapshot.
    // A subsequent launch must use start_stored_page_local to reread preferences and page.
    async fn prepare(
        &self,
        registry: &SessionRegistry,
        page: &PageLaunchScope,
        directory: PathBuf,
        path: OsString,
        shell: Option<PathBuf>,
        credit: Credit,
    ) -> Result<(StateStore, Vec<ShellCandidate>)> {
        let store = self
            .prepare_store(registry, page, directory, credit.clone())
            .await?;
        let candidates = blocking(credit, move || {
            #[cfg(unix)]
            let candidates = pulse_pty::profiles::discover_unix_shells(&path, shell.as_deref());
            #[cfg(windows)]
            let candidates = {
                let _ = (path, shell);
                let system = pulse_platform::windows::system_directory()
                    .map_err(|_| "workspace.unavailable")?;
                let programs = std::env::var_os("ProgramFiles").map(PathBuf::from);
                let distribution = pulse_platform::windows::default_wsl_distribution()
                    .ok()
                    .flatten();
                pulse_pty::profiles::discover_current_windows_shells(
                    &system,
                    programs.as_deref(),
                    distribution.as_deref(),
                )
            };
            candidates.map_err(|_| "workspace.unavailable")
        })
        .await?;
        self.check(registry, page)?;
        Ok((store, candidates))
    }
    // Same root-owned private database, independently of shell enumeration. This does
    // not publish a page receipt or authorize execution; callers recheck after I/O.
    async fn prepare_store(
        &self,
        registry: &SessionRegistry,
        page: &PageLaunchScope,
        directory: PathBuf,
        credit: Credit,
    ) -> Result<StateStore> {
        self.check(registry, page)?;
        let requested = directory.clone();
        let store = self
            .store
            .get_or_try_init(|| async {
                let directory =
                    blocking(credit.clone(), move || super::app_data::prepare(&directory)).await?;
                self.check(registry, page)?;
                #[cfg(unix)]
                let opened = StateStore::open(directory.join("state.db")).await;
                #[cfg(windows)]
                let opened = StateStore::open_admitted(
                    directory.join("state.db"),
                    super::database_native::admit,
                    credit.clone(),
                )
                .await;
                let (store, task) = opened.map_err(|_| "workspace.unavailable")?;
                if self.closed.load(Ordering::SeqCst) || self.storage.is_closed() {
                    task.shutdown().await.map_err(|_| "workspace.unavailable")?;
                    return Err("workspace.denied");
                }
                Ok::<_, &'static str>(StoreState {
                    directory,
                    store,
                    task: Mutex::new(Some(task)),
                })
            })
            .await?;
        if store.directory != requested {
            return Err("workspace.denied");
        }
        self.check(registry, page)?;
        Ok(store.store.clone())
    }
}
#[cfg(test)]
#[path = "product_profile_admission_tests.rs"]
mod admission_tests;
#[path = "product_profiles_agent_identities.rs"]
mod agent_identity_service;
#[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
#[path = "product_profiles_config.rs"]
mod config_service;
#[cfg(all(test, unix))]
#[path = "product_hosts_source_tests.rs"]
mod hosts_source_tests;
#[path = "product_profiles_key_identities.rs"]
mod key_identity_service;
#[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
pub(super) use config_service::{ConfigImportCapture, ImportControl};
#[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
pub(super) use config_service::{ConfigReadRequest, PageConfigCapture};
#[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
#[path = "product_profiles_key_write.rs"]
mod key_write_service;
#[cfg(all(test, unix))]
#[path = "product_profiles_launch_tests.rs"]
mod launch_tests;
#[path = "product_profiles_password.rs"]
mod password_service;
#[path = "product_profiles_shutdown.rs"]
mod shutdown;
#[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
#[path = "product_profiles_ssh.rs"]
mod ssh_service;
#[path = "product_profiles_storage.rs"]
mod storage;
#[cfg(all(test, unix))]
#[path = "product_profiles_tests.rs"]
mod tests;
#[path = "product_profiles_vault.rs"]
mod vault_service;

#[path = "product_profiles_default.rs"]
mod default_service;

#[path = "product_profiles_custom.rs"]
pub(super) mod custom_service;
#[path = "product_profiles_editor.rs"]
pub(super) mod editor_service;
#[path = "product_profiles_lifecycle.rs"]
mod lifecycle_service;
