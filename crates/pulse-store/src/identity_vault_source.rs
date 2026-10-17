//! Native identity/reference metadata and ciphertext captured at one SQLite boundary.
use crate::{
    Request, Result, StateStore, StoreError,
    ssh_identities::{self, IdentitySnapshot},
    vault_database::{self, VaultSnapshot},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;
/// No plaintext, Debug/serialization or use permission; store instance remains private in both snapshots.
/// ```compile_fail
/// fn dump(s:pulse_store::identity_vault_source::IdentityVaultSource){println!("{s:?}");}
/// ```
/// ```compile_fail
/// fn send(s:pulse_store::identity_vault_source::IdentityVaultSource){let _=serde_json::to_string(&s);}
/// ```
#[derive(Clone)]
pub struct IdentityVaultSource {
    identities: IdentitySnapshot,
    vault: VaultSnapshot,
}
impl IdentityVaultSource {
    /// Bounded metadata with presence derived from the same ciphertext source; not key availability.
    pub fn identities(&self) -> &IdentitySnapshot {
        &self.identities
    }
    /// Backend-only encrypted source for subsequent exact validation, not activation or target permission.
    pub fn vault(&self) -> &VaultSnapshot {
        &self.vault
    }
}
fn read(c: &Connection, owner: Uuid) -> Result<IdentityVaultSource> {
    let identities = ssh_identities::load(c, owner)?;
    let vault = vault_database::load(c, owner)?;
    if identities.vault_id() != vault.vault_id() {
        return Err(StoreError::IdentityRecord);
    }
    Ok(IdentityVaultSource { identities, vault })
}
pub(crate) fn load(c: &Connection, owner: Uuid) -> Result<IdentityVaultSource> {
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
    expected: &IdentityVaultSource,
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
    if !ssh_identities::same_metadata(&current.identities, &expected.identities)
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

pub(crate) struct PasswordEditRequest {
    owner: Uuid,
    expected: IdentityVaultSource,
    identity: Uuid,
    edit: vault_database::VaultEdit,
    scope: crate::vault_lifecycle::VaultKeyLifecycle,
    epoch: crate::vault_lifecycle::AvailableKeyEpoch,
    authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    reply: oneshot::Sender<Result<VaultSnapshot>>,
}
impl PasswordEditRequest {
    pub(crate) fn execute(self, c: &Connection) {
        if self.reply.is_closed() {
            return;
        }
        let allowed = || {
            !self.reply.is_closed()
                && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.authorize)()))
                    .unwrap_or(false)
                && self.epoch.is_current_for(&self.scope).unwrap_or(false)
        };
        let result = (|| {
            if self.expected.identities.owner != self.owner {
                return Err(StoreError::IdentityConflict);
            }
            let credential = match &self.edit {
                vault_database::VaultEdit::Create(r) | vault_database::VaultEdit::Replace(r)
                    if r.kind() == "ssh_password" =>
                {
                    r.id()
                }
                vault_database::VaultEdit::Remove(id) => *id,
                _ => return Err(StoreError::IdentityConflict),
            };
            vault_database::edit_preflight(
                c,
                self.owner,
                &self.expected.vault,
                self.edit,
                allowed,
                |tx, current_vault| {
                    let identity = ssh_identities::load(tx, self.owner)?;
                    if !ssh_identities::same_metadata(&identity, &self.expected.identities)
                        || !vault_database::same_source(current_vault, &self.expected.vault)?
                    {
                        return Err(StoreError::IdentityConflict);
                    }
                    let row = identity
                        .records()
                        .find(|r| r.id() == self.identity)
                        .ok_or(StoreError::IdentityConflict)?;
                    let reference = row.identity().password();
                    if reference.id() != credential
                        || Some(reference.vault_id()) != current_vault.vault_id()
                        || self.scope.expected_id()? != reference.vault_id()
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
    /// Ciphertext-only password edit tied to one saved identity/source and available key epoch.
    /// Trusted caller must seal under this scope and independently authorize current page/edit intent.
    /// No KDF/plaintext reaches SQL; canceled waits retain predicate credit until native execution exits.
    pub async fn edit_identity_password_ciphertext(
        &self,
        expected: IdentityVaultSource,
        identity: Uuid,
        edit: vault_database::VaultEdit,
        scope: crate::vault_lifecycle::VaultKeyLifecycle,
        epoch: crate::vault_lifecycle::AvailableKeyEpoch,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<VaultSnapshot> {
        if expected.identities.owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::IdentityPasswordEdit(Box::new(
            PasswordEditRequest {
                owner: self.host_owner,
                expected,
                identity,
                edit,
                scope,
                epoch,
                authorize,
                reply,
            },
        )))?;
        result.await.map_err(|_| StoreError::Closed)?
    }

    /// Validate exact native source at one write-locked boundary, not ongoing use/target permission.
    /// Caller authorization must also verify current page/owner/epoch; no key or plaintext is returned.
    pub async fn validate_identity_vault_source(
        &self,
        expected: IdentityVaultSource,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<()> {
        if expected.identities.owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::IdentityVaultValidate(
            self.host_owner,
            expected,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Same-transaction bounded native read; predicate must own actual-operation I/O credit until exit.
    /// Cancellation never supplies fallback values or releases a currently executing SQLite callback early.
    pub async fn load_identity_vault_source(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<IdentityVaultSource> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::IdentityVaultLoad(
            self.host_owner,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
