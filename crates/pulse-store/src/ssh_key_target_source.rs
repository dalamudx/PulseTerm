//! Atomic private-key target metadata capture. No peer proof, plaintext or credential-use permission.
use crate::{
    Request, Result, StateStore, StoreError,
    ssh_hosts::HostSnapshot,
    ssh_key_identities::{self, KeyIdentitySnapshot},
    ssh_target_source::{self, SshTargetSource, TemporarySshTarget},
    ssh_trust::TrustSnapshot,
    vault_database::VaultSnapshot,
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;

/// Instance-bound endpoint, private-key metadata, ciphertext and raw trust from one WAL view.
/// The password directory is also frozen by the common capture; this object grants no permission.
/// ```compile_fail
/// fn log(s: pulse_store::ssh_key_target_source::SshKeyTargetSource) { println!("{s:?}"); }
/// ```
/// ```compile_fail
/// fn send(s: pulse_store::ssh_key_target_source::SshKeyTargetSource) { let _ = serde_json::to_string(&s); }
/// ```
#[derive(Clone)]
pub struct SshKeyTargetSource {
    common: SshTargetSource,
    identities: KeyIdentitySnapshot,
}
impl SshKeyTargetSource {
    pub(crate) fn with_common(mut self, common: SshTargetSource) -> Self {
        self.common = common;
        self
    }
    /// Explicit interactive account metadata only. This grants no password read, peer or use authority.
    pub fn supports_interactive_selection(&self, entrance: Uuid, identity: Uuid) -> bool {
        !self.common.is_route_bound() && self.supports_interactive_account(entrance, identity)
    }
    /// Frozen exact-hop metadata only; no peer, use or execution permission.
    pub fn supports_route_interactive_selection(&self, entrance: Uuid, identity: Uuid) -> bool {
        self.common.is_route_bound() && self.supports_interactive_account(entrance, identity)
    }
    fn supports_interactive_account(&self, entrance: Uuid, identity: Uuid) -> bool {
        self.common.supports_password_selection(entrance, identity)
            && !self.identities.records().any(|row| row.id() == identity)
            && self
                .common
                .identity_vault()
                .identities()
                .records()
                .any(|row| {
                    row.id() == identity
                        && self.common.hosts().records().iter().any(|host| {
                            host.id() == entrance
                                && host.endpoint().username() == row.identity().username()
                        })
                })
    }
    /// Frozen hop/identity matching only, not actual-peer or credential-use authority.
    pub fn supports_private_key_selection(&self, entrance: Uuid, identity: Uuid) -> bool {
        self.common.supports_selection(
            entrance,
            crate::ssh_route_source::SshRouteIdentity::PrivateKey(identity),
        )
    }
    /// Exact typed hop or direct Agent selection. Ordinary jump entries cannot become direct.
    pub fn supports_agent_selection(&self, entrance: Uuid, identity: Uuid) -> bool {
        self.common.supports_selection(
            entrance,
            crate::ssh_route_source::SshRouteIdentity::Agent(identity),
        ) && !self.identities.records().any(|row| row.id() == identity)
            && !self
                .common
                .identity_vault()
                .identities()
                .records()
                .any(|row| row.id() == identity)
            && self.common.agent_identities().records().any(|row| {
                row.id() == identity
                    && self.common.hosts().records().iter().any(|host| {
                        host.id() == entrance
                            && host.endpoint().username() == row.identity().username()
                    })
            })
    }
    /// Metadata-only admission for an explicit saved direct Agent policy, never a route upgrade.
    pub fn supports_direct_agent_selection(&self, entrance: Uuid, identity: Uuid) -> bool {
        !self.common.is_route_bound()
            && !self.is_temporary()
            && self.supports_agent_selection(entrance, identity)
    }
    /// Distinguish device-local saved targets from unsaved metadata, without granting use.
    pub fn is_temporary(&self) -> bool {
        self.common.is_temporary()
    }
    /// Metadata-scope liveness; does not grant peer or private-key use.
    pub fn target_liveness(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        self.common.target_liveness()
    }
    /// Saved endpoints; their identifiers and addresses do not grant network access.
    pub fn hosts(&self) -> &HostSnapshot {
        self.common.hosts()
    }
    /// Private-key references with presence derived in the same transaction; not available keys.
    pub fn identities(&self) -> &KeyIdentitySnapshot {
        &self.identities
    }
    /// Encrypted source only. Original epoch and explicit target-use authorization remain separate.
    pub fn vault(&self) -> &VaultSnapshot {
        self.common.identity_vault().vault()
    }
    /// Raw trust records. An upper protocol adapter must parse keys and bind the actual peer.
    pub fn trust(&self) -> &TrustSnapshot {
        self.common.trust()
    }
    /// Frozen password metadata and ciphertext from the same transaction as the key directory.
    /// This bounded source does not grant peer proof, key availability or credential use.
    pub fn password_source(&self) -> &SshTargetSource {
        &self.common
    }
}
fn owned_by(source: &SshKeyTargetSource, owner: Uuid) -> bool {
    source.common.hosts().owner == owner
        && source.common.trust().owner == owner
        && source.identities.owner == owner
}
fn read(c: &Connection, owner: Uuid) -> Result<SshKeyTargetSource> {
    finish_read(c, owner, ssh_target_source::read(c, owner)?)
}
pub(crate) fn finish_read(
    c: &Connection,
    owner: Uuid,
    common: SshTargetSource,
) -> Result<SshKeyTargetSource> {
    let identities = ssh_key_identities::load(c, owner)?;
    if identities.vault_id() != common.identity_vault().vault().vault_id() {
        return Err(StoreError::IdentityRecord);
    }
    if !common.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    Ok(SshKeyTargetSource { common, identities })
}
fn load(c: &Connection, owner: Uuid) -> Result<SshKeyTargetSource> {
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    read(&tx, owner)
}
fn validate(
    c: &Connection,
    owner: Uuid,
    expected: &SshKeyTargetSource,
    allowed: impl Fn() -> bool,
) -> Result<()> {
    if !owned_by(expected, owner) {
        return Err(StoreError::IdentityConflict);
    }
    if !allowed() || !expected.common.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = finish_read(
        &tx,
        owner,
        ssh_target_source::reread(&tx, owner, &expected.common)?,
    )?;
    if !ssh_target_source::same(&current.common, &expected.common)?
        || !ssh_key_identities::same_metadata(&current.identities, &expected.identities)
    {
        return Err(StoreError::IdentityConflict);
    }
    if !allowed() || !expected.common.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    tx.rollback().map_err(|_| StoreError::Database)?;
    Ok(())
}
pub(crate) enum SourceRequest {
    Load {
        owner: Uuid,
        temporary: Option<TemporarySshTarget>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<SshKeyTargetSource>>,
    },
    Validate {
        owner: Uuid,
        expected: Box<SshKeyTargetSource>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<()>>,
    },
}
impl SourceRequest {
    pub(crate) fn execute(self, c: &Connection) {
        match self {
            Self::Load {
                owner,
                temporary,
                authorize,
                reply,
            } => {
                if reply.is_closed() {
                    return;
                }
                let allowed = || {
                    !reply.is_closed()
                        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                            .unwrap_or(false)
                };
                let result = if allowed() {
                    match temporary.as_ref() {
                        None => load(c, owner),
                        Some(target) => {
                            let tx = Transaction::new_unchecked(c, TransactionBehavior::Deferred)
                                .map_err(|_| StoreError::Database);
                            tx.and_then(|tx| {
                                let common =
                                    ssh_target_source::read_for_target(&tx, owner, Some(target))?;
                                finish_read(&tx, owner, common)
                            })
                        }
                    }
                } else {
                    Err(StoreError::IdentityAuthorization)
                };
                let result = if allowed() {
                    result
                } else {
                    Err(StoreError::IdentityAuthorization)
                };
                drop(authorize);
                let _sent = reply.send(result);
            }
            Self::Validate {
                owner,
                expected,
                authorize,
                reply,
            } => {
                if reply.is_closed() {
                    return;
                }
                let result = validate(c, owner, &expected, || {
                    !reply.is_closed()
                        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                            .unwrap_or(false)
                });
                drop(authorize);
                let _sent = reply.send(result);
            }
        }
    }
}
impl StateStore {
    /// Capture temporary target, private-key metadata, ciphertext and trust from one WAL view.
    /// Caller authorization retains actual SQL I/O credit; the target itself grants no key/peer use.
    pub async fn load_temporary_ssh_key_target_source(
        &self,
        target: TemporarySshTarget,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<SshKeyTargetSource> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshKeyTargetSource(Box::new(SourceRequest::Load {
            owner: self.host_owner,
            temporary: Some(target),
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Capture all saved private-key target sources together on the existing worker.
    /// The native predicate must own I/O credit until actual SQL exit, including after cancellation.
    pub async fn load_ssh_key_target_source(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<SshKeyTargetSource> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshKeyTargetSource(Box::new(SourceRequest::Load {
            owner: self.host_owner,
            temporary: None,
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Compare the exact original source under the writer lock and recheck caller authorization.
    /// Success is a point comparison, never a peer, epoch, plaintext or continuing-use authority.
    pub async fn validate_ssh_key_target_source(
        &self,
        expected: SshKeyTargetSource,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<()> {
        if !owned_by(&expected, self.host_owner) {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshKeyTargetSource(Box::new(
            SourceRequest::Validate {
                owner: self.host_owner,
                expected: Box::new(expected),
                authorize,
                reply,
            },
        )))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
#[cfg(test)]
#[path = "ssh_key_target_source_tests.rs"]
mod tests;
