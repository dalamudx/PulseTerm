//! Root-application ownership of an already authorized and registered local session.
//! Keep this owner in application/session state, NEVER in document or view state.
//! No path, Profile selection, input authority or frontend command is introduced.
use crate::{
    LocalSessionReceipt,
    local_launch::{LocalStartError, RegisteredLocalSession},
    registry::{SessionId, SessionRegistry},
};
use pulse_term::session::SessionHandle;
use tokio::{
    runtime::Handle,
    sync::{oneshot, watch},
    task::JoinHandle,
};

type Completion =
    oneshot::Receiver<std::result::Result<LocalSessionReceipt, crate::LocalSessionError>>;
type Result<T> = std::result::Result<T, LocalStartError>;

/// Content-free observation only: none of these states grants page/session permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupState {
    /// Root owner retains the session; navigation must not drop it.
    Owned,
    /// Close requested, awaiting actual actor AND native-worker completion.
    Closing,
    /// Native completion was received and registry capacity retired.
    Reaped,
    /// Completion or retirement failed; do not interpret this as released capacity.
    Failed,
}

/// Root-owned session; Drop requests close and detaches cleanup, not the native worker.
/// The registry and runtime MUST be the ones used for the admitted launch. The runtime
/// must remain alive through cleanup; process/runtime teardown and descendant trees
/// are not guaranteed here. Unknown completion retains registry capacity fail closed.
#[must_use]
pub struct LocalSessionOwner {
    registry: SessionRegistry,
    id: SessionId,
    handle: SessionHandle,
    completion: Option<Completion>,
    runtime: Handle,
    state: watch::Sender<CleanupState>,
}
impl LocalSessionOwner {
    /// Adopt at the root runtime immediately after PendingLocalLaunch::wait succeeds.
    /// This trusted Rust operation is not authorization and must not accept page data.
    pub fn adopt(runtime: Handle, registered: RegisteredLocalSession) -> Self {
        let (state, _) = watch::channel(CleanupState::Owned);
        Self {
            registry: registered.registry,
            runtime,
            id: registered.id,
            handle: registered.handle,
            completion: Some(registered.completion),
            state,
        }
    }
    /// Composition identity only, NOT page or view permission. A root collection must
    /// reject foreign owners even when their numeric session IDs happen to coincide.
    pub fn is_managed_by(&self, registry: &SessionRegistry) -> bool {
        self.registry.same_instance(registry)
    }
    /// Backend identity, not a capability. Serialize separately as a decimal string.
    pub fn id(&self) -> SessionId {
        self.id
    }
    /// Trusted adapter access; all page actions still require registry authorization.
    pub fn handle(&self) -> &SessionHandle {
        &self.handle
    }
    /// Observers do not retain this owner or prevent its Drop cleanup.
    pub fn observe_cleanup(&self) -> watch::Receiver<CleanupState> {
        self.state.subscribe()
    }
    /// Await natural completion and retire. Call only once display/EOF consumers have
    /// finished using the registered session. Cancelling this future closes and cleans
    /// up via Drop, retaining the completion receiver until the native supervisor joins.
    pub async fn finish(mut self) -> Result<LocalSessionReceipt> {
        // Borrow the receiver so cancelling this await leaves it available to Drop.
        let result = match self.completion.as_mut() {
            Some(completion) => completion
                .await
                .map_err(|_| LocalStartError::Worker)
                .and_then(|r| r.map_err(LocalStartError::Native)),
            None => Err(LocalStartError::Worker),
        };
        self.completion.take();
        let result = result.and_then(|receipt| {
            self.registry.reap_closed(self.id)?;
            Ok(receipt)
        });
        self.state.send_replace(if result.is_ok() {
            CleanupState::Reaped
        } else {
            CleanupState::Failed
        });
        result
    }
    /// Explicit root shutdown. Cancelling/dropping its waiter does not abort cleanup.
    pub fn cancel(mut self) -> LocalSessionCleanup {
        self.handle.close();
        let completion = self.completion.take();
        LocalSessionCleanup {
            worker: self.spawn_cleanup(completion),
        }
    }
    fn spawn_cleanup(
        &self,
        completion: Option<Completion>,
    ) -> JoinHandle<Result<LocalSessionReceipt>> {
        self.state.send_replace(CleanupState::Closing);
        let registry = self.registry.clone();
        let handle = self.handle.clone();
        let id = self.id;
        let state = self.state.clone();
        self.runtime.spawn(async move {
            let result = match completion {
                Some(completion) => completion
                    .await
                    .map_err(|_| LocalStartError::Worker)
                    .and_then(|r| r.map_err(LocalStartError::Native)),
                None => Err(LocalStartError::Worker),
            };
            let result = result.and_then(|receipt| {
                // Only a successful receipt proves all native workers joined. Actor
                // lifecycle alone, a closed channel or a timeout is not that proof.
                registry.reap_closed(id)?;
                Ok(receipt)
            });
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
impl Drop for LocalSessionOwner {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(completion) = self.completion.take() {
            // Dropping a JoinHandle detaches it; never abort actual cleanup here.
            drop(self.spawn_cleanup(Some(completion)));
        }
    }
}

/// Cleanup observation; not an actor/native task and not permission to reap early.
#[must_use]
pub struct LocalSessionCleanup {
    worker: JoinHandle<Result<LocalSessionReceipt>>,
}
impl LocalSessionCleanup {
    /// Await actual completion and registry retirement, or a fail-closed error.
    pub async fn join(self) -> Result<LocalSessionReceipt> {
        self.worker.await.map_err(|_| LocalStartError::Worker)?
    }
}

#[cfg(all(test, unix))]
#[path = "local_session_owner_tests.rs"]
mod tests;
