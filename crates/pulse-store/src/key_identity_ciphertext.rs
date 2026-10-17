//! Ciphertext-only atomic private-key/passphrase write; references and metadata do not grant use authority.
use crate::{
    Request, Result, StateStore, StoreError,
    key_identity_vault_source::KeyIdentityVaultSource,
    ssh_key_identities,
    vault_database::{self, VaultEdit, VaultSnapshot},
    vault_lifecycle::{AvailableKeyEpoch, VaultKeyLifecycle},
    vault_record::CredentialRecord,
};
use pulse_core::secret_ref::{SecretKind, SecretRef};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;
/// One bounded ciphertext update for exactly the references in one saved identity.
/// Trusted caller must seal under the separately supplied original available epoch.
/// No plaintext, Debug/serialization, peer permission or connection action.
/// ```compile_fail
/// fn dump(edit: pulse_store::key_identity_ciphertext::KeyIdentityCiphertextEdit) { println!("{edit:?}"); }
/// ```
/// ```compile_fail
/// fn send(edit: pulse_store::key_identity_ciphertext::KeyIdentityCiphertextEdit) { let _ = serde_json::to_string(&edit); }
/// ```
pub struct KeyIdentityCiphertextEdit {
    expected: KeyIdentityVaultSource,
    identity: Uuid,
    edits: Vec<VaultEdit>,
}
fn record_edit(
    vault: &VaultSnapshot,
    reference: SecretRef,
    record: CredentialRecord,
) -> Result<VaultEdit> {
    if vault.vault_id() != Some(reference.vault_id())
        || vault.credential_deleted(reference.id())
        || record.id() != reference.id()
        || record.kind() != reference.kind().as_str()
    {
        return Err(StoreError::IdentityConflict);
    }
    // Re-validate namespace and bounded ciphertext without decrypting or doing KDF work in SQL.
    let bytes = record
        .encode_ciphertext()
        .map_err(|_| StoreError::VaultRecord)?;
    let record = CredentialRecord::decode_ciphertext(&bytes, reference.vault_id())
        .map_err(|_| StoreError::VaultRecord)?;
    Ok(if reference.has_value() {
        VaultEdit::Replace(record)
    } else {
        VaultEdit::Create(record)
    })
}
impl KeyIdentityCiphertextEdit {
    /// Match both roles to the saved original source; absent optional reference requires absent record.
    /// Does not authenticate ciphertext, unlock Vault or grant write/SSH permission.
    pub fn new(
        expected: KeyIdentityVaultSource,
        identity: Uuid,
        key: CredentialRecord,
        pass: Option<CredentialRecord>,
    ) -> Result<Self> {
        let row = expected
            .identities()
            .records()
            .find(|row| row.id() == identity)
            .ok_or(StoreError::IdentityConflict)?;
        let reference = row.identity().private_key();
        if reference.kind() != SecretKind::SshPrivateKey {
            return Err(StoreError::IdentityConflict);
        }
        let mut edits = vec![record_edit(expected.vault(), reference, key)?];
        match (row.identity().passphrase(), pass) {
            (Some(reference), Some(record)) if reference.kind() == SecretKind::SshKeyPassphrase => {
                edits.push(record_edit(expected.vault(), reference, record)?)
            }
            (None, None) => {}
            _ => return Err(StoreError::IdentityConflict),
        }
        Ok(Self {
            expected,
            identity,
            edits,
        })
    }
}
pub(crate) struct KeyIdentityCiphertextRequest {
    pub(crate) owner: Uuid,
    pub(crate) edit: KeyIdentityCiphertextEdit,
    pub(crate) scope: VaultKeyLifecycle,
    pub(crate) epoch: AvailableKeyEpoch,
    pub(crate) authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    pub(crate) reply: oneshot::Sender<Result<VaultSnapshot>>,
}
impl KeyIdentityCiphertextRequest {
    pub(crate) fn execute(self, connection: &rusqlite::Connection) {
        if self.reply.is_closed() {
            return;
        }
        let KeyIdentityCiphertextEdit {
            expected,
            identity,
            edits,
        } = self.edit;
        let allowed = || {
            !self.reply.is_closed()
                && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.authorize)()))
                    .unwrap_or(false)
                && self.epoch.is_current_for(&self.scope).unwrap_or(false)
        };
        let result = (|| {
            if expected.identities().owner != self.owner
                || expected.vault().vault_id() != Some(self.scope.expected_id()?)
            {
                return Err(StoreError::IdentityConflict);
            }
            vault_database::edit_batch_preflight(
                connection,
                self.owner,
                expected.vault(),
                edits,
                allowed,
                |tx, current_vault| {
                    let current = ssh_key_identities::load(tx, self.owner)?;
                    if !ssh_key_identities::same_metadata(&current, expected.identities())
                        || !vault_database::same_source(current_vault, expected.vault())?
                        || !current.records().any(|row| row.id() == identity)
                    {
                        return Err(StoreError::IdentityConflict);
                    }
                    Ok(())
                },
            )
        })();
        drop(self.authorize);
        let _ = self.reply.send(result);
    }
}
impl StateStore {
    /// Native single-transaction ciphertext edit: one original source, both saved references, original epoch.
    /// Canceled waiters cannot release actual-operation credit while SQL is still active.
    pub async fn edit_key_identity_ciphertext(
        &self,
        edit: KeyIdentityCiphertextEdit,
        scope: VaultKeyLifecycle,
        epoch: AvailableKeyEpoch,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<VaultSnapshot> {
        if edit.expected.identities().owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::KeyIdentityCiphertextEdit(Box::new(
            KeyIdentityCiphertextRequest {
                owner: self.host_owner,
                edit,
                scope,
                epoch,
                authorize,
                reply,
            },
        )))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
