//! Single-worker admission for optional SSH trust. Never a page IPC or approval grant.
use crate::{
    Request, Result, StateStore, StoreError,
    ssh_trust::{TrustEdit, TrustSnapshot},
};
use tokio::sync::oneshot;
impl StateStore {
    /// Read a validated, revisioned trust snapshot. Errors must not become an empty pin list.
    pub async fn load_ssh_trust(&self) -> Result<TrustSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::HostTrustLoad(self.trust_owner, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Commit a trust change only after a separate backend-confirmed user decision.
    /// This trusted Rust interface does not prove approval; there is no page or IPC caller.
    /// A lost reply is never retried implicitly. Callers must reload after uncertain commit.
    pub async fn edit_ssh_trust(
        &self,
        expected: TrustSnapshot,
        edit: TrustEdit,
    ) -> Result<TrustSnapshot> {
        if expected.owner != self.trust_owner {
            return Err(StoreError::HostTrustConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::HostTrustEdit(
            self.trust_owner,
            expected,
            edit,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
