//! Network-neutral saved-password preconditions. Actual SSH peer/trust/use authorization belongs above pulse-app.
use super::{PageVaultOperation, Result, VaultOwnerError};
use pulse_store::{
    StateStore,
    ssh_target_source::SshTargetSource,
    vault_lifecycle::{AvailableKeyEpoch, VaultKeyLifecycle},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
#[path = "vault_target_action.rs"]
mod target_action;
pub use target_action::{
    AuthenticatedProtocolParts, PasswordForwardSink, PasswordSessionAction, PasswordSessionIntent,
    PasswordSessionSink, PasswordSessionTask, PasswordTargetAction, PasswordTargetSink,
    PasswordTargetUseIntent, PasswordTargetUseTask,
};
/// Single-use source/page/original-epoch preparation, never peer proof or target-use permission.
/// Upper adapters must independently bind the actual verified connection and validated trust source.
/// Native liveness/resource guards may be installed through `PageVaultOwner::begin_authorized`.
/// No plaintext/authentication/credential-read capability is returned.
/// ```compile_fail
/// fn dump(v:pulse_app::vault_owner::SavedPasswordUsePreparation){println!("{v:?}");}
/// ```
pub struct SavedPasswordUsePreparation {
    operation: PageVaultOperation,
    selected: (Uuid, Uuid),
    source: SshTargetSource,
    scope: VaultKeyLifecycle,
    epoch: AvailableKeyEpoch,
    deadline: Instant,
}
impl SavedPasswordUsePreparation {
    fn authorization(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let selection = self
            .source
            .supports_password_selection(self.selected.0, self.selected.1)
            && self
                .source
                .identity_vault()
                .identities()
                .records()
                .any(|row| row.id() == self.selected.1 && row.identity().password().has_value());
        let target = self.source.target_liveness();
        let page = self.operation.authorization();
        let epoch = self.epoch.clone();
        let deadline = self.deadline;
        Arc::new(move || {
            selection
                && target()
                && Instant::now() < deadline
                && page()
                && epoch.is_original_current().unwrap_or(false)
        })
    }
    /// Consume once for exact original-source/page/key-epoch/native-guard validation only.
    /// This point receipt cannot authorize a peer or credential consumption; no ongoing source lock is held.
    pub async fn validate(self, store: &StateStore) -> Result<()> {
        let allowed = self.authorization();
        if !allowed() {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_target_source(self.source.clone(), allowed.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !allowed() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(())
    }
}
impl PageVaultOperation {
    /// Freeze saved password identity, target metadata and original available epoch under current page.
    /// Does not parse public keys, verify a connection or approve target use. Upper Rust adapter owns those checks.
    /// Native guard/resource credit stays in actual SQL predicates after waiting-future cancellation.
    pub async fn prepare_saved_password_use(
        self,
        store: &StateStore,
        source: SshTargetSource,
        host: Uuid,
        identity: Uuid,
    ) -> Result<SavedPasswordUsePreparation> {
        self.prepare_saved_password_use_pinned(store, source, host, identity, None)
            .await
    }
    /// Original key epoch from a separate trusted GUI capture; never recapture a later unlock.
    pub(super) async fn prepare_saved_password_use_pinned(
        self,
        store: &StateStore,
        source: SshTargetSource,
        host: Uuid,
        identity: Uuid,
        pinned: Option<AvailableKeyEpoch>,
    ) -> Result<SavedPasswordUsePreparation> {
        self.prepare_saved_password_use_pinned_at(
            store,
            source,
            host,
            identity,
            pinned,
            Instant::now() + Duration::from_secs(5),
        )
        .await
    }
    // Explicit joint actions supply their original deadline, never a fresh preparation lifetime.
    pub(super) async fn prepare_saved_password_use_pinned_at(
        self,
        store: &StateStore,
        source: SshTargetSource,
        host: Uuid,
        identity: Uuid,
        pinned: Option<AvailableKeyEpoch>,
        deadline: Instant,
    ) -> Result<SavedPasswordUsePreparation> {
        let selected = (host, identity);
        if Instant::now() >= deadline {
            return Err(VaultOwnerError::Denied);
        }
        self.current()?;
        if !source.supports_password_selection(host, identity) {
            return Err(VaultOwnerError::Denied);
        }
        let row = source
            .identity_vault()
            .identities()
            .records()
            .find(|row| row.id() == identity)
            .ok_or(VaultOwnerError::Denied)?;
        let reference = row.identity().password();
        if reference.kind() != pulse_core::secret_ref::SecretKind::SshPassword
            || !reference.has_value()
            || Some(reference.vault_id()) != source.identity_vault().vault().vault_id()
        {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .bind(source.identity_vault().vault().clone())?
            .ok_or(VaultOwnerError::Denied)?;
        let epoch = match pinned {
            Some(epoch) => {
                if !epoch.is_current_for(&scope).unwrap_or(false) {
                    return Err(VaultOwnerError::Denied);
                }
                epoch
            }
            None => scope
                .available_epoch()
                .map_err(|_| VaultOwnerError::Denied)?,
        };
        let preparation = SavedPasswordUsePreparation {
            operation: self,
            selected,
            source,
            scope,
            epoch,
            deadline,
        };
        let allowed = preparation.authorization();
        store
            .validate_ssh_target_source(preparation.source.clone(), allowed.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !allowed() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(preparation)
    }
}
