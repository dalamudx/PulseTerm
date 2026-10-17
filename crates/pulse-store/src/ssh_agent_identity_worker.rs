use super::{AgentIdentityEdit, AgentIdentitySnapshot};
use crate::{Request, Result, StateStore, StoreError};
use rusqlite::Connection;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};
use tokio::sync::oneshot;
use uuid::Uuid;
pub(crate) enum AgentRequest {
    Load {
        owner: Uuid,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<AgentIdentitySnapshot>>,
    },
    Edit {
        owner: Uuid,
        expected: AgentIdentitySnapshot,
        change: AgentIdentityEdit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<AgentIdentitySnapshot>>,
    },
}
fn permitted(
    reply: &oneshot::Sender<Result<AgentIdentitySnapshot>>,
    authorize: &dyn Fn() -> bool,
) -> bool {
    !reply.is_closed() && catch_unwind(AssertUnwindSafe(authorize)).unwrap_or(false)
}
impl AgentRequest {
    pub(crate) fn serve(self, db: &Connection) {
        match self {
            Self::Load {
                owner,
                authorize,
                reply,
            } => {
                let result = if permitted(&reply, &*authorize) {
                    super::load(db, owner)
                } else {
                    Err(StoreError::IdentityAuthorization)
                };
                let result = if permitted(&reply, &*authorize) {
                    result
                } else {
                    Err(StoreError::IdentityAuthorization)
                };
                drop(authorize);
                let _sent = reply.send(result);
            }
            Self::Edit {
                owner,
                expected,
                change,
                authorize,
                reply,
            } => {
                let result = super::edit(db, owner, &expected, change, || {
                    permitted(&reply, &*authorize)
                });
                drop(authorize);
                let _sent = reply.send(result);
            }
        }
    }
}
impl StateStore {
    /// Read public Agent key metadata in the shared worker. Does not create tables or contact a socket.
    pub async fn load_ssh_agent_identities(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<AgentIdentitySnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::AgentIdentity(Box::new(AgentRequest::Load {
            owner: self.host_owner,
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Save or tombstone an explicitly selected Agent key using complete-source CAS and final authorization.
    /// This method grants neither Agent discovery/signing nor SSH target-use authority.
    pub async fn edit_ssh_agent_identity(
        &self,
        expected: AgentIdentitySnapshot,
        change: AgentIdentityEdit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<AgentIdentitySnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::AgentIdentity(Box::new(AgentRequest::Edit {
            owner: self.host_owner,
            expected,
            change,
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
