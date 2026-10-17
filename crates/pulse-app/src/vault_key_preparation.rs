//! Network-neutral private-key preconditions; verified-peer and target-use authority remain separate.
use super::{PageVaultOperation, Result, VaultOwnerError};
use pulse_core::secret_ref::SecretKind;
use pulse_store::{
    StateStore,
    ssh_key_target_source::SshKeyTargetSource,
    vault_lifecycle::{AvailableKeyEpoch, VaultKeyLifecycle},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
#[path = "vault_key_target_action.rs"]
mod target_action;
pub use target_action::{
    PrivateKeyForwardSink, PrivateKeySessionAction, PrivateKeySessionIntent, PrivateKeySessionSink,
    PrivateKeySessionTask, PrivateKeySourceConfirmation, PrivateKeyTargetAction,
    PrivateKeyTargetSink, PrivateKeyTargetUseIntent, PrivateKeyTargetUseTask,
};

/// Single-use current-page, complete-source and original-key-epoch preparation.
/// Neither this object nor successful validation authorizes plaintext, a peer, or authentication.
/// ```compile_fail
/// fn dump(p: pulse_app::vault_owner::SavedPrivateKeyUsePreparation) { println!("{p:?}"); }
/// ```
/// ```compile_fail
/// fn send(p: pulse_app::vault_owner::SavedPrivateKeyUsePreparation) { let _ = serde_json::to_vec(&p); }
/// ```
pub struct SavedPrivateKeyUsePreparation {
    operation: PageVaultOperation,
    selected: (Uuid, Uuid),
    source: SshKeyTargetSource,
    scope: VaultKeyLifecycle,
    epoch: AvailableKeyEpoch,
    deadline: Instant,
}

fn selected(source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
    source.supports_private_key_selection(host, identity)
        && source.identities().records().any(|row| {
            if row.id() != identity {
                return false;
            }
            let value = row.identity();
            let key = value.private_key();
            key.kind() == SecretKind::SshPrivateKey
                && key.has_value()
                && Some(key.vault_id()) == source.vault().vault_id()
                && value.passphrase().is_none_or(|pass| {
                    pass.kind() == SecretKind::SshKeyPassphrase
                        && pass.has_value()
                        && pass.vault_id() == key.vault_id()
                        && pass.id() != key.id()
                })
        })
}
impl SavedPrivateKeyUsePreparation {
    fn authorization(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let selection = selected(&self.source, self.selected.0, self.selected.1);
        let target = self.source.target_liveness();
        let page = self.operation.authorization();
        let epoch = self.epoch.clone();
        let scope = self.scope.clone();
        let deadline = self.deadline;
        Arc::new(move || {
            selection
                && target()
                && Instant::now() < deadline
                && page()
                && epoch.is_current_for(&scope).unwrap_or(false)
        })
    }
    /// Consume for one exact source/page/original-epoch check. No ongoing source lock or use permission is returned.
    pub async fn validate(self, store: &StateStore) -> Result<()> {
        let allowed = self.authorization();
        if !allowed() {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), allowed.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !allowed() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(())
    }
}
impl PageVaultOperation {
    /// Prepare the saved private-key role on the current page and currently available original epoch.
    /// Does not decode a key or passphrase, connect, verify a peer, or approve credential use.
    /// Native guard credit follows the SQL predicate until exit even when this waiter is cancelled.
    pub async fn prepare_saved_private_key_use(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        host: Uuid,
        identity: Uuid,
    ) -> Result<SavedPrivateKeyUsePreparation> {
        self.prepare_saved_private_key_use_pinned(store, source, host, identity, None)
            .await
    }
    /// A separate trusted original-intent capture may pin an epoch, never borrow a later unlock.
    pub(super) async fn prepare_saved_private_key_use_pinned(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        host: Uuid,
        identity: Uuid,
        pinned: Option<AvailableKeyEpoch>,
    ) -> Result<SavedPrivateKeyUsePreparation> {
        self.prepare_saved_private_key_use_pinned_until(
            store,
            source,
            host,
            identity,
            pinned,
            Instant::now() + Duration::from_secs(5),
        )
        .await
    }
    /// Only independently captured joint-authentication actions may supply a longer fixed deadline.
    pub(super) async fn prepare_saved_private_key_use_pinned_until(
        self,
        store: &StateStore,
        source: SshKeyTargetSource,
        host: Uuid,
        identity: Uuid,
        pinned: Option<AvailableKeyEpoch>,
        deadline: Instant,
    ) -> Result<SavedPrivateKeyUsePreparation> {
        self.current()?;
        if !selected(&source, host, identity) {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .bind(source.vault().clone())?
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
        let preparation = SavedPrivateKeyUsePreparation {
            operation: self,
            selected: (host, identity),
            source,
            scope,
            epoch,
            deadline,
        };
        let allowed = preparation.authorization();
        store
            .validate_ssh_key_target_source(preparation.source.clone(), allowed.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !allowed() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(preparation)
    }
}
