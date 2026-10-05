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
impl UnlockPermit {
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
