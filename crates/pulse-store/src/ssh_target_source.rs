//! Optional pure storage capture for target-use preparation. No network/public-key parsing or plaintext.
use crate::{
    Request, Result, StateStore, StoreError,
    identity_vault_source::{self, IdentityVaultSource},
    ssh_hosts::{self, HostSnapshot},
    ssh_trust::{self, TrustSnapshot},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;
#[path = "ssh_temporary_target.rs"]
mod temporary_target;
pub use temporary_target::TemporarySshTarget;
/// All bounded sources, including the explicit Agent metadata directory, from one WAL view, including private store-instance provenance.
/// No key availability, peer proof, page or target-use permission is implied.
/// ```compile_fail
/// fn dump(v:pulse_store::ssh_target_source::SshTargetSource){println!("{v:?}");}
/// ```
/// ```compile_fail
/// fn send(v:pulse_store::ssh_target_source::SshTargetSource){let _=serde_json::to_string(&v);}
/// ```
#[derive(Clone)]
pub struct SshTargetSource {
    hosts: HostSnapshot,
    identity_vault: IdentityVaultSource,
    trust: TrustSnapshot,
    agents: crate::ssh_agent_identities::AgentIdentitySnapshot,
    temporary: Option<TemporarySshTarget>,
    route: Option<crate::ssh_route_source::RouteBinding>,
}
impl SshTargetSource {
    /// Frozen Agent metadata only; discovery, signing and peer authority remain separate.
    pub fn agent_identities(&self) -> &crate::ssh_agent_identities::AgentIdentitySnapshot {
        &self.agents
    }
    /// Frozen route association, not network or forwarding permission.
    pub fn is_route_bound(&self) -> bool {
        self.route.is_some()
    }
    /// Exact final route role only; plain sources and intermediate hops return false.
    pub fn is_route_leaf(&self) -> bool {
        self.route
            .as_ref()
            .is_some_and(|route| route.forward_target().is_none())
    }
    /// Frozen next hop only. None for direct sources and the final route entrance.
    /// Metadata does not authorize forwarding or credentials.
    pub fn forward_target(&self) -> Option<&pulse_core::ssh_target::SshTarget> {
        self.route
            .as_ref()
            .and_then(|binding| binding.forward_target())
    }
    pub(crate) fn first_pin_successor(
        &self,
        updated: &Self,
        target: &pulse_core::ssh_target::SshTarget,
    ) -> Result<Self> {
        // Only the exact opaque hop binding may carry a first-pin reply into the parent route.
        if self.route.is_none() || updated.route.is_none() {
            return Err(StoreError::IdentityConflict);
        }
        let mut normalized = updated.clone();
        normalized.trust = self.trust.clone();
        if !same(&normalized, self)? {
            return Err(StoreError::IdentityConflict);
        }
        let old = &self.trust;
        let new = &updated.trust;
        let exact_target = |pin: &ssh_trust::TrustTarget| {
            pin.host() == target.host() && pin.port() == target.port()
        };
        if old.revision.checked_add(1) != Some(new.revision)
            || old.owner != new.owner
            || old.rows.len().checked_add(1) != Some(new.rows.len())
            || old.rows.iter().any(|row| exact_target(&row.target))
            || !old
                .rows
                .iter()
                .all(|row| new.rows.iter().any(|next| next == row))
        {
            return Err(StoreError::HostTrustConflict);
        }
        let added = new
            .rows
            .iter()
            .find(|row| exact_target(&row.target))
            .ok_or(StoreError::HostTrustConflict)?;
        if added.deleted || added.key.is_none() {
            return Err(StoreError::HostTrustConflict);
        }
        let mut result = updated.clone();
        result.route = None;
        Ok(result)
    }
    pub(crate) fn bind_route(&mut self, binding: crate::ssh_route_source::RouteBinding) {
        self.route = Some(binding);
    }
    /// Exact metadata selection only; page, original epoch and actual-peer approval remain separate.
    pub fn supports_password_selection(&self, entrance: Uuid, identity: Uuid) -> bool {
        self.supports_selection(
            entrance,
            crate::ssh_route_source::SshRouteIdentity::Password(identity),
        )
    }
    pub(crate) fn supports_selection(
        &self,
        entrance: Uuid,
        identity: crate::ssh_route_source::SshRouteIdentity,
    ) -> bool {
        let id = match identity {
            crate::ssh_route_source::SshRouteIdentity::Password(id)
            | crate::ssh_route_source::SshRouteIdentity::PrivateKey(id)
            | crate::ssh_route_source::SshRouteIdentity::Agent(id) => id,
        };
        if !matches!(
            identity,
            crate::ssh_route_source::SshRouteIdentity::Agent(_)
        ) && self.agents.records().any(|record| record.id() == id)
        {
            return false;
        }
        self.route.as_ref().map_or_else(
            || self.hosts.supports_direct_ssh(entrance),
            |route| route.accepts(entrance, identity),
        )
    }
    /// Metadata-scope liveness only; this does not authorize a peer or credential use.
    pub fn target_is_current(&self) -> bool {
        self.temporary
            .as_ref()
            .is_none_or(TemporarySshTarget::is_current)
    }
    /// Temporary IDs are not persisted hosts and must not enter the saved-host MRU.
    pub fn is_temporary(&self) -> bool {
        self.temporary.is_some()
    }
    /// Carry metadata revocation into existing native guards without holding SQL I/O credit.
    /// A live callback is not peer, Vault, or credential-use authority.
    pub fn target_liveness(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let target = self.temporary.clone();
        Arc::new(move || target.as_ref().is_none_or(TemporarySshTarget::is_current))
    }
    /// Backend endpoint catalogue; IDs and username/addresses must not be logged or treated as permission.
    pub fn hosts(&self) -> &HostSnapshot {
        &self.hosts
    }
    /// Identity metadata and encrypted Vault from the same transaction; presence does not unlock use.
    pub fn identity_vault(&self) -> &IdentityVaultSource {
        &self.identity_vault
    }
    /// Unparsed trust records; upper SSH adapter must fail closed unless every key parses.
    pub fn trust(&self) -> &TrustSnapshot {
        &self.trust
    }
}
pub(crate) fn read(c: &Connection, owner: Uuid) -> Result<SshTargetSource> {
    read_for_target(c, owner, None)
}
pub(crate) fn reread(
    c: &Connection,
    owner: Uuid,
    expected: &SshTargetSource,
) -> Result<SshTargetSource> {
    match &expected.route {
        Some(route) => route.reread(c, owner),
        None => read_for_target(c, owner, expected.temporary.as_ref()),
    }
}
pub(crate) fn read_for_target(
    c: &Connection,
    owner: Uuid,
    temporary: Option<&TemporarySshTarget>,
) -> Result<SshTargetSource> {
    let hosts = match temporary {
        Some(target) => target.snapshot(owner)?,
        None => ssh_hosts::load(c, owner)?,
    };
    let result = SshTargetSource {
        hosts,
        identity_vault: identity_vault_source::load(c, owner)?,
        trust: ssh_trust::load(c, owner)?,
        agents: crate::ssh_agent_identities::load(c, owner)?,
        temporary: temporary.cloned(),
        route: None,
    };
    if !result.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    Ok(result)
}
fn load(c: &Connection, owner: Uuid) -> Result<SshTargetSource> {
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    read(&tx, owner)
}
pub(crate) fn same(current: &SshTargetSource, expected: &SshTargetSource) -> Result<bool> {
    if !current.target_is_current() || !expected.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    let same_target = match (&current.temporary, &expected.temporary) {
        (None, None) => true,
        (Some(a), Some(b)) => a.same_handle(b),
        _ => false,
    };
    let same_route = match (&current.route, &expected.route) {
        (None, None) => true,
        (Some(a), Some(b)) => a.same(b),
        _ => false,
    };
    Ok(same_route
        && same_target
        && current.agents == expected.agents
        && current.hosts.source_matches(&expected.hosts)
        && crate::ssh_identities::same_metadata(
            current.identity_vault.identities(),
            expected.identity_vault.identities(),
        )
        && crate::vault_database::same_capture(
            current.identity_vault.vault(),
            expected.identity_vault.vault(),
        )?
        && current.trust.rows == expected.trust.rows
        && current.trust.clock == expected.trust.clock
        && current.trust.revision() == expected.trust.revision()
        && current.trust.records().len() == expected.trust.records().len()
        && current
            .trust
            .records()
            .iter()
            .zip(expected.trust.records())
            .all(|(a, b)| a.target() == b.target() && a.key() == b.key()))
}
fn validate(
    c: &Connection,
    owner: Uuid,
    expected: &SshTargetSource,
    allowed: impl Fn() -> bool,
) -> Result<()> {
    if expected.hosts.owner != owner || expected.trust.owner != owner {
        return Err(StoreError::IdentityConflict);
    }
    if !allowed() || !expected.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    if !same(&reread(&tx, owner, expected)?, expected)? {
        return Err(StoreError::IdentityConflict);
    }
    if !allowed() || !expected.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    tx.rollback().map_err(|_| StoreError::Database)?;
    Ok(())
}
fn approve_first(
    c: &Connection,
    owner: Uuid,
    expected: &SshTargetSource,
    host: Uuid,
    record: ssh_trust::TrustRecord,
    allowed: impl Fn() -> bool,
) -> Result<SshTargetSource> {
    if expected.hosts.owner != owner || expected.trust.owner != owner {
        return Err(StoreError::IdentityConflict);
    }
    if !allowed() || !expected.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = reread(&tx, owner, expected)?;
    if !same(&current, expected)? {
        return Err(StoreError::IdentityConflict);
    }
    if current
        .route
        .as_ref()
        .is_some_and(|route| !route.accepts_entrance(host))
    {
        return Err(StoreError::HostConflict);
    }
    let endpoint = current
        .hosts
        .records()
        .iter()
        .find(|entry| entry.id() == host)
        .ok_or(StoreError::HostConflict)?
        .endpoint();
    if endpoint.target().host() != record.target().host()
        || endpoint.target().port() != record.target().port()
    {
        return Err(StoreError::HostTrustConflict);
    }
    ssh_trust::insert_first(&tx, owner, record)?;
    let result = reread(&tx, owner, expected)?;
    if !allowed() || !expected.target_is_current() {
        return Err(StoreError::IdentityAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(result)
}

pub(crate) enum SourceRequest {
    ApproveFirst {
        owner: Uuid,
        expected: Box<SshTargetSource>,
        host: Uuid,
        record: ssh_trust::TrustRecord,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<SshTargetSource>>,
    },
    Load {
        owner: Uuid,
        temporary: Option<TemporarySshTarget>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<SshTargetSource>>,
    },
    Validate {
        owner: Uuid,
        expected: Box<SshTargetSource>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<()>>,
    },
}
impl SourceRequest {
    pub(crate) fn execute(self, c: &Connection) {
        match self {
            Self::ApproveFirst {
                owner,
                expected,
                host,
                record,
                authorize,
                reply,
            } => {
                if reply.is_closed() {
                    return;
                }
                let result = approve_first(c, owner, &expected, host, record, || {
                    !reply.is_closed()
                        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                            .unwrap_or(false)
                });
                drop(authorize);
                let _sent = reply.send(result);
            }
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
                            tx.and_then(|tx| read_for_target(&tx, owner, Some(target)))
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
    /// Capture one temporary endpoint plus identity/Vault and trust in the existing SQL worker.
    /// No Host, Endpoint or Profile is saved and no credential/network permission is granted.
    pub async fn load_temporary_ssh_target_source(
        &self,
        target: TemporarySshTarget,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<SshTargetSource> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshTargetSource(Box::new(SourceRequest::Load {
            owner: self.host_owner,
            temporary: Some(target),
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Insert an absent pin at the exact captured endpoint under the whole source writer lock.
    /// Trusted upper layers must separately authorize explicit first-use approval and validate keys.
    /// This storage operation cannot replace a known target, activate keys or grant a connection.
    /// The final native predicate retains operation credit until actual SQL exit; no automatic retries.
    pub async fn approve_first_ssh_pin(
        &self,
        expected: SshTargetSource,
        host: Uuid,
        record: ssh_trust::TrustRecord,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<SshTargetSource> {
        if expected.hosts.owner != self.host_owner || expected.trust.owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshTargetSource(Box::new(
            SourceRequest::ApproveFirst {
                owner: self.host_owner,
                expected: Box::new(expected),
                host,
                record,
                authorize,
                reply,
            },
        )))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Read endpoint, identity/Vault and raw application trust in one bounded read transaction.
    /// Native authorization owns actual-operation I/O credit; canceled waits cannot release it early.
    pub async fn load_ssh_target_source(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<SshTargetSource> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshTargetSource(Box::new(SourceRequest::Load {
            owner: self.host_owner,
            temporary: None,
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Exact write-locked source check. No plaintext, key activation, or ongoing target-use permission.
    /// Callers must independently confirm original peer/page/key epoch; even successful validation is only a point receipt.
    pub async fn validate_ssh_target_source(
        &self,
        expected: SshTargetSource,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<()> {
        if expected.hosts.owner != self.host_owner || expected.trust.owner != self.host_owner {
            return Err(StoreError::IdentityConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshTargetSource(Box::new(
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
#[path = "ssh_target_source_lifecycle_tests.rs"]
mod lifecycle_tests;
#[cfg(test)]
#[path = "ssh_target_source_tests.rs"]
mod tests;
