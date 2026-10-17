//! One pre-admitted blocking job; no unbounded work queue or implicit key activation.
use crate::vault_kdf::{KdfError, KdfJob, admit_validation_job, validate_password};
use crate::vault_lifecycle::{LifecycleError, UnlockCandidate, UnlockPermit, VaultKeyLifecycle};
use crate::vault_wrap::{AuthenticatedDek, ValidationWrappedDek, WrapError, recover_admitted};
use tokio::{runtime::Handle, sync::oneshot, task::JoinHandle};
use zeroize::Zeroizing;
/// Owned bounded password, no Debug/Clone/serde; transfers to the blocking closure.
/// Caller remains responsible for copies created before this ownership transfer.
/// ```compile_fail
/// fn dump(p:pulse_store::vault_unlock::PasswordInput) {println!("{p:?}");}
/// ```
pub struct PasswordInput {
    pub(crate) bytes: Zeroizing<Vec<u8>>,
}
impl PasswordInput {
    /// Invalid supplied buffers are also zeroized on return.
    pub fn new(bytes: Vec<u8>) -> Result<Self, KdfError> {
        let bytes = Zeroizing::new(bytes);
        validate_password(&bytes)?;
        Ok(Self { bytes })
    }
}
/// No password/metadata contents are included in these errors.
#[derive(Debug, thiserror::Error)]
pub enum UnlockError {
    /// No active Tokio runtime; no job/permit is started.
    #[error("vault.unlock_runtime_unavailable")]
    Runtime,
    /// A worker ended without a result; no auto retry or replacement key.
    #[error("vault.unlock_worker_ended")]
    Worker,
    /// Admission/entropy/derivation failed.
    #[error(transparent)]
    Kdf(#[from] KdfError),
    /// Expected scope/epoch was revoked.
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
    /// DEK identity/AEAD authentication failed.
    #[error(transparent)]
    Wrap(#[from] WrapError),
}
/// Pending ownership. Dropping this or the consuming `ready()` future cancels.
/// The CPU job remains charged until its actual closure exits, even if detached.
pub struct ValidationUnlockRequest {
    source_envelope: Option<Vec<u8>>,
    permit: UnlockPermit,
    receiver: oneshot::Receiver<Result<UnlockCandidate, UnlockError>>,
    task: JoinHandle<()>,
}
/// Cancellation observation only, no key or abort/early-quota-release method.
pub struct CancelledUnlock {
    task: JoinHandle<()>,
}
impl CancelledUnlock {
    /// Finite worker observation; dropping this handle does not cancel the CPU job.
    pub async fn join(self) -> Result<(), UnlockError> {
        self.task.await.map_err(|_| UnlockError::Worker)
    }
}
/// A joined, authenticated result is still not active until explicit confirmation.
pub struct ReadyUnlock {
    source_envelope: Option<Vec<u8>>,
    permit: UnlockPermit,
    candidate: UnlockCandidate,
}
impl ValidationUnlockRequest {
    /// Register explicit epoch cancellation with a trusted page/application owner before work completes.
    pub fn revocation(&self) -> crate::vault_lifecycle::UnlockRevocation {
        self.permit.revocation()
    }
    /// Consuming async wait: timeout/drop cancels the permit and owned receiver.
    /// Awaits both delivery and worker join; errors never activate a key.
    pub async fn ready(self) -> Result<ReadyUnlock, UnlockError> {
        let result = self.receiver.await;
        let joined = self.task.await;
        joined.map_err(|_| UnlockError::Worker)?;
        let candidate = result.map_err(|_| UnlockError::Worker)??;
        Ok(ReadyUnlock {
            source_envelope: self.source_envelope,
            permit: self.permit,
            candidate,
        })
    }
    /// Cancel owned request; never abort a running CPU closure or free its quota early.
    pub fn cancel(self) -> CancelledUnlock {
        drop(self.permit);
        drop(self.receiver);
        CancelledUnlock { task: self.task }
    }
}
impl ReadyUnlock {
    /// Key-free same-epoch cancellation for trusted native ownership handoff.
    pub fn revocation(&self) -> crate::vault_lifecycle::UnlockRevocation {
        self.permit.revocation()
    }
    pub(crate) fn confirmation_cancel(&self) -> crate::vault_lifecycle::UnlockConfirmationCancel {
        self.permit.confirmation_cancel()
    }
    pub(crate) fn confirm_bound(
        mut self,
        scope: &VaultKeyLifecycle,
        envelope: &[u8],
    ) -> Result<crate::vault_lifecycle::UnlockActivation, LifecycleError> {
        if self.source_envelope.as_deref() != Some(envelope) {
            return Err(LifecycleError::Stale);
        }
        scope.confirm_guarded(&mut self.permit, &mut self.candidate)
    }
    #[cfg(test)]
    pub(crate) fn fixture(
        scope: &VaultKeyLifecycle,
        envelope: Vec<u8>,
    ) -> Result<Self, LifecycleError> {
        let mut permit = scope.begin()?;
        let worker = permit.worker()?;
        let proof =
            AuthenticatedDek::fixture(scope.expected_id()?).map_err(|_| LifecycleError::Closed)?;
        let candidate = worker.finish(proof)?;
        Ok(Self {
            permit,
            candidate,
            source_envelope: Some(envelope),
        })
    }
    /// Only a currently authorized backend adapter should call this.
    /// Foreign/old confirmation is rejected without consuming this receipt.
    pub fn confirm(&mut self, scope: &VaultKeyLifecycle) -> Result<(), UnlockError> {
        scope.confirm(&mut self.permit, &mut self.candidate)?;
        Ok(())
    }
}
/// Trusted Rust-only validation API. No caller-selected cost or plaintext IPC.
/// Validate metadata and reserve the global quota *before* scheduling one closure.
pub fn begin_validation_unlock(
    scope: &VaultKeyLifecycle,
    password: PasswordInput,
    envelope: ValidationWrappedDek,
) -> Result<ValidationUnlockRequest, UnlockError> {
    begin_validation_unlock_authorized(scope, password, envelope, std::sync::Arc::new(|| true))
}
/// Native-owned validation unlock; predicate retains actual operation resources until CPU exit.
/// Reject retired authorization before admission/computation/publication, no implicit activation.
pub fn begin_validation_unlock_authorized(
    scope: &VaultKeyLifecycle,
    password: PasswordInput,
    envelope: ValidationWrappedDek,
    authorize: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<ValidationUnlockRequest, UnlockError> {
    let runtime = Handle::try_current().map_err(|_| UnlockError::Runtime)?;
    if !authorize() {
        return Err(LifecycleError::Stale.into());
    }
    let id = scope.expected_id()?;
    envelope.validate_for(id)?;
    let source_envelope = envelope.encode_ciphertext()?;
    let job = admit_validation_job()?;
    let retention = authorize.clone();
    let mut request = dispatch_retained(&runtime, scope, job, Some(retention), move |job| {
        if !authorize() {
            return Err(LifecycleError::Stale.into());
        }
        let key = recover_admitted(&password.bytes, id, &envelope, job)?;
        if !authorize() {
            return Err(LifecycleError::Stale.into());
        }
        Ok(key)
    })?;
    request.source_envelope = Some(source_envelope);
    Ok(request)
}
#[cfg(test)]
fn dispatch<F>(
    runtime: &Handle,
    scope: &VaultKeyLifecycle,
    job: KdfJob,
    compute: F,
) -> Result<ValidationUnlockRequest, UnlockError>
where
    F: FnOnce(&KdfJob) -> Result<AuthenticatedDek, UnlockError> + Send + 'static,
{
    dispatch_retained(runtime, scope, job, None, compute)
}
fn dispatch_retained<F>(
    runtime: &Handle,
    scope: &VaultKeyLifecycle,
    job: KdfJob,
    retention: Option<std::sync::Arc<dyn Fn() -> bool + Send + Sync>>,
    compute: F,
) -> Result<ValidationUnlockRequest, UnlockError>
where
    F: FnOnce(&KdfJob) -> Result<AuthenticatedDek, UnlockError> + Send + 'static,
{
    let mut permit = scope.begin()?;
    let worker = permit.worker()?;
    let (sender, receiver) = oneshot::channel();
    let task = runtime.spawn_blocking(move || {
        let _retention = retention;
        let job = job;
        if sender.is_closed() {
            return;
        }
        let result = (|| {
            worker.ensure_current()?;
            let key = compute(&job)?;
            Ok(worker.finish(key)?)
        })();
        // Failed delivery drops the key-free receipt and revokes stored candidate key.
        let _undelivered = sender.send(result);
        // `job` lives through authentication, publication/send and closure exit.
    });
    Ok(ValidationUnlockRequest {
        source_envelope: None,
        permit,
        receiver,
        task,
    })
}
#[cfg(test)]
#[path = "vault_unlock_tests.rs"]
mod tests;
