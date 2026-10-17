//! Protocol-neutral native session ownership. This grants NO SSH, credential or page permission.
//! Trusted upper adapters must authenticate and authorize separately, and supply a join future
//! which succeeds ONLY after the terminal actor and every owned native I/O resource actually exit.
use crate::{
    local_session_owner::CleanupState,
    native_lifecycle::NativeWork,
    registry::{LocalLaunchTicket, PageLaunchScope, RegistryError, SessionId, SessionRegistry},
};
use pulse_term::session::SessionHandle;
use std::future::Future;
use tokio::{
    runtime::Handle,
    sync::{oneshot, watch},
    task::JoinHandle,
};

/// Content-free ownership failure; unknown native exit never releases capacity.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolSessionError {
    /// Page, source registry or shared admission rejected.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Native join failed or could not prove all resources exited.
    #[error("protocol native completion unknown")]
    NativeUnknown,
    /// No root runtime can supervise this operation.
    #[error("protocol supervisor unavailable")]
    Runtime,
}
type Result<T> = std::result::Result<T, ProtocolSessionError>;
type Completion = oneshot::Receiver<Result<()>>;

/// Reserve shared session AND native-worker capacity before a trusted adapter starts I/O.
/// Dropping an unused reservation releases it. This is not target-use authority or an IPC value.
#[must_use]
pub struct ProtocolSessionReservation {
    registry: SessionRegistry,
    runtime: Handle,
    ticket: LocalLaunchTicket,
    native: NativeWork,
}
impl ProtocolSessionReservation {
    /// Reserve against a backend-confirmed original page before starting protocol I/O.
    pub fn reserve(registry: &SessionRegistry, page: &PageLaunchScope) -> Result<Self> {
        let runtime = Handle::try_current().map_err(|_| ProtocolSessionError::Runtime)?;
        let ticket = registry.begin_page_local_launch(page)?;
        let native = registry.native_workers.reserve()?;
        Ok(Self {
            registry: registry.clone(),
            runtime,
            ticket,
            native,
        })
    }
    /// Call BEFORE actual protocol setup. Unknown/drop/panic after this point quarantines quota.
    /// The adapter must keep the result until it can supervise actual native completion.
    pub fn begin(mut self) -> Result<StartedProtocolSession> {
        self.ticket.require_current()?;
        if self.native.sealed() {
            return Err(RegistryError::Forbidden.into());
        }
        self.native.start();
        Ok(StartedProtocolSession {
            registry: self.registry,
            runtime: self.runtime,
            ticket: Some(self.ticket),
            native: Some(self.native),
        })
    }
}

/// Native setup has begun. Losing this object is UNKNOWN cleanup, not free capacity.
#[must_use]
pub struct StartedProtocolSession {
    registry: SessionRegistry,
    runtime: Handle,
    ticket: Option<LocalLaunchTicket>,
    native: Option<NativeWork>,
}
impl StartedProtocolSession {
    #[cfg(feature = "ssh-use")]
    pub(crate) fn check_original_page(&self, page: &PageLaunchScope) -> Result<()> {
        if self.native.as_ref().is_none_or(|native| native.sealed()) {
            return Err(RegistryError::Forbidden.into());
        }
        self.registry
            .check_protocol_ticket_page(self.ticket.as_ref().ok_or(RegistryError::Forbidden)?, page)
            .map_err(Into::into)
    }
    #[cfg(feature = "ssh-use")]
    pub(crate) fn source_registry(&self) -> &SessionRegistry {
        &self.registry
    }
    #[cfg(feature = "ssh-use")]
    pub(crate) fn root_runtime(&self) -> Handle {
        self.runtime.clone()
    }
    /// Commit the original page atomically using the same registry that reserved the slot.
    /// `native_join` must own (not merely observe) the complete native supervisor/actor join.
    /// It must not return success for channel closure, actor closure, cancellation or a timeout.
    /// Rejection closes the actor and supervises native_join in the root runtime before release.
    pub fn commit<F>(
        mut self,
        handle: SessionHandle,
        native_join: F,
    ) -> Result<ProtocolSessionOwner>
    where
        F: Future<Output = Result<()>> + Send + 'static,
    {
        let registry = self.registry.clone();
        let result = if self.native.as_ref().is_none_or(|native| native.sealed()) {
            Err(RegistryError::Forbidden)
        } else {
            self.ticket
                .as_mut()
                .ok_or(RegistryError::Forbidden)
                .and_then(|ticket| registry.commit_local_launch(ticket, handle.clone()))
        };
        let id = match result {
            Ok(id) => id,
            Err(error) => {
                handle.close();
                let runtime = self.runtime.clone();
                runtime.spawn(async move {
                    if native_join.await.is_ok() {
                        self.complete_native();
                    }
                    // Failure/drop leaves both admission ledgers quarantined.
                });
                return Err(error.into());
            }
        };
        let (sender, completion) = oneshot::channel();
        let runtime = self.runtime.clone();
        runtime.spawn(async move {
            let result = native_join.await;
            if result.is_ok() {
                self.complete_native();
            }
            let _ = sender.send(result);
        });
        let (state, _) = watch::channel(CleanupState::Owned);
        Ok(ProtocolSessionOwner {
            registry,
            runtime,
            id,
            handle,
            completion: Some(completion),
            state,
            appearance: Default::default(),
        })
    }
    /// Retire a started setup which produced no registered actor, only after actual native cleanup.
    /// The trusted adapter must initiate close and supply an owned close-and-join future.
    /// Dropping this observation cannot abort cleanup; failed or lost cleanup stays quarantined.
    pub fn finish_unregistered<F>(mut self, native_cleanup: F) -> ProtocolSessionCleanup
    where
        F: Future<Output = Result<()>> + Send + 'static,
    {
        let runtime = self.runtime.clone();
        ProtocolSessionCleanup {
            worker: runtime.spawn(async move {
                let result = native_cleanup.await;
                if result.is_ok() {
                    self.complete_native();
                }
                result
            }),
        }
    }
    fn complete_native(&mut self) {
        if let Some(native) = self.native.take() {
            native.complete();
        }
        // Only known physical completion may release an uncommitted reservation.
        self.ticket.take();
    }
}
impl Drop for StartedProtocolSession {
    fn drop(&mut self) {
        if self.native.is_some()
            && let Some(ticket) = self.ticket.as_mut()
        {
            ticket.retain_failed();
        }
        // NativeWork::drop records failed/unknown, never a successful drain.
    }
}

/// Root-owned protocol session. Its registry, ID, handle and completion cannot be replaced.
/// Keep it outside document/view state. Root destruction closes and detaches supervised cleanup.
#[must_use]
pub struct ProtocolSessionOwner {
    registry: SessionRegistry,
    runtime: Handle,
    id: SessionId,
    handle: SessionHandle,
    completion: Option<Completion>,
    state: watch::Sender<CleanupState>,
    appearance: pulse_store::profile_editor::ProfileAppearance,
}
impl ProtocolSessionOwner {
    #[cfg(feature = "ssh-use")]
    pub(crate) fn with_appearance(
        mut self,
        appearance: pulse_store::profile_editor::ProfileAppearance,
    ) -> Self {
        self.appearance = appearance;
        self
    }
    pub(crate) fn appearance(&self) -> &pulse_store::profile_editor::ProfileAppearance {
        &self.appearance
    }
    /// Backend identity only, never a page capability.
    pub fn id(&self) -> SessionId {
        self.id
    }
    /// Trusted adapter access; frontend use still requires a current registry binding.
    pub fn handle(&self) -> &SessionHandle {
        &self.handle
    }
    /// Require exact registry provenance, not an equal numeric session identifier.
    pub fn is_managed_by(&self, registry: &SessionRegistry) -> bool {
        self.registry.same_instance(registry)
    }
    /// Key-free observation only; does not own native work or authorize input/reclamation.
    pub fn observe_cleanup(&self) -> watch::Receiver<CleanupState> {
        self.state.subscribe()
    }
    /// Await natural completion after final display/EOF consumers finish, then retire capacity.
    /// Cancelling this waiter invokes Drop cleanup without dropping the native supervisor.
    pub async fn finish(mut self) -> Result<()> {
        let result = match self.completion.as_mut() {
            Some(completion) => completion
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)
                .and_then(|result| result),
            None => Err(ProtocolSessionError::NativeUnknown),
        };
        self.completion.take();
        let result = result.and_then(|()| self.registry.reap_closed(self.id).map_err(Into::into));
        if result.is_err() {
            self.handle.close();
        }
        self.state.send_replace(if result.is_ok() {
            CleanupState::Reaped
        } else {
            CleanupState::Failed
        });
        result
    }
    /// Explicit close. Dropping/cancelling the cleanup waiter does not abort actual cleanup.
    pub fn cancel(mut self) -> ProtocolSessionCleanup {
        self.handle.close();
        let completion = self.completion.take();
        ProtocolSessionCleanup {
            worker: self.spawn_cleanup(completion),
        }
    }
    fn spawn_cleanup(&self, completion: Option<Completion>) -> JoinHandle<Result<()>> {
        self.state.send_replace(CleanupState::Closing);
        let registry = self.registry.clone();
        let handle = self.handle.clone();
        let id = self.id;
        let state = self.state.clone();
        self.runtime.spawn(async move {
            let result = match completion {
                Some(completion) => completion
                    .await
                    .map_err(|_| ProtocolSessionError::NativeUnknown)
                    .and_then(|result| result),
                None => Err(ProtocolSessionError::NativeUnknown),
            }
            .and_then(|()| registry.reap_closed(id).map_err(Into::into));
            if result.is_err() {
                handle.close();
            }
            state.send_replace(if result.is_ok() {
                CleanupState::Reaped
            } else {
                CleanupState::Failed
            });
            result
        })
    }
}
impl Drop for ProtocolSessionOwner {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(completion) = self.completion.take() {
            drop(self.spawn_cleanup(Some(completion)));
        }
    }
}
/// Cleanup observation only; dropping it does not abort the root-owned cleanup task.
#[must_use]
pub struct ProtocolSessionCleanup {
    worker: JoinHandle<Result<()>>,
}
impl ProtocolSessionCleanup {
    /// Wait for actual native completion and same-source registry reclamation.
    pub async fn join(self) -> Result<()> {
        self.worker
            .await
            .map_err(|_| ProtocolSessionError::NativeUnknown)?
    }
}

#[cfg(test)]
#[path = "protocol_session_owner_tests.rs"]
mod tests;
