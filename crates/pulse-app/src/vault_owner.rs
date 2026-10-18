//! Page-scoped Rust-only Vault initialization/authentication, independent of PTY lifetime.
#[path = "vault_key_write.rs"]
mod key_write;
pub use key_write::PrivateKeyIdentityWriteIntent;
#[cfg(feature = "ssh-use")]
#[path = "vault_key_preparation.rs"]
mod key_preparation;
#[cfg(feature = "ssh-use")]
#[path = "vault_session_gesture.rs"]
mod session_gesture;
#[cfg(feature = "ssh-use")]
#[path = "vault_ssh_preparation.rs"]
mod ssh_preparation;
#[cfg(feature = "ssh-use")]
pub use key_preparation::{
    PrivateKeyForwardSink, PrivateKeySessionAction, PrivateKeySessionIntent, PrivateKeySessionSink,
    PrivateKeySessionTask, PrivateKeySourceConfirmation, PrivateKeyTargetAction,
    PrivateKeyTargetSink, PrivateKeyTargetUseIntent, PrivateKeyTargetUseTask,
    SavedPrivateKeyUsePreparation,
};
#[cfg(feature = "ssh-use")]
pub use session_gesture::{
    AgentForwardSink, AgentInteractiveForwardSink, AgentInteractiveSessionAction,
    AgentInteractiveSessionGesture, AgentInteractiveSessionIntent, AgentInteractiveSessionSink,
    AgentSessionAction, AgentSessionGesture, AgentSessionIntent, AgentSessionSink,
    ColdSavedAliasIntent, ColdSavedSessionIntent, InteractiveForwardSink, InteractiveSessionAction,
    InteractiveSessionGesture, InteractiveSessionIntent, InteractiveSessionSink,
    PasswordSessionGesture, PrivateKeySessionGesture, RouteHopGesture, RouteHopSelection,
    RoutedSessionGesture, SavedAliasGesture, SavedSessionGesture, SavedSessionSelection,
};
#[cfg(feature = "ssh-use")]
pub use session_gesture::{
    PrivateKeyInteractiveForwardSink, PrivateKeyInteractiveSessionAction,
    PrivateKeyInteractiveSessionGesture, PrivateKeyInteractiveSessionIntent,
    PrivateKeyInteractiveSessionSink,
};
#[cfg(test)]
#[path = "vault_owner_tests.rs"]
mod tests;
use crate::registry::{PageLaunchScope, SessionRegistry};
use pulse_store::{
    StateStore,
    vault_creation::{ReadyCreation, ValidationCreationRequest, begin_validation_creation},
    vault_database::{VaultEdit, VaultSnapshot},
    vault_lifecycle::{KeyStatus, UnlockRevocation, VaultKeyLifecycle},
    vault_unlock::{
        PasswordInput, ReadyUnlock, ValidationUnlockRequest, begin_validation_unlock_authorized,
    },
};
#[cfg(feature = "ssh-use")]
pub use ssh_preparation::{
    AuthenticatedProtocolParts, PasswordForwardSink, PasswordSessionAction, PasswordSessionIntent,
    PasswordSessionSink, PasswordSessionTask, PasswordTargetAction, PasswordTargetSink,
    PasswordTargetUseIntent, PasswordTargetUseTask, SavedPasswordUsePreparation,
};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;
/// Static errors never include password, identity, nonce or source data.
#[derive(Debug, thiserror::Error)]
pub enum VaultOwnerError {
    /// Retired page/owner, foreign scope or failed source/epoch confirmation.
    #[error("vault.owner_denied")]
    Denied,
    /// Previous native operation still retains capacity; cancellation does not release it.
    #[error("vault.owner_busy")]
    Busy,
    /// No encrypted source available; never create a replacement key on unlock.
    #[error("vault.owner_missing")]
    Missing,
    /// Native work failed or source changed; no automatic retry.
    #[error("vault.owner_failed")]
    Failed,
}
type Result<T> = std::result::Result<T, VaultOwnerError>;
type Authorize = Arc<dyn Fn() -> bool + Send + Sync>;
struct Token {
    changed: Arc<tokio::sync::Notify>,
    guard: Authorize,
    live: AtomicBool,
    accepted: AtomicBool,
    page: Arc<PageLaunchScope>,
    revocation: Mutex<Option<UnlockRevocation>>,
    creation: Mutex<Option<Uuid>>,
}
impl Token {
    fn revoke(&self) {
        self.live.store(false, Ordering::Release);
        if !self.accepted.load(Ordering::Acquire) {
            let revoke = {
                self.revocation
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take()
            };
            if let Some(revoke) = revoke {
                revoke.cancel();
            }
        }
    }
    fn notify(&self) {
        self.changed.notify_waiters();
    }
    fn cancel(&self) {
        self.revoke();
        self.notify();
    }
}
struct State {
    closed: bool,
    pending: Weak<Token>,
    source: Option<VaultSnapshot>,
    scope: Option<VaultKeyLifecycle>,
}
struct Inner {
    registry: SessionRegistry,
    state: Mutex<State>,
}
impl Inner {
    fn guard(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        match self.state.lock() {
            Ok(state) => Ok(state),
            Err(poison) => {
                let mut state = poison.into_inner();
                state.closed = true;
                let pending = state.pending.upgrade();
                if let Some(token) = &pending {
                    token.revoke();
                }
                if let Some(scope) = &state.scope {
                    let _closed = scope.close();
                }
                drop(state);
                if let Some(token) = pending {
                    token.notify();
                }
                Err(VaultOwnerError::Denied)
            }
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        if let Some(pending) = state.pending.upgrade() {
            pending.cancel();
        }
        if let Some(scope) = &state.scope {
            let _closed = scope.close();
        }
    }
}
/// Root-held owner; destroying/closing it clears keys but does not close sessions or SQLite.
#[derive(Clone)]
pub struct PageVaultOwner {
    inner: Arc<Inner>,
}
/// Non-cloneable operation; drop revokes pending epoch immediately, retaining native capacity.
/// Single-use saved-identity edit intent pinned to original native page, source and available key epoch.
/// It contains no plaintext and grants neither secret reading nor SSH target use.
/// ```compile_fail
/// fn duplicate(i:pulse_app::vault_owner::PasswordIdentityWriteIntent){let _=i.clone();}
/// ```
/// ```compile_fail
/// fn dump(i:pulse_app::vault_owner::PasswordIdentityWriteIntent){println!("{i:?}");}
/// ```
#[must_use]
pub struct PasswordIdentityWriteIntent {
    operation: PageVaultOperation,
    source: pulse_store::identity_vault_source::IdentityVaultSource,
    identity: Uuid,
    epoch: pulse_store::vault_lifecycle::AvailableKeyEpoch,
}
impl PasswordIdentityWriteIntent {
    pub(crate) fn cancellation(&self) -> PageVaultCancellation {
        self.operation.cancellation()
    }
    pub(crate) fn bound_to(
        &self,
        controller: &VaultControllerSource,
        page: &Arc<PageLaunchScope>,
    ) -> bool {
        if !controller.matches_password(self) || !Arc::ptr_eq(&self.operation.token.page, page) {
            return false;
        }
        let Ok(owner) = self.operation.current() else {
            return false;
        };
        let Ok(state) = owner.guard() else {
            return false;
        };
        state
            .scope
            .as_ref()
            .is_some_and(|scope| self.epoch.is_current_for(scope).unwrap_or(false))
    }
    pub(crate) async fn write_guarded(
        self,
        store: &StateStore,
        plain: pulse_store::vault_crypto::SecretBytes,
        gate: Authorize,
    ) -> Result<VaultSnapshot> {
        self.operation
            .write_password_identity_epoch(
                store,
                self.source,
                self.identity,
                plain,
                Some(self.epoch),
                gate,
            )
            .await
    }

    /// Consume once, using only the captured epoch; later unlock can never upgrade this intent.
    pub async fn write(
        self,
        store: &StateStore,
        plain: pulse_store::vault_crypto::SecretBytes,
    ) -> Result<VaultSnapshot> {
        self.operation
            .write_password_identity_epoch(
                store,
                self.source,
                self.identity,
                plain,
                Some(self.epoch),
                Arc::new(|| true),
            )
            .await
    }
}

/// Non-cloneable operation; drop revokes pending epoch immediately, retaining native capacity.
pub struct PageVaultOperation {
    owner: Weak<Inner>,
    token: Arc<Token>,
}
/// Move-only key-free native result; lost delivery clears this pending activation.
pub struct PageVaultActivation {
    operation: PageVaultOperation,
}
impl PageVaultActivation {
    /// Opaque page-source equality for a native handoff, not a use permission.
    pub fn matches_page(&self, page: &Arc<PageLaunchScope>) -> bool {
        Arc::ptr_eq(&self.operation.token.page, page)
    }
    /// Accept only after the original Rust waiter receives the joined/source-confirmed result.
    pub fn accept(self) -> Result<()> {
        let owner = self.operation.current()?;
        let state = owner.guard()?;
        if state.closed
            || !self.operation.token.live.load(Ordering::Acquire)
            || !(self.operation.token.guard)()
            || owner
                .registry
                .check_launch_page(&self.operation.token.page)
                .is_err()
        {
            return Err(VaultOwnerError::Denied);
        }
        self.operation.token.accepted.store(true, Ordering::Release);
        self.operation
            .token
            .revocation
            .lock()
            .map_err(|_| VaultOwnerError::Denied)?
            .take();
        Ok(())
    }
}
/// Cloneable weak cancellation receipt; neither key material nor another epoch can be selected.
#[derive(Clone)]
pub struct PageVaultCancellation {
    owner: Weak<Inner>,
    token: Weak<Token>,
}
impl PageVaultCancellation {
    /// Await explicit cancellation/drop/owner retirement of this original operation, without polling.
    /// Holds only key-free notification data, not the owner, token, key or native capacity.
    /// Caller still rechecks source/epoch/page/native guards: their changes alone are not this signal.
    pub async fn wait_cancelled(&self) {
        let Some(token) = self.token.upgrade() else {
            return;
        };
        let signal = token.changed.clone();
        drop(token);
        loop {
            let notified = signal.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let live = self.owner.upgrade().is_some()
                && self
                    .token
                    .upgrade()
                    .is_some_and(|token| token.live.load(Ordering::Acquire));
            if !live {
                return;
            }
            notified.await;
        }
    }
    /// Immediately revoke this native operation and its unaccepted key epoch.
    pub fn cancel(&self) {
        if let Some(token) = self.token.upgrade() {
            token.cancel();
        }
    }
}

impl Drop for PageVaultOperation {
    fn drop(&mut self) {
        self.token.cancel();
    }
}
pub(crate) struct VaultControllerSource {
    owner: Weak<Inner>,
}
impl VaultControllerSource {
    pub(crate) fn matches_password(&self, intent: &PasswordIdentityWriteIntent) -> bool {
        Weak::ptr_eq(&self.owner, &intent.operation.owner)
    }
    pub(crate) fn matches_activation(&self, activation: &PageVaultActivation) -> bool {
        Weak::ptr_eq(&self.owner, &activation.operation.owner)
    }
    pub(crate) fn matches_cancel(&self, cancel: &PageVaultCancellation) -> bool {
        Weak::ptr_eq(&self.owner, &cancel.owner)
    }
}
impl PageVaultOwner {
    pub(crate) fn handoff_source(&self) -> (SessionRegistry, VaultControllerSource) {
        (
            self.inner.registry.clone(),
            VaultControllerSource {
                owner: Arc::downgrade(&self.inner),
            },
        )
    }
    /// Registry provenance is backend-controlled, never a frontend owner ID.
    pub fn new(registry: SessionRegistry) -> Self {
        Self {
            inner: Arc::new(Inner {
                registry,
                state: Mutex::new(State {
                    closed: false,
                    pending: Weak::new(),
                    source: None,
                    scope: None,
                }),
            }),
        }
    }
    /// Begin only on a current opaque page scope; busy until all native predicates actually drop.
    pub fn begin(&self, page: Arc<PageLaunchScope>) -> Result<PageVaultOperation> {
        self.begin_authorized(page, Arc::new(|| true))
    }
    /// Native gate retains actual read/CPU resources and revokes when its waiter is cancelled.
    pub fn begin_authorized(
        &self,
        page: Arc<PageLaunchScope>,
        guard: Authorize,
    ) -> Result<PageVaultOperation> {
        if !guard() {
            return Err(VaultOwnerError::Denied);
        }
        self.inner
            .registry
            .check_launch_page(&page)
            .map_err(|_| VaultOwnerError::Denied)?;
        let mut state = self.inner.guard()?;
        if state.closed {
            return Err(VaultOwnerError::Denied);
        }
        if state.pending.upgrade().is_some() {
            return Err(VaultOwnerError::Busy);
        }
        let token = Arc::new(Token {
            changed: Arc::new(tokio::sync::Notify::new()),
            guard,
            live: AtomicBool::new(true),
            accepted: AtomicBool::new(false),
            page,
            revocation: Mutex::new(None),
            creation: Mutex::new(None),
        });
        state.pending = Arc::downgrade(&token);
        Ok(PageVaultOperation {
            owner: Arc::downgrade(&self.inner),
            token,
        })
    }
    /// Called synchronously after native navigation/window retirement; preserve earlier accepted key.
    pub fn retire_page(&self) -> Result<()> {
        let pending = {
            let state = self.inner.guard()?;
            let pending = state.pending.upgrade();
            if let Some(token) = &pending {
                token.revoke();
            }
            pending
        };
        if let Some(token) = pending {
            token.notify();
        }
        Ok(())
    }
    /// Explicit app lock clears current key and pending epoch; existing sessions remain untouched.
    pub fn lock(&self) -> Result<()> {
        let (pending, result) = {
            let state = self.inner.guard()?;
            let pending = state.pending.upgrade();
            if let Some(token) = &pending {
                token.revoke();
            }
            let result = state.scope.as_ref().map_or(Ok(()), |scope| {
                scope.lock().map_err(|_| VaultOwnerError::Denied)
            });
            (pending, result)
        };
        if let Some(token) = pending {
            token.notify();
        }
        result
    }
    /// Permanently close owner, not shared database or root sessions; native credits stay charged.
    pub fn close(&self) -> Result<()> {
        let (pending, result) = {
            let mut state = self.inner.guard()?;
            if state.closed {
                return Ok(());
            }
            state.closed = true;
            let pending = state.pending.upgrade();
            if let Some(token) = &pending {
                token.revoke();
            }
            let result = state.scope.as_ref().map_or(Ok(()), |scope| {
                scope.close().map_err(|_| VaultOwnerError::Denied)
            });
            (pending, result)
        };
        if let Some(token) = pending {
            token.notify();
        }
        result
    }
}
impl PageVaultOperation {
    /// Key-free cancellation for a native waiter; capacity remains until actual predicates drop.
    pub fn cancellation(&self) -> PageVaultCancellation {
        PageVaultCancellation {
            owner: self.owner.clone(),
            token: Arc::downgrade(&self.token),
        }
    }
    fn current(&self) -> Result<Arc<Inner>> {
        let owner = self.owner.upgrade().ok_or(VaultOwnerError::Denied)?;
        if !(self.token.guard)()
            || !self.token.live.load(Ordering::Acquire)
            || owner.registry.check_launch_page(&self.token.page).is_err()
        {
            return Err(VaultOwnerError::Denied);
        }
        let state = owner.guard()?;
        if state.closed
            || !state
                .pending
                .upgrade()
                .is_some_and(|t| Arc::ptr_eq(&t, &self.token))
        {
            return Err(VaultOwnerError::Denied);
        }
        drop(state);
        Ok(owner)
    }
    /// Retain this operation token in actual CPU/SQL predicates, never only its waiting future.
    pub fn authorization(&self) -> Authorize {
        let owner = self.owner.clone();
        let token = self.token.clone();
        Arc::new(move || {
            let Some(owner) = owner.upgrade() else {
                return false;
            };
            if !(token.guard)()
                || !token.live.load(Ordering::Acquire)
                || owner.registry.check_launch_page(&token.page).is_err()
            {
                return false;
            }
            owner.guard().is_ok_and(|state| {
                !state.closed
                    && state
                        .pending
                        .upgrade()
                        .is_some_and(|t| Arc::ptr_eq(&t, &token))
            })
        })
    }
    fn bind(&self, snapshot: VaultSnapshot) -> Result<Option<VaultKeyLifecycle>> {
        let owner = self.current()?;
        let mut state = owner.guard()?;
        if !(self.token.guard)() || !self.token.live.load(Ordering::Acquire) || state.closed {
            return Err(VaultOwnerError::Denied);
        }
        if let Some(old) = &state.source {
            let old_bytes = old
                .envelope()
                .map(|e| e.encode_ciphertext())
                .transpose()
                .map_err(|_| VaultOwnerError::Failed)?;
            let new_bytes = snapshot
                .envelope()
                .map(|e| e.encode_ciphertext())
                .transpose()
                .map_err(|_| VaultOwnerError::Failed)?;
            if !old.same_store(&snapshot)
                || (old.vault_id().is_some()
                    && (old.vault_id() != snapshot.vault_id() || old_bytes != new_bytes))
            {
                state.closed = true;
                if let Some(scope) = &state.scope {
                    let _closed = scope.close();
                }
                self.token.revoke();
                drop(state);
                self.token.notify();
                return Err(VaultOwnerError::Denied);
            }
        }
        if state.scope.is_none()
            && let Some(id) = snapshot.vault_id()
        {
            state.scope = Some(VaultKeyLifecycle::new(id).map_err(|_| VaultOwnerError::Denied)?);
        }
        state.source = Some(snapshot);
        Ok(state.scope.clone())
    }
    /// Capture a single-use password-edit intent; source metadata is not peer/target-use authorization.
    /// Caller must read the native source under actual I/O admission and separately bound UI ticket expiry.
    pub fn prepare_password_identity_write(
        self,
        source: pulse_store::identity_vault_source::IdentityVaultSource,
        identity: Uuid,
    ) -> Result<PasswordIdentityWriteIntent> {
        self.current()?;
        let row = source
            .identities()
            .records()
            .find(|r| r.id() == identity)
            .ok_or(VaultOwnerError::Denied)?;
        let reference = row.identity().password();
        if reference.kind() != pulse_core::secret_ref::SecretKind::SshPassword
            || Some(reference.vault_id()) != source.vault().vault_id()
        {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .bind(source.vault().clone())?
            .ok_or(VaultOwnerError::Denied)?;
        let epoch = scope
            .available_epoch()
            .map_err(|_| VaultOwnerError::Denied)?;
        Ok(PasswordIdentityWriteIntent {
            operation: self,
            source,
            identity,
            epoch,
        })
    }

    /// Rust-only saved-identity password write; no path/raw SecretRef/kind comes from a frontend.
    /// Owned plaintext is sealed and dropped before SQL; native receives encrypted record only.
    /// This edits Vault storage, never grants credential read or SSH peer/target-use authority.
    pub async fn write_password_identity_source(
        &self,
        store: &StateStore,
        source: pulse_store::identity_vault_source::IdentityVaultSource,
        identity_id: Uuid,
        plain: pulse_store::vault_crypto::SecretBytes,
    ) -> Result<VaultSnapshot> {
        self.write_password_identity_epoch(
            store,
            source,
            identity_id,
            plain,
            None,
            Arc::new(|| true),
        )
        .await
    }
    async fn write_password_identity_epoch(
        &self,
        store: &StateStore,
        source: pulse_store::identity_vault_source::IdentityVaultSource,
        identity_id: Uuid,
        plain: pulse_store::vault_crypto::SecretBytes,
        pinned_epoch: Option<pulse_store::vault_lifecycle::AvailableKeyEpoch>,
        dispatch_gate: Authorize,
    ) -> Result<VaultSnapshot> {
        struct Waiter {
            cancel: PageVaultCancellation,
            complete: bool,
        }
        impl Drop for Waiter {
            fn drop(&mut self) {
                if !self.complete {
                    self.cancel.cancel();
                }
            }
        }
        let mut waiter = Waiter {
            cancel: self.cancellation(),
            complete: false,
        };
        self.current()?;
        let identity = source
            .identities()
            .records()
            .find(|row| row.id() == identity_id)
            .ok_or(VaultOwnerError::Denied)?;
        let reference = identity.identity().password();
        if reference.kind() != pulse_core::secret_ref::SecretKind::SshPassword
            || Some(reference.vault_id()) != source.vault().vault_id()
        {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .bind(source.vault().clone())?
            .ok_or(VaultOwnerError::Denied)?;
        let epoch = match pinned_epoch {
            Some(epoch) => epoch,
            None => scope
                .available_epoch()
                .map_err(|_| VaultOwnerError::Denied)?,
        };
        let record = pulse_store::vault_record::CredentialRecord::seal_in_epoch(
            &scope,
            &epoch,
            reference.id(),
            "ssh_password",
            plain.expose(),
        )
        .map_err(|_| VaultOwnerError::Denied)?;
        drop(plain);
        let edit = if reference.has_value() {
            VaultEdit::Replace(record)
        } else {
            VaultEdit::Create(record)
        };
        if !dispatch_gate() {
            return Err(VaultOwnerError::Denied);
        }
        let original = self.authorization();
        let native_gate = dispatch_gate.clone();
        let authorize: Authorize = Arc::new(move || original() && native_gate());
        let saved = store
            .edit_identity_password_ciphertext(
                source,
                identity_id,
                edit,
                scope.clone(),
                epoch.clone(),
                authorize,
            )
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        self.current()?;
        if !epoch
            .is_current_for(&scope)
            .map_err(|_| VaultOwnerError::Denied)?
        {
            return Err(VaultOwnerError::Denied);
        }
        if !dispatch_gate() {
            return Err(VaultOwnerError::Denied);
        }
        waiter.complete = true;
        Ok(saved)
    }

    /// Validate a saved password identity's exact source against current page/controller/key epoch.
    /// This is preparation only: no plaintext, ongoing freshness or SSH peer/target-use permission.
    /// The supplied native source must have been read under an actual-operation I/O predicate.
    pub async fn validate_password_identity_source(
        &self,
        store: &StateStore,
        source: pulse_store::identity_vault_source::IdentityVaultSource,
        identity_id: Uuid,
    ) -> Result<()> {
        self.current()?;
        let identity = source
            .identities()
            .records()
            .find(|row| row.id() == identity_id)
            .ok_or(VaultOwnerError::Denied)?;
        let reference = identity.identity().password();
        if !reference.has_value()
            || reference.kind() != pulse_core::secret_ref::SecretKind::SshPassword
            || Some(reference.vault_id()) != source.vault().vault_id()
        {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .bind(source.vault().clone())?
            .ok_or(VaultOwnerError::Denied)?;
        let epoch = scope
            .available_epoch()
            .map_err(|_| VaultOwnerError::Denied)?;
        let current = self.authorization();
        let guarded_scope = scope.clone();
        let guarded_epoch = epoch.clone();
        let authorize = Arc::new(move || {
            current()
                && guarded_epoch
                    .is_current_for(&guarded_scope)
                    .unwrap_or(false)
        });
        store
            .validate_identity_vault_source(source, authorize)
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        self.current()?;
        if !epoch
            .is_current_for(&scope)
            .map_err(|_| VaultOwnerError::Denied)?
        {
            return Err(VaultOwnerError::Denied);
        }
        Ok(())
    }

    /// Availability only after a fresh bounded source read; not a secret-use or connection permission.
    pub fn status(&self, snapshot: VaultSnapshot) -> Result<Option<KeyStatus>> {
        self.bind(snapshot)?
            .map(|s| s.status().map_err(|_| VaultOwnerError::Denied))
            .transpose()
    }
    /// Explicit empty-source initialization; fresh ID is generated in Rust, never supplied by page.
    pub fn begin_creation(
        &self,
        password: PasswordInput,
        snapshot: VaultSnapshot,
    ) -> Result<(Uuid, ValidationCreationRequest)> {
        if self.bind(snapshot)?.is_some() {
            return Err(VaultOwnerError::Denied);
        }
        let id = Uuid::now_v7();
        let mut creation = self
            .token
            .creation
            .lock()
            .map_err(|_| VaultOwnerError::Denied)?;
        if creation.is_some() {
            return Err(VaultOwnerError::Busy);
        }
        let request = begin_validation_creation(password, id, self.authorization())
            .map_err(|_| VaultOwnerError::Failed)?;
        *creation = Some(id);
        Ok((id, request))
    }
    /// Persist original empty snapshot once; receipt guard and page predicate both cover native SQL.
    pub async fn persist_creation(
        &self,
        store: &StateStore,
        snapshot: VaultSnapshot,
        id: Uuid,
        ready: ReadyCreation,
    ) -> Result<()> {
        self.current()?;
        if *self
            .token
            .creation
            .lock()
            .map_err(|_| VaultOwnerError::Denied)?
            != Some(id)
            || self.bind(snapshot.clone())?.is_some()
        {
            return Err(VaultOwnerError::Denied);
        }
        let envelope = ready.envelope().map_err(|_| VaultOwnerError::Denied)?;
        let page = self.authorization();
        let creation = ready.persistence_authorization();
        let gate: Authorize = Arc::new(move || page() && creation());
        let saved = store
            .edit_vault_ciphertext(snapshot, VaultEdit::Initialize(id, envelope), gate)
            .await
            .map_err(|_| VaultOwnerError::Failed)?;
        self.bind(saved)?;
        Ok(())
    }
    /// Begin authenticated unlock and register exact-epoch retirement before exposing pending work.
    pub fn begin_unlock(
        &self,
        password: PasswordInput,
        snapshot: VaultSnapshot,
    ) -> Result<ValidationUnlockRequest> {
        let scope = self
            .bind(snapshot.clone())?
            .ok_or(VaultOwnerError::Missing)?;
        if scope.status().map_err(|_| VaultOwnerError::Denied)? != KeyStatus::Locked {
            return Err(VaultOwnerError::Busy);
        }
        let envelope = snapshot.envelope().ok_or(VaultOwnerError::Missing)?.clone();
        let request =
            begin_validation_unlock_authorized(&scope, password, envelope, self.authorization())
                .map_err(|_| VaultOwnerError::Failed)?;
        let revocation = request.revocation();
        if let Err(error) = self.register(revocation.clone(), &scope) {
            revocation.cancel();
            drop(request.cancel());
            return Err(error);
        }
        Ok(request)
    }
    fn register(&self, revocation: UnlockRevocation, scope: &VaultKeyLifecycle) -> Result<()> {
        let owner = self.current()?;
        let state = owner.guard()?;
        if state.closed || !self.token.live.load(Ordering::Acquire) || !revocation.belongs_to(scope)
        {
            revocation.cancel();
            return Err(VaultOwnerError::Denied);
        }
        *self
            .token
            .revocation
            .lock()
            .map_err(|_| VaultOwnerError::Denied)? = Some(revocation);
        Ok(())
    }
    /// Atomic source/epoch confirmation precedes native owner acceptance; no key/secret is returned.
    pub async fn confirm_unlock(
        self,
        store: &StateStore,
        snapshot: VaultSnapshot,
        ready: ReadyUnlock,
    ) -> Result<PageVaultActivation> {
        let scope = self
            .bind(snapshot.clone())?
            .ok_or(VaultOwnerError::Missing)?;
        self.register(ready.revocation(), &scope)?;
        store
            .confirm_vault_unlock(snapshot, scope, ready, self.authorization())
            .await
            .map_err(|_| VaultOwnerError::Failed)?;
        let owner = self.current()?;
        let state = owner.guard()?;
        if state.closed
            || !self.token.live.load(Ordering::Acquire)
            || owner.registry.check_launch_page(&self.token.page).is_err()
        {
            self.token.revoke();
            drop(state);
            self.token.notify();
            return Err(VaultOwnerError::Denied);
        }
        drop(state);
        Ok(PageVaultActivation { operation: self })
    }
}
