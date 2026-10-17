//! Existing bounded blocking worker admission, not a page or network entry.
use crate::{
    Request, Result, StateStore, StoreError,
    ssh_hosts::{HostEdit, HostSnapshot},
};
use std::sync::Arc;
use tokio::sync::oneshot;
impl StateStore {
    /// Page adapters hold their read reservation in this execution predicate, not in
    /// the waiting future. Revocation before/after SQL rejects publication; DB stays open.
    pub async fn load_ssh_hosts_authorized(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<HostSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::HostsLoad(self.host_owner, Some(authorize), reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }

    /// Read all validated endpoint metadata. Corruption must not become an empty host list.
    pub async fn load_ssh_hosts(&self) -> Result<HostSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::HostsLoad(self.host_owner, None, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// CAS edit with caller-owned backend admission checked in the blocking SQL transaction.
    /// Hold operation credit in this closure until actual exit. No automatic retry/rollback.
    pub async fn edit_ssh_hosts(
        &self,
        expected: HostSnapshot,
        edit: HostEdit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<HostSnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::HostConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::HostsEdit(
            self.host_owner,
            expected,
            edit,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
