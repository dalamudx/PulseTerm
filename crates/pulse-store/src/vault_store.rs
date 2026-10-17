//! Ciphertext-only requests on existing bounded worker; no KDF or plaintext on SQL thread.
use crate::{
    Request, Result, StateStore, StoreError,
    vault_database::{VaultEdit, VaultSnapshot},
};
use std::sync::Arc;
use tokio::sync::oneshot;
impl StateStore {
    /// Joint native confirmation against exact current ciphertext source and lifecycle epoch.
    /// Consumes the keyless ready receipt. Cancellation/lost delivery revokes its activation;
    /// native authorization owns I/O credit until exit and must revoke the lifecycle on owner retirement.
    /// This is not window/SecretRef permission, authentication or plaintext IPC.
    pub async fn confirm_vault_unlock(
        &self,
        expected: VaultSnapshot,
        scope: crate::vault_lifecycle::VaultKeyLifecycle,
        ready: crate::vault_unlock::ReadyUnlock,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<()> {
        if expected.owner != self.host_owner {
            return Err(StoreError::VaultConflict);
        }
        let cancellation = ready.confirmation_cancel();
        let (reply, result) = oneshot::channel();
        self.admit(Request::VaultConfirm(
            self.host_owner,
            expected,
            scope,
            ready,
            authorize,
            reply,
        ))?;
        let activation = result.await.map_err(|_| StoreError::Closed)??;
        activation.accept();
        cancellation.accept();
        Ok(())
    }
    /// Trusted native source read. Keep actual-operation I/O credit inside this predicate.
    /// A missing row is an absent Vault, never a reason to generate a replacement key.
    pub async fn load_vault_ciphertext(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<VaultSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::VaultLoad(self.host_owner, authorize, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Ciphertext-only CAS edit, authorized again on actual blocking transaction stages.
    /// Lost receipt or cancelled waiter is not rollback of an already committed transaction.
    pub async fn edit_vault_ciphertext(
        &self,
        expected: VaultSnapshot,
        edit: VaultEdit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<VaultSnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::VaultConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::VaultEdit(
            self.host_owner,
            expected,
            edit,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
