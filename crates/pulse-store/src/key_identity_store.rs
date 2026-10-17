//! Private-key identity requests on the existing bounded SQLite worker. No plaintext or use permission.
use crate::{
    Request, Result, StateStore, StoreError,
    ssh_key_identities::{KeyIdentityEdit, KeyIdentitySnapshot},
};
use std::sync::Arc;
use tokio::sync::oneshot;
impl StateStore {
    /// Capture private-key metadata and both credential presences from one transaction.
    /// Predicate must retain actual-operation I/O credit until SQL exit, including after cancellation.
    pub async fn load_ssh_key_identities(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<KeyIdentitySnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::KeyIdentityLoad(self.host_owner, authorize, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Store-instance and source CAS for metadata only; never activates keys or authorizes SSH use.
    pub async fn edit_ssh_key_identities(
        &self,
        expected: KeyIdentitySnapshot,
        intent: KeyIdentityEdit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<KeyIdentitySnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::KeyIdentityEdit(
            self.host_owner,
            expected,
            intent,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
