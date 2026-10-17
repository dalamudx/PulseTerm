//! Private-key references and Vault ciphertext captured at one native boundary; no plaintext or use permission.
use crate::{
    Request, Result, StateStore, StoreError,
    ssh_key_identities::{self, KeyIdentitySnapshot},
    vault_database::{self, VaultSnapshot},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;
/// Opaque instance-bound source for later exact validation; never activates a key or grants SSH use.
/// ```compile_fail
/// fn log(s: pulse_store::key_identity_vault_source::KeyIdentityVaultSource) { println!("{s:?}"); }
/// ```
/// ```compile_fail
/// fn send(s: pulse_store::key_identity_vault_source::KeyIdentityVaultSource) { let _ = serde_json::to_string(&s); }
/// ```
#[derive(Clone)]
pub struct KeyIdentityVaultSource {
    identities: KeyIdentitySnapshot,
    vault: VaultSnapshot,
}
impl KeyIdentityVaultSource {
    /// Metadata with both credential presences derived from the same source; not key availability.
    pub fn identities(&self) -> &KeyIdentitySnapshot {
        &self.identities
    }
    /// Encrypted native source only, never plaintext or activation permission.
    pub fn vault(&self) -> &VaultSnapshot {
        &self.vault
    }
}
fn read(c: &Connection, owner: Uuid) -> Result<KeyIdentityVaultSource> {
    let identities = ssh_key_identities::load(c, owner)?;
    let vault = vault_database::load(c, owner)?;
    if identities.vault_id() != vault.vault_id() {
        return Err(StoreError::IdentityRecord);
    }
    Ok(KeyIdentityVaultSource { identities, vault })
}
pub(crate) fn load(c: &Connection, owner: Uuid) -> Result<KeyIdentityVaultSource> {
    if !c.is_autocommit() {
        return read(c, owner);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    read(&tx, owner)
}
pub(crate) fn validate(
    c: &Connection,
    owner: Uuid,
    expected: &KeyIdentityVaultSource,
    authorize: impl Fn() -> bool,
) -> Result<()> {
    if expected.identities.owner != owner {
        return Err(StoreError::IdentityConflict);
    }
    if !authorize() {
        return Err(StoreError::IdentityAuthorization);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = read(&tx, owner)?;
    if !ssh_key_identities::same_metadata(&current.identities, &expected.identities)
        || !vault_database::same_source(&current.vault, &expected.vault)?
    {
        return Err(StoreError::IdentityConflict);
    }
    if !authorize() {
        return Err(StoreError::IdentityAuthorization);
    }
    tx.rollback().map_err(|_| StoreError::Database)?;
    Ok(())
}
impl StateStore {
    /// Read one consistent private-key identity/Vault source on the existing bounded worker.
    /// Predicate must retain actual-operation I/O credit through SQL exit after caller cancellation.
    pub async fn load_key_identity_vault_source(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<KeyIdentityVaultSource> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::KeyIdentityVaultLoad(
            self.host_owner,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Confirm exact original metadata/ciphertext and current caller context without lending secrets.
    /// A successful source comparison never grants target-use permission or upgrades an unlock epoch.
    pub async fn validate_key_identity_vault_source(
        &self,
        expected: KeyIdentityVaultSource,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<()> {
        if expected.identities.owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::KeyIdentityVaultValidate(
            self.host_owner,
            expected,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
