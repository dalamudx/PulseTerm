//! Two-phase backend memory key handoff. No password CPU scheduling or IPC.
//! Adapters must independently authorize users/windows; tickets are backend-only.
use crate::vault_crypto::{CryptoError, SealedSecret, SecretContext, VaultCipher};
use crate::vault_wrap::AuthenticatedDek;
use std::sync::{
    Arc, Mutex, MutexGuard, Weak,
    atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;
/// No key or input is included in errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LifecycleError {
    /// Scope closed/destroyed/poisoned; fails closed.
    #[error("vault.scope_closed")]
    Closed,
    /// Only one pending handoff in this scope.
    #[error("vault.unlock_busy")]
    Busy,
    /// A different owner, epoch, canceled permit or already-completed permit.
    #[error("vault.unlock_stale")]
    Stale,
    /// Backend monotonic epochs never wrap or reset.
    #[error("vault.epoch_exhausted")]
    Exhausted,
    /// Expected identity is missing or does not match the authenticated result.
    #[error("vault.key_identity")]
    Identity,
    /// Key use is unavailable before explicit confirmation or after lock.
    #[error("vault.key_locked")]
    Locked,
}
/// Availability only; not a permissions/UI-authentication assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStatus {
    /// No active DEK.
    Locked,
    /// A computation/candidate still needs confirmation.
    Pending,
    /// A current key was explicitly confirmed.
    Available,
}
/// Keyless exact-controller/epoch evidence, not SecretRef/target or frontend authorization.
/// ```compile_fail
/// fn dump(e:pulse_store::vault_lifecycle::AvailableKeyEpoch){println!("{e:?}");}
/// ```
/// ```compile_fail
/// fn export(e:pulse_store::vault_lifecycle::AvailableKeyEpoch){let _=serde_json::to_string(&e);}
/// ```
#[must_use]
#[derive(Clone)]
pub struct AvailableKeyEpoch {
    owner: Weak<Mutex<State>>,
    epoch: u64,
}
impl AvailableKeyEpoch {
    /// Key-free boolean check of this original weak controller only, without retaining it in a resource guard.
    /// Owner destruction/lock/reunlock cannot upgrade it. Not a use grant; crypto must validate under its key lock.
    pub fn is_original_current(&self) -> Result<bool, LifecycleError> {
        let Some(owner) = self.owner.upgrade() else {
            return Ok(false);
        };
        let state = guard(&owner)?;
        Ok(state.epoch == self.epoch
            && state.key.is_some()
            && state.pending.is_none()
            && state.candidate_key.is_none())
    }
    /// Availability check only; not target authorization. Crypto use must revalidate in the same key lock.
    pub fn is_current_for(&self, scope: &VaultKeyLifecycle) -> Result<bool, LifecycleError> {
        match scope.guard_epoch(self) {
            Ok(_) => Ok(true),
            Err(LifecycleError::Stale | LifecycleError::Locked) => Ok(false),
            Err(error) => Err(error),
        }
    }
}
impl VaultKeyLifecycle {
    /// Capture only an explicitly available key. Has-value or pending candidates are insufficient.
    pub fn available_epoch(&self) -> Result<AvailableKeyEpoch, LifecycleError> {
        let state = guard(&self.shared)?;
        if state.key.is_none() || state.pending.is_some() || state.candidate_key.is_some() {
            return Err(LifecycleError::Locked);
        }
        Ok(AvailableKeyEpoch {
            owner: Arc::downgrade(&self.shared),
            epoch: state.epoch,
        })
    }
    fn guard_epoch(
        &self,
        epoch: &AvailableKeyEpoch,
    ) -> Result<MutexGuard<'_, State>, LifecycleError> {
        if !Weak::ptr_eq(&epoch.owner, &Arc::downgrade(&self.shared)) {
            return Err(LifecycleError::Stale);
        }
        let state = guard(&self.shared)?;
        if state.epoch != epoch.epoch {
            return Err(LifecycleError::Stale);
        }
        if state.key.is_none() || state.pending.is_some() || state.candidate_key.is_some() {
            return Err(LifecycleError::Locked);
        }
        Ok(state)
    }
    /// Trusted adapter must also authorize exact stored identity/reference/role/source and operation.
    /// Epoch validation and encryption share one short key lock: later unlock cannot upgrade a request.
    pub fn seal_in_epoch(
        &self,
        epoch: &AvailableKeyEpoch,
        context: &SecretContext,
        plain: &[u8],
    ) -> Result<SealedSecret, KeyUseError> {
        let state = self.guard_epoch(epoch)?;
        Ok(state
            .key
            .as_ref()
            .ok_or(LifecycleError::Locked)?
            .seal(context, plain)?)
    }
    /// Rust-only adapter operation. Never register a plaintext IPC; exact target authorization remains independent.
    pub fn open_for_adapter_in_epoch(
        &self,
        epoch: &AvailableKeyEpoch,
        context: &SecretContext,
        sealed: &SealedSecret,
    ) -> Result<crate::vault_crypto::SecretBytes, KeyUseError> {
        let state = self.guard_epoch(epoch)?;
        Ok(state
            .key
            .as_ref()
            .ok_or(LifecycleError::Locked)?
            .open(context, sealed)?)
    }
}

struct Token {
    epoch: u64,
    live: AtomicBool,
}
struct State {
    id: Uuid,
    epoch: u64,
    closed: bool,
    pending: Option<Arc<Token>>,
    key: Option<VaultCipher>,
    candidate_key: Option<AuthenticatedDek>,
}
impl State {
    fn retire(&mut self) {
        self.key = None;
        self.candidate_key = None;
        if let Some(token) = self.pending.take() {
            token.live.store(false, Ordering::Release);
        }
    }
    fn advance(&mut self) -> Result<u64, LifecycleError> {
        if let Some(next) = self.epoch.checked_add(1) {
            self.epoch = next;
            Ok(next)
        } else {
            self.retire();
            self.closed = true;
            Err(LifecycleError::Exhausted)
        }
    }
    fn current(&self, token: &Arc<Token>) -> bool {
        !self.closed
            && token.live.load(Ordering::Acquire)
            && self.epoch == token.epoch
            && self
                .pending
                .as_ref()
                .is_some_and(|pending| Arc::ptr_eq(pending, token))
    }
}
fn guard(shared: &Arc<Mutex<State>>) -> Result<MutexGuard<'_, State>, LifecycleError> {
    match shared.lock() {
        Ok(state) if !state.closed => Ok(state),
        Ok(_) => Err(LifecycleError::Closed),
        Err(poison) => {
            let mut state = poison.into_inner();
            state.retire();
            state.closed = true;
            Err(LifecycleError::Closed)
        }
    }
}
/// Scope clones share one epoch and owned key; worker/permit handles are weak.
#[derive(Clone)]
pub struct VaultKeyLifecycle {
    shared: Arc<Mutex<State>>,
}
/// RAII cancellation. Neither permits nor candidates are Clone/Debug/serde.
/// ```compile_fail
/// fn duplicate(p: pulse_store::vault_lifecycle::UnlockPermit) { let _copy = p.clone(); }
/// ```
pub struct UnlockPermit {
    owner: Weak<Mutex<State>>,
    token: Arc<Token>,
    issued: bool,
    confirmed: bool,
}
/// Move once into the supervised computation, without keeping the scope alive.
pub struct UnlockWorker {
    owner: Weak<Mutex<State>>,
    token: Arc<Token>,
}
/// Key-free receipt. The revocable scope owns every stored candidate key.
pub struct UnlockCandidate {
    owner: Weak<Mutex<State>>,
    token: Arc<Token>,
    confirmed: bool,
}
/// Key-free, epoch-specific publication receipt; lost delivery revokes only its activation.
pub(crate) struct UnlockActivation {
    owner: Weak<Mutex<State>>,
    epoch: u64,
    accepted: bool,
}
impl UnlockActivation {
    pub(crate) fn accept(mut self) {
        self.accepted = true;
    }
}
impl Drop for UnlockActivation {
    fn drop(&mut self) {
        if self.accepted {
            return;
        }
        if let Some(owner) = self.owner.upgrade() {
            let mut state = owner.lock().unwrap_or_else(|p| p.into_inner());
            if state.epoch == self.epoch {
                state.key = None;
            }
        }
    }
}
impl VaultKeyLifecycle {
    /// Trusted expected Vault identity; no caller-controlled window/generation strings.
    pub fn new(id: Uuid) -> Result<Self, LifecycleError> {
        if id.is_nil() {
            return Err(LifecycleError::Identity);
        }
        Ok(Self {
            shared: Arc::new(Mutex::new(State {
                id,
                epoch: 0,
                closed: false,
                pending: None,
                key: None,
                candidate_key: None,
            })),
        })
    }
    pub(crate) fn expected_id(&self) -> Result<Uuid, LifecycleError> {
        Ok(guard(&self.shared)?.id)
    }
    /// Observe only availability, never key material.
    pub fn status(&self) -> Result<KeyStatus, LifecycleError> {
        let state = guard(&self.shared)?;
        Ok(if state.key.is_some() {
            KeyStatus::Available
        } else if state.pending.is_some() {
            KeyStatus::Pending
        } else {
            KeyStatus::Locked
        })
    }
    /// Create one pending epoch. No CPU work or allocation is scheduled here.
    pub fn begin(&self) -> Result<UnlockPermit, LifecycleError> {
        let mut state = guard(&self.shared)?;
        if state.pending.is_some() {
            return Err(LifecycleError::Busy);
        }
        if state.key.is_some() {
            return Err(LifecycleError::Stale);
        }
        let token = Arc::new(Token {
            epoch: state.advance()?,
            live: AtomicBool::new(true),
        });
        state.pending = Some(token.clone());
        Ok(UnlockPermit {
            owner: Arc::downgrade(&self.shared),
            token,
            issued: false,
            confirmed: false,
        })
    }
    /// Confirm only this still-live backend permit/candidate pair. No implicit unlock.
    pub fn confirm(
        &self,
        permit: &mut UnlockPermit,
        candidate: &mut UnlockCandidate,
    ) -> Result<(), LifecycleError> {
        let owner = Arc::downgrade(&self.shared);
        if !owner.ptr_eq(&permit.owner)
            || !owner.ptr_eq(&candidate.owner)
            || !Arc::ptr_eq(&permit.token, &candidate.token)
            || permit.confirmed
        {
            return Err(LifecycleError::Stale);
        }
        let mut state = guard(&self.shared)?;
        if !state.current(&permit.token) {
            return Err(LifecycleError::Stale);
        }
        let key = state.candidate_key.take().ok_or(LifecycleError::Stale)?;
        state.key = Some(key.into_cipher());
        candidate.confirmed = true;
        state.pending = None;
        permit.confirmed = true;
        permit.token.live.store(false, Ordering::Release);
        Ok(())
    }
    pub(crate) fn confirm_guarded(
        &self,
        permit: &mut UnlockPermit,
        candidate: &mut UnlockCandidate,
    ) -> Result<UnlockActivation, LifecycleError> {
        self.confirm(permit, candidate)?;
        Ok(UnlockActivation {
            owner: permit.owner.clone(),
            epoch: permit.token.epoch,
            accepted: false,
        })
    }
    /// Clear owned key/pending candidates even when epoch exhaustion closes the scope.
    /// Cannot retract a credential already released to an authorized transport.
    pub fn lock(&self) -> Result<(), LifecycleError> {
        let mut state = guard(&self.shared)?;
        state.retire();
        state.advance()?;
        Ok(())
    }
    /// Permanently disable this scope; clones/late workers cannot revive it.
    pub fn close(&self) -> Result<(), LifecycleError> {
        let mut state = guard(&self.shared)?;
        state.retire();
        state.closed = true;
        Ok(())
    }
    /// Bounded internal encryption, not a frontend permission or plaintext command.
    pub fn seal(&self, context: &SecretContext, plain: &[u8]) -> Result<SealedSecret, KeyUseError> {
        let state = guard(&self.shared)?;
        Ok(state
            .key
            .as_ref()
            .ok_or(LifecycleError::Locked)?
            .seal(context, plain)?)
    }
}
impl VaultKeyLifecycle {
    /// Trusted backend authentication adapter only. Never register a plaintext IPC.
    /// The adapter must authorize the target SecretRef independently of key availability.
    pub fn open_for_adapter(
        &self,
        context: &SecretContext,
        sealed: &SealedSecret,
    ) -> Result<crate::vault_crypto::SecretBytes, KeyUseError> {
        let state = guard(&self.shared)?;
        Ok(state
            .key
            .as_ref()
            .ok_or(LifecycleError::Locked)?
            .open(context, sealed)?)
    }
}
#[cfg(test)]
#[path = "vault_key_epoch_tests.rs"]
mod key_epoch_tests;
/// Static internal-use errors.
#[derive(Debug, thiserror::Error)]
pub enum KeyUseError {
    /// Scope/key unavailable.
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
    /// AEAD failed.
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}
/// Caller-side weak cancellation; neither key material nor I/O reservation is retained.
pub(crate) struct UnlockConfirmationCancel {
    owner: Weak<Mutex<State>>,
    epoch: u64,
    accepted: bool,
}
impl UnlockConfirmationCancel {
    pub(crate) fn accept(mut self) {
        self.accepted = true;
    }
}
impl Drop for UnlockConfirmationCancel {
    fn drop(&mut self) {
        if self.accepted {
            return;
        }
        if let Some(owner) = self.owner.upgrade() {
            let mut state = owner.lock().unwrap_or_else(|p| p.into_inner());
            if state.epoch == self.epoch {
                state.retire();
            }
        }
    }
}
/// Cloneable key-free handle that can revoke only its issued unlock epoch.
/// Dropping a handle does not cancel; the native owner explicitly invokes cancel on retirement.
#[derive(Clone)]
pub struct UnlockRevocation {
    owner: Weak<Mutex<State>>,
    token: Arc<Token>,
}
impl UnlockRevocation {
    /// Backend controller identity check without exposing an epoch, key or nonce.
    pub fn belongs_to(&self, scope: &VaultKeyLifecycle) -> bool {
        Weak::ptr_eq(&self.owner, &Arc::downgrade(&scope.shared))
    }
    /// Clear pending or already-enabled material for this epoch, never a subsequent epoch.
    pub fn cancel(&self) {
        self.token.live.store(false, Ordering::Release);
        if let Some(owner) = self.owner.upgrade() {
            let mut state = owner.lock().unwrap_or_else(|p| p.into_inner());
            if state.epoch == self.token.epoch {
                state.retire();
            }
        }
    }
}

impl UnlockPermit {
    /// Key-free explicit cancellation for this epoch, including its late activation.
    pub fn revocation(&self) -> UnlockRevocation {
        UnlockRevocation {
            owner: self.owner.clone(),
            token: self.token.clone(),
        }
    }
    pub(crate) fn confirmation_cancel(&self) -> UnlockConfirmationCancel {
        UnlockConfirmationCancel {
            owner: self.owner.clone(),
            epoch: self.token.epoch,
            accepted: false,
        }
    }
    /// Issue exactly one worker token; no parallel duplicate computations.
    pub fn worker(&mut self) -> Result<UnlockWorker, LifecycleError> {
        let shared = self.owner.upgrade().ok_or(LifecycleError::Closed)?;
        if !guard(&shared)?.current(&self.token) {
            return Err(LifecycleError::Stale);
        }
        if self.issued {
            return Err(LifecycleError::Busy);
        }
        self.issued = true;
        Ok(UnlockWorker {
            owner: self.owner.clone(),
            token: self.token.clone(),
        })
    }
}
impl Drop for UnlockPermit {
    fn drop(&mut self) {
        if self.confirmed {
            return;
        }
        self.token.live.store(false, Ordering::Release);
        if let Some(shared) = self.owner.upgrade()
            && let Ok(mut state) = guard(&shared)
            && state
                .pending
                .as_ref()
                .is_some_and(|token| Arc::ptr_eq(token, &self.token))
        {
            state.pending = None;
            state.candidate_key = None;
        }
    }
}
impl Drop for UnlockCandidate {
    fn drop(&mut self) {
        if self.confirmed {
            return;
        }
        self.token.live.store(false, Ordering::Release);
        if let Some(shared) = self.owner.upgrade()
            && let Ok(mut state) = guard(&shared)
            && state
                .pending
                .as_ref()
                .is_some_and(|token| Arc::ptr_eq(token, &self.token))
        {
            state.pending = None;
            state.candidate_key = None;
        }
    }
}
impl UnlockWorker {
    pub(crate) fn ensure_current(&self) -> Result<(), LifecycleError> {
        let shared = self.owner.upgrade().ok_or(LifecycleError::Closed)?;
        if guard(&shared)?.current(&self.token) {
            Ok(())
        } else {
            Err(LifecycleError::Stale)
        }
    }
    /// Return a candidate only after DEK authentication. Stale owned keys drop/zeroize.
    pub fn finish(self, key: AuthenticatedDek) -> Result<UnlockCandidate, LifecycleError> {
        let shared = self.owner.upgrade().ok_or(LifecycleError::Closed)?;
        let mut state = guard(&shared)?;
        if !state.current(&self.token) {
            return Err(LifecycleError::Stale);
        }
        if key.vault_id() != state.id {
            return Err(LifecycleError::Identity);
        }
        if state.candidate_key.is_some() {
            return Err(LifecycleError::Busy);
        }
        state.candidate_key = Some(key);
        drop(state);
        Ok(UnlockCandidate {
            owner: self.owner,
            token: self.token,
            confirmed: false,
        })
    }
}
#[cfg(test)]
#[path = "vault_lifecycle_tests.rs"]
mod tests;
