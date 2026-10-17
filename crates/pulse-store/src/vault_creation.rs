//! Encrypted-only initial Vault creation; never implicit activation or replacement.
#[cfg(test)]
#[path = "vault_creation_tests.rs"]
mod tests;
use crate::vault_kdf::{KdfError, KdfJob, admit_validation_job};
use crate::vault_unlock::PasswordInput;
use crate::vault_wrap::{ValidationWrappedDek, WrapError, create_admitted};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::{runtime::Handle, sync::oneshot, task::JoinHandle};
use uuid::Uuid;
/// Static creation errors never contain passwords, keys or encrypted data.
#[derive(Debug, thiserror::Error)]
pub enum CreationError {
    /// Missing runtime before CPU admission.
    #[error("vault.creation_runtime")]
    Runtime,
    /// Nil or revoked backend identity/scope before admission or after work.
    #[error("vault.creation_cancelled")]
    Cancelled,
    /// Native computation ended without a joined result; never auto retry.
    #[error("vault.creation_worker")]
    Worker,
    /// Shared global KDF admission failure.
    #[error(transparent)]
    Kdf(#[from] KdfError),
    /// Entropy/derivation/wrapping failure; no fallback key.
    #[error(transparent)]
    Wrap(#[from] WrapError),
}
type Authorize = Arc<dyn Fn() -> bool + Send + Sync>;
struct Owner {
    live: Arc<AtomicBool>,
    authorize: Authorize,
}
impl Owner {
    fn current(&self) -> bool {
        current(&self.live, &self.authorize)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.live.store(false, Ordering::Release);
    }
}
fn current(live: &AtomicBool, authorize: &Authorize) -> bool {
    live.load(Ordering::Acquire) && authorize()
}
/// Owned request; drop/ready-future cancellation revokes results, not active CPU quota.
/// No Debug/Clone/serde and no clear key can be obtained from this request.
pub struct ValidationCreationRequest {
    owner: Option<Owner>,
    receiver: Option<oneshot::Receiver<Result<ValidationWrappedDek, CreationError>>>,
    task: Option<JoinHandle<()>>,
}
/// Encrypted-only result. Dropping it revokes its derived persistence predicate.
/// Persistence requires original empty store snapshot and current native scope separately.
pub struct ReadyCreation {
    owner: Owner,
    envelope: ValidationWrappedDek,
}
impl ValidationCreationRequest {
    /// Immediately revoke this operation, retaining CPU quota until actual closure exit.
    pub fn cancel(&self) {
        if let Some(owner) = &self.owner {
            owner.live.store(false, Ordering::Release);
        }
    }
    /// Wait for delivery and actual native join. A successful result remains locked.
    pub async fn ready(mut self) -> Result<ReadyCreation, CreationError> {
        let result = self.receiver.take().ok_or(CreationError::Worker)?.await;
        self.task
            .take()
            .ok_or(CreationError::Worker)?
            .await
            .map_err(|_| CreationError::Worker)?;
        let envelope = result.map_err(|_| CreationError::Worker)??;
        let owner = self.owner.take().ok_or(CreationError::Worker)?;
        if !owner.current() {
            return Err(CreationError::Cancelled);
        }
        Ok(ReadyCreation { owner, envelope })
    }
    /// Cancel and await real worker exit; cancellation cannot free a running permit early.
    pub async fn cancel_and_join(mut self) -> Result<(), CreationError> {
        self.cancel();
        if let Some(mut receiver) = self.receiver.take() {
            receiver.close();
        }
        self.task
            .take()
            .ok_or(CreationError::Worker)?
            .await
            .map_err(|_| CreationError::Worker)
    }
}
impl Drop for ValidationCreationRequest {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl ReadyCreation {
    /// Ciphertext copy only after current native authorization; this does not enable a key.
    pub fn envelope(&self) -> Result<ValidationWrappedDek, CreationError> {
        if !self.owner.current() {
            return Err(CreationError::Cancelled);
        }
        Ok(self.envelope.clone())
    }
    /// Derived SQL predicate stays revocable when this receipt is dropped.
    /// Caller must still retain actual-I/O credit and validate original store snapshot.
    pub fn persistence_authorization(&self) -> Authorize {
        let live = self.owner.live.clone();
        let authorize = self.owner.authorize.clone();
        Arc::new(move || current(&live, &authorize))
    }
}
/// Trusted backend creation with fixed existing KDF profile; no password IPC or cost selection.
/// Backend supplies a fresh identity and authorization retaining operation credit until exit.
/// Only encrypted envelope is returned; temporary DEK/KEK drop inside blocking computation.
pub fn begin_validation_creation(
    password: PasswordInput,
    id: Uuid,
    authorize: Authorize,
) -> Result<ValidationCreationRequest, CreationError> {
    let runtime = Handle::try_current().map_err(|_| CreationError::Runtime)?;
    if id.is_nil() || !authorize() {
        return Err(CreationError::Cancelled);
    }
    let job = admit_validation_job()?;
    dispatch(&runtime, job, authorize, move |job| {
        let (envelope, cipher) = create_admitted(&password.bytes, id, job)?;
        drop(cipher);
        Ok(envelope)
    })
}
fn dispatch<F>(
    runtime: &Handle,
    job: KdfJob,
    authorize: Authorize,
    compute: F,
) -> Result<ValidationCreationRequest, CreationError>
where
    F: FnOnce(&KdfJob) -> Result<ValidationWrappedDek, CreationError> + Send + 'static,
{
    let owner = Owner {
        live: Arc::new(AtomicBool::new(true)),
        authorize,
    };
    let live = owner.live.clone();
    let authorize = owner.authorize.clone();
    let (sender, receiver) = oneshot::channel();
    let task = runtime.spawn_blocking(move || {
        let job = job;
        if sender.is_closed() || !current(&live, &authorize) {
            return;
        }
        let result = (|| {
            let envelope = compute(&job)?;
            if !current(&live, &authorize) {
                return Err(CreationError::Cancelled);
            }
            Ok(envelope)
        })();
        let _delivery = sender.send(result);
    });
    Ok(ValidationCreationRequest {
        owner: Some(owner),
        receiver: Some(receiver),
        task: Some(task),
    })
}
