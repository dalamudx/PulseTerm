//! Identity metadata requests on the existing bounded worker. No plaintext or use permission.
use crate::{
    Request, Result, StateStore, StoreError,
    ssh_identities::{IdentityEdit, IdentitySnapshot},
};
use std::sync::Arc;
use tokio::sync::oneshot;
impl StateStore {
    /// Read one consistent identity/Vault source. Predicate must own actual-operation I/O credit.
    /// Cancellation drops queued work; active native work retains its predicate until exit.
    pub async fn load_ssh_identities(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<IdentitySnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::IdentityLoad(self.host_owner, authorize, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Current-source metadata CAS only; no key activation or SSH credential-use authorization.
    pub async fn edit_ssh_identities(
        &self,
        expected: IdentitySnapshot,
        intent: IdentityEdit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<IdentitySnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::IdentityEdit(
            self.host_owner,
            expected,
            intent,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
