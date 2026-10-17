//! Backend-only PTY launch supervision. Call only after page/Profile authorization.
//! A reservation is not an executable, input, or window permission.
use crate::{
    LocalSessionError, LocalSessionReceipt,
    registry::{PageLaunchScope, RegistryError, SessionId, SessionRegistry, WindowToken},
};
use pulse_core::TermSize;
use pulse_pty::{
    PtyCommand,
    launch::LocalSessionEnvironment,
    profiles::{
        AutoProfileCatalog, AutoProfileLaunch, ProfileLaunchError, ShellCandidate, ShellKind,
    },
};
use pulse_store::{StateStore, StoreError};
use pulse_term::session::SessionHandle;
use tokio::{runtime::Handle, sync::oneshot, task::JoinHandle};

/// Backend-only errors; native diagnostics may contain local paths and must not be sent to a page.
#[derive(Debug, thiserror::Error)]
pub enum LocalStartError {
    /// Window, quota, or stale launch ticket rejected the operation.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Catalog identity or launch options were invalid before any child was spawned.
    #[error(transparent)]
    Profile(#[from] ProfileLaunchError),
    /// Stored Profile preferences could not be loaded; never treat failure as defaults.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Native child setup failed; never serialize the underlying diagnostics to a page.
    #[error(transparent)]
    Native(#[from] LocalSessionError),
    /// No runtime or supervising worker was available.
    #[error("local launch supervisor unavailable")]
    Worker,
}

/// A committed actor; the background supervisor continues to own and join the native child.
/// A caller must explicitly retire this session from the registry after observing completion.
pub struct RegisteredLocalSession {
    // Set only by the committing supervisor. Never supplied by a page or another registry.
    pub(crate) registry: SessionRegistry,
    /// Backend identity, not a page capability.
    pub(crate) id: SessionId,
    /// Actor handle for the already-authorized runtime adapter.
    pub(crate) handle: SessionHandle,
    /// True actor EOF and native exit; not an OSC 133 command status.
    pub(crate) completion: oneshot::Receiver<Result<LocalSessionReceipt, LocalSessionError>>,
}

impl RegisteredLocalSession {
    /// Stable backend identity. This does not grant page or input permission.
    pub fn id(&self) -> SessionId {
        self.id
    }
    /// Read-only access to the authorized actor; root ownership must be adopted promptly.
    pub fn handle(&self) -> &SessionHandle {
        &self.handle
    }
}

/// An in-flight reservation and native spawn. Dropping this receipt does not abort the worker:
/// it closes/joins an already spawned child before releasing the quota reservation.
#[must_use]
pub struct PendingLocalLaunch {
    receipt: oneshot::Receiver<Result<RegisteredLocalSession, LocalStartError>>,
    accepted: oneshot::Sender<()>,
    worker: JoinHandle<Result<(), LocalStartError>>,
}

impl PendingLocalLaunch {
    /// Accept ownership only after receiving the completed registration receipt. If the
    /// caller is cancelled before the acknowledgement, the supervisor closes and reaps it.
    pub async fn wait(self) -> Result<RegisteredLocalSession, LocalStartError> {
        let Self {
            receipt,
            accepted,
            worker: _,
        } = self;
        let result = receipt.await.map_err(|_| LocalStartError::Worker)?;
        if result.is_ok() {
            accepted.send(()).map_err(|_| LocalStartError::Worker)?;
        }
        result
    }

    /// Suppress delivery, then join the supervisor so quota/child cleanup has actually ended.
    pub fn cancel(self) -> CancelledLocalLaunch {
        let Self {
            receipt,
            accepted,
            worker,
        } = self;
        drop(receipt);
        drop(accepted);
        CancelledLocalLaunch { worker }
    }
}

/// Await a cancelled launch's cleanup. Dropping this waiter never aborts its supervisor.
#[must_use]
pub struct CancelledLocalLaunch {
    worker: JoinHandle<Result<(), LocalStartError>>,
}
impl CancelledLocalLaunch {
    /// Await child join and any registry rollback without a fixed UI timer assumption.
    pub async fn join(self) -> Result<(), LocalStartError> {
        self.worker.await.map_err(|_| LocalStartError::Worker)?
    }
}

/// Reserve before spawning a child. `command` must originate from a separately authorized
/// current Profile and page, never from a frontend executable/path/argv parameter. Only a
/// trusted Rust adapter may call this. This process-local supervisor does not guarantee
/// descendant-tree termination or survive destruction of the Tokio runtime.
pub fn start_local_registered(
    registry: &SessionRegistry,
    owner: WindowToken,
    command: PtyCommand,
    size: TermSize,
    history: usize,
) -> Result<PendingLocalLaunch, LocalStartError> {
    let runtime = Handle::try_current().map_err(|_| LocalStartError::Worker)?;
    let ticket = registry.begin_local_launch(owner)?;
    spawn_reserved_local(
        registry,
        ticket,
        command,
        size,
        history,
        runtime,
        pulse_term::QueryPalette::default(),
    )
}

fn spawn_reserved_local(
    registry: &SessionRegistry,
    mut ticket: crate::registry::LocalLaunchTicket,
    command: PtyCommand,
    size: TermSize,
    history: usize,
    runtime: Handle,
    config: impl Into<crate::TerminalStartConfig>,
) -> Result<PendingLocalLaunch, LocalStartError> {
    let config = config.into();
    let registry = registry.clone();
    let (sender, receipt) = oneshot::channel();
    let (accepted, confirmation) = oneshot::channel();
    let mut native_work = registry.native_workers.reserve()?;
    let worker = runtime.spawn(async move {
        if sender.is_closed() || native_work.sealed() {
            return Ok(());
        }
        if let Err(e) = ticket.require_current() {
            drop(ticket);
            let _ = sender.send(Err(LocalStartError::Registry(e)));
            return Ok(());
        }
        native_work.start();
        match crate::spawn_local_with_palette(command, size, history, config).await {
            Err(e) => {
                if e.rejected_before_native() {
                    // Typed cwd rejection happens before PTY/child/thread allocation.
                    // The blocking setup has returned; unknown/partial setup remains fail closed.
                    native_work.complete();
                    drop(ticket);
                    let _ = sender.send(Err(LocalStartError::Native(e)));
                    return Ok(());
                }
                // Setup errors/partial thread creation are not a native join proof.
                ticket.retain_failed();
                let _ = sender.send(Err(LocalStartError::Native(e)));
                Err(LocalStartError::Worker)
            }
            Ok((handle, task)) => {
                if sender.is_closed() || native_work.sealed() {
                    handle.close();
                    match task.join().await {
                        Ok(_) => {
                            native_work.complete();
                            Ok(())
                        }
                        Err(e) => {
                            ticket.retain_failed();
                            Err(LocalStartError::Native(e))
                        }
                    }
                } else {
                    match registry.commit_local_launch(&mut ticket, handle.clone()) {
                        Err(e) => {
                            handle.close();
                            match task.join().await {
                                Ok(_) => {
                                    native_work.complete();
                                    drop(ticket);
                                    let _ = sender.send(Err(LocalStartError::Registry(e)));
                                    Ok(())
                                }
                                Err(cleanup) => {
                                    ticket.retain_failed();
                                    let _ = sender.send(Err(LocalStartError::Registry(e)));
                                    Err(LocalStartError::Native(cleanup))
                                }
                            }
                        }
                        Ok(id) => {
                            let (completion_sender, completion) = oneshot::channel();
                            let ready = RegisteredLocalSession {
                                registry: registry.clone(),
                                id,
                                handle: handle.clone(),
                                completion,
                            };
                            let accepted = if sender.send(Ok(ready)).is_err() {
                                false
                            } else {
                                tokio::select! {
                                    biased;
                                    // A receipt already handed to a root owner must retain its
                                    // completion channel; shutdown closes that owner separately.
                                    result = confirmation => result.is_ok(),
                                    () = native_work.wait_sealed() => false,
                                }
                            };
                            if !accepted {
                                handle.close();
                                task.join().await.map_err(LocalStartError::Native)?;
                                registry.reap_closed(id)?;
                                native_work.complete();
                                Ok(())
                            } else {
                                let result = task.join_with_receipt().await;
                                let verified = result.is_ok();
                                let _ = completion_sender.send(result);
                                if verified {
                                    native_work.complete();
                                    Ok(())
                                } else {
                                    Err(LocalStartError::Worker)
                                }
                            }
                        }
                    }
                }
            }
        }
    });
    Ok(PendingLocalLaunch {
        receipt,
        accepted,
        worker,
    })
}

/// A literal requested Profile identity and options, not an executable or page permission.
pub struct ProfileSelection {
    /// Stable catalog identity supplied by a separately authorized backend.
    pub kind: ShellKind,
    /// Literal arguments and environment edits validated by the Profile preparation policy.
    pub options: AutoProfileLaunch,
}

/// Resolve the executable and shell classification only from the current backend catalog,
/// then enter the same bounded/acknowledged PTY launch path. Missing, unavailable, and
/// deleted profiles fail rather than invoking a default shell. The caller must independently
/// authorize the current page and own the pane/environment metadata; this is not an IPC API.
pub fn start_profile_local(
    registry: &SessionRegistry,
    owner: WindowToken,
    catalog: &AutoProfileCatalog,
    selection: ProfileSelection,
    context: &LocalSessionEnvironment,
    size: TermSize,
    history: usize,
) -> Result<PendingLocalLaunch, LocalStartError> {
    let command = catalog.prepare_launch(selection.kind, selection.options, context)?;
    start_local_registered(registry, owner, command, size, history)
}

/// Page-scoped variant for a future native adapter. It MUST first confirm the actual
/// local-origin document nonce and WindowToken, then own/drop the resulting scope on
/// navigation. The scope does not itself authenticate a page and is never an IPC value.
/// Reservations, pre-spawn checks, and commit all reject a retired page generation.
pub fn start_page_profile_local(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    catalog: &AutoProfileCatalog,
    selection: ProfileSelection,
    context: &LocalSessionEnvironment,
    size: TermSize,
    history: usize,
) -> Result<PendingLocalLaunch, LocalStartError> {
    let command = catalog.prepare_launch(selection.kind, selection.options, context)?;
    let runtime = Handle::try_current().map_err(|_| LocalStartError::Worker)?;
    let ticket = registry.begin_page_local_launch(page)?;
    spawn_reserved_local(
        registry,
        ticket,
        command,
        size,
        history,
        runtime,
        pulse_term::QueryPalette::default(),
    )
}

/// The restricted local-shell entry for a product page. The page may name only a
/// current catalog identity; executable, arguments, working directory, environment,
/// and platform login policy remain backend-owned. This does not authenticate a page:
/// the native adapter must confirm its origin, WindowToken, and current document
/// before passing the corresponding scope. Never convert a raw IPC path into a catalog.
pub fn start_page_default_profile_local(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    catalog: &AutoProfileCatalog,
    kind: ShellKind,
    context: &LocalSessionEnvironment,
    size: TermSize,
    history: usize,
) -> Result<PendingLocalLaunch, LocalStartError> {
    start_page_profile_local(
        registry,
        page,
        catalog,
        ProfileSelection {
            kind,
            options: AutoProfileLaunch::default(),
        },
        context,
        size,
        history,
    )
}

/// Trusted discovery and bounded PTY configuration for a single stored Profile launch.
/// Construct only from backend state; never deserialize candidate paths from a page.
#[derive(Clone, Copy)]
pub struct StoredProfileLaunch<'a> {
    /// Current backend-discovered executable candidates.
    pub candidates: &'a [ShellCandidate],
    /// Stable catalog identity selected by the page.
    pub kind: ShellKind,
    /// Backend-owned process environment metadata.
    pub context: &'a LocalSessionEnvironment,
    /// Validated terminal size.
    pub size: TermSize,
    /// Backend-bounded scrollback capacity.
    pub history: usize,
}

/// Load validated Profile preferences from the private store for each request,
/// then reconcile trusted backend discovery and reserve the still-current page.
/// The store read is not a permission grant: navigation during that await is
/// rejected by the generation check in `begin_page_local_launch`. A failed
/// read or invalid catalog cannot fall back to a default or spawn a child.
/// This is a trusted Rust interface; candidates must never be supplied by IPC.
pub async fn start_page_stored_default_profile_local(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    store: &StateStore,
    spec: StoredProfileLaunch<'_>,
) -> Result<PendingLocalLaunch, LocalStartError> {
    Ok(
        start_page_stored_configured_local(registry, page, store, spec)
            .await?
            .0,
    )
}

pub(crate) async fn start_page_stored_configured_local(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    store: &StateStore,
    spec: StoredProfileLaunch<'_>,
) -> Result<
    (
        PendingLocalLaunch,
        pulse_store::profile_editor::ProfileAppearance,
    ),
    LocalStartError,
> {
    start_page_stored_configured_local_with_preset(
        registry,
        page,
        store,
        spec,
        pulse_store::settings::TerminalPreset::Classic,
    )
    .await
}

pub(crate) async fn start_page_stored_configured_local_with_preset(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    store: &StateStore,
    spec: StoredProfileLaunch<'_>,
    preset: impl Into<pulse_store::settings::TerminalStartup>,
) -> Result<
    (
        PendingLocalLaunch,
        pulse_store::profile_editor::ProfileAppearance,
    ),
    LocalStartError,
> {
    use pulse_store::profiles::AutomaticShell;
    let shell = match spec.kind {
        ShellKind::Bash => AutomaticShell::Bash,
        ShellKind::Zsh => AutomaticShell::Zsh,
        ShellKind::Fish => AutomaticShell::Fish,
        ShellKind::Sh => AutomaticShell::Sh,
        ShellKind::Pwsh => AutomaticShell::Pwsh,
    };
    registry.check_launch_page(page)?;
    let snapshot = store.load_profile_editor(shell).await?;
    let catalog =
        crate::profile_state::restore_catalog(snapshot.lifecycle().preferences(), spec.candidates)
            .map_err(ProfileLaunchError::Profile)?;
    let config = snapshot.config();
    config.validate()?;
    let options = AutoProfileLaunch {
        args: config.args.into_iter().map(Into::into).collect(),
        cwd: config.cwd.map(Into::into),
        env: config
            .env
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        env_remove: config.env_remove.into_iter().map(Into::into).collect(),
        ..Default::default()
    };
    let command = catalog.prepare_launch(spec.kind, options, spec.context)?;
    let runtime = Handle::try_current().map_err(|_| LocalStartError::Worker)?;
    let ticket = registry.begin_page_local_launch(page)?;
    let mut startup = preset.into();
    if let Some(preset) = config.appearance.color_preset {
        startup.preset = preset;
    }
    let palette = crate::TerminalStartConfig::from(startup);
    let pending = spawn_reserved_local(
        registry,
        ticket,
        command,
        spec.size,
        spec.history,
        runtime,
        palette,
    )?;
    Ok((pending, config.appearance))
}

/// Backend-selected persistent custom Profile identity and local runtime context.
/// This structure is never accepted from a page; only the opaque ID comes from IPC.
pub struct CustomProfileLaunch<'a> {
    /// UUID of a saved, active custom Profile.
    pub id: uuid::Uuid,
    /// Backend-owned application and CLI routing environment.
    pub context: &'a LocalSessionEnvironment,
    /// Native-selected initial geometry.
    pub size: TermSize,
    /// Retained until the physical SQLite read exits, including waiter cancellation.
    pub store_retention: Box<dyn Send>,
}
/// Re-read a saved custom Profile, reserve the original page, and supervise native startup.
/// Arguments are literal and no automatic login flag or executable fallback is supplied.
pub async fn start_page_custom_local_with_preset(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    store: &StateStore,
    spec: CustomProfileLaunch<'_>,
    mut startup: pulse_store::settings::TerminalStartup,
) -> Result<
    (
        PendingLocalLaunch,
        pulse_store::profile_editor::ProfileAppearance,
    ),
    LocalStartError,
> {
    registry.check_launch_page(page)?;
    let source = store
        .load_custom_profiles_retaining(spec.store_retention)
        .await?;
    let profile = source.active(spec.id)?;
    let draft = profile.draft();
    draft.validate()?;
    let config = &draft.config;
    let mut command = PtyCommand::new(draft.executable.clone());
    command.args = config.args.iter().map(Into::into).collect();
    command.cwd = config.cwd.as_ref().map(Into::into);
    command.env = config
        .env
        .iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
    command.env_remove = config.env_remove.iter().map(Into::into).collect();
    let command = pulse_pty::launch::prepare_local_command(command, spec.context, None)
        .map_err(ProfileLaunchError::Launch)?;
    let runtime = Handle::try_current().map_err(|_| LocalStartError::Worker)?;
    let ticket = registry.begin_page_local_launch(page)?;
    if let Some(preset) = config.appearance.color_preset {
        startup.preset = preset;
    }
    let pending = spawn_reserved_local(
        registry,
        ticket,
        command,
        spec.size,
        startup.history_limit,
        runtime,
        crate::TerminalStartConfig::from(startup),
    )?;
    Ok((pending, config.appearance.clone()))
}

#[cfg(all(test, unix))]
#[path = "local_launch_default_tests.rs"]
mod default_tests;
#[cfg(all(test, unix))]
#[path = "local_launch_page_tests.rs"]
mod page_tests;
#[cfg(all(test, unix))]
#[path = "local_launch_profile_tests.rs"]
mod profile_tests;
#[cfg(all(test, unix))]
#[path = "local_launch_revocation_tests.rs"]
mod revocation_tests;
/// Bounded, path-free offer for the backend's current discovered Profile catalog.
/// This is not a launch permission; the native adapter must separately authorize
/// the requesting page and verify the nonce before publishing the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileOffer {
    /// Stable automatic Profile identity. No executable path or arguments.
    pub kind: ShellKind,
    /// Hidden Profiles remain explicitly selectable, but are omitted from a normal picker.
    pub hidden: bool,
}

/// Read the private store and current trusted discovery for a single page.
/// Both checks bracket the asynchronous read; a list is only a momentary snapshot,
/// and launching later must redo the store read and reserve the current page.
pub async fn list_page_profile_offers(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    store: &StateStore,
    candidates: &[ShellCandidate],
) -> Result<Vec<ProfileOffer>, LocalStartError> {
    registry.check_launch_page(page)?;
    let preferences = store.load_auto_profiles().await?;
    let catalog = crate::profile_state::restore_catalog(&preferences, candidates)
        .map_err(ProfileLaunchError::Profile)?;
    let offers = catalog
        .profiles()
        .filter(|profile| profile.is_available())
        .map(|profile| ProfileOffer {
            kind: profile.kind(),
            hidden: profile.is_hidden(),
        })
        .collect();
    registry.check_launch_page(page)?;
    Ok(offers)
}

#[cfg(all(test, unix))]
#[path = "local_launch_stored_tests.rs"]
mod stored_tests;
#[cfg(test)]
#[path = "local_launch_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "local_launch_custom_tests.rs"]
mod custom_tests;
