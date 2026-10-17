//! Atomic saved SSH jump metadata. No dialing, plaintext, peer proof or credential-use permission.
use crate::{
    Request, Result, StateStore, StoreError, ssh_key_identities, ssh_key_target_source,
    ssh_key_target_source::SshKeyTargetSource, ssh_target_source,
};
use pulse_core::{host_catalogue::EndpointProtocol, ssh_endpoint::SshEndpoint};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;
#[path = "ssh_route_hop_source.rs"]
mod hop_source;
pub(crate) use hop_source::RouteBinding;

#[path = "ssh_alias_source.rs"]
mod alias;
pub use alias::SavedSshAliasSelection;
/// An explicit identity kind and saved ID. It does not authorize reading or using a secret.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SshRouteIdentity {
    /// Password identity metadata.
    Password(Uuid),
    /// Private-key identity metadata.
    PrivateKey(Uuid),
    /// Explicit saved local Agent public-key metadata. Never enables Agent forwarding.
    Agent(Uuid),
}

/// A frozen saved entrance, including its exact username and chosen identity.
#[derive(Clone, PartialEq, Eq)]
pub struct SshRouteHop {
    profile: Uuid,
    entrance: Uuid,
    endpoint: SshEndpoint,
    identity: SshRouteIdentity,
}
impl SshRouteHop {
    /// The exact saved Profile, never a machine's implicit default entrance.
    pub fn profile_id(&self) -> Uuid {
        self.profile
    }
    /// The original saved entrance ID, including at intermediate hops.
    pub fn entrance_id(&self) -> Uuid {
        self.entrance
    }
    /// Frozen canonical destination and login metadata; no socket is opened.
    pub fn endpoint(&self) -> &SshEndpoint {
        &self.endpoint
    }
    /// Explicit kind and reference. Presence here is not secret availability.
    pub fn identity(&self) -> SshRouteIdentity {
        self.identity
    }
}

/// One WAL view of all entrances, Profile edges, all three identity directories, ciphertext and raw trust.
/// This source cannot be serialized or Debug-formatted. Actual peers, page/epoch admission and
/// per-hop credential-use actions must still be independently bound by the protocol adapter.
/// ```compile_fail
/// fn dump(s: pulse_store::ssh_route_source::SshRouteSource) { println!("{s:?}"); }
/// ```
/// ```compile_fail
/// fn send(s: pulse_store::ssh_route_source::SshRouteSource) { let _ = serde_json::to_string(&s); }
/// ```
#[derive(Clone)]
pub struct SshRouteSource {
    owner: Uuid,
    destination: Uuid,
    leaf_identity: SshRouteIdentity,
    source: SshKeyTargetSource,
    hops: Vec<SshRouteHop>,
}
impl SshRouteSource {
    /// Outermost first, destination last; at most eight jumps and one final entrance.
    pub fn hops(&self) -> &[SshRouteHop] {
        &self.hops
    }
    /// Joint encrypted/metadata source only. Existing direct-path guards remain in force.
    pub fn target_source(&self) -> &SshKeyTargetSource {
        &self.source
    }
}

fn identity_exists(
    source: &SshKeyTargetSource,
    identity: SshRouteIdentity,
    username: &str,
) -> bool {
    let id = match identity {
        SshRouteIdentity::Password(id)
        | SshRouteIdentity::PrivateKey(id)
        | SshRouteIdentity::Agent(id) => id,
    };
    if default_identity(source, id).ok() != Some(identity) {
        return false;
    }
    match identity {
        SshRouteIdentity::Password(id) => source
            .password_source()
            .identity_vault()
            .identities()
            .records()
            .any(|r| r.id() == id && r.identity().username() == username),
        SshRouteIdentity::PrivateKey(id) => source
            .identities()
            .records()
            .any(|r| r.id() == id && r.identity().username() == username),
        SshRouteIdentity::Agent(id) => source
            .password_source()
            .agent_identities()
            .records()
            .any(|r| r.id() == id && r.identity().username() == username),
    }
}
fn default_identity(source: &SshKeyTargetSource, id: Uuid) -> Result<SshRouteIdentity> {
    let password = source
        .password_source()
        .identity_vault()
        .identities()
        .records()
        .any(|r| r.id() == id);
    let key = source.identities().records().any(|r| r.id() == id);
    let agent = source
        .password_source()
        .agent_identities()
        .records()
        .any(|r| r.id() == id);
    match (password, key, agent) {
        (true, false, false) => Ok(SshRouteIdentity::Password(id)),
        (false, true, false) => Ok(SshRouteIdentity::PrivateKey(id)),
        (false, false, true) => Ok(SshRouteIdentity::Agent(id)),
        _ => Err(StoreError::IdentityConflict),
    }
}
fn read(
    c: &Connection,
    owner: Uuid,
    destination: Uuid,
    leaf_identity: SshRouteIdentity,
) -> Result<SshRouteSource> {
    // These readers do not start nested transactions: the caller owns the whole WAL view.
    let source = ssh_key_target_source::finish_read(c, owner, ssh_target_source::read(c, owner)?)?;
    resolve(owner, source, destination, leaf_identity)
}
fn resolve(
    owner: Uuid,
    source: SshKeyTargetSource,
    destination: Uuid,
    leaf_identity: SshRouteIdentity,
) -> Result<SshRouteSource> {
    if source.hosts().owner != owner
        || source.is_temporary()
        || source.password_source().is_route_bound()
    {
        return Err(StoreError::IdentityConflict);
    }
    let graph = source.hosts().catalogue().ok_or(StoreError::HostConflict)?;
    let destination_profile = graph
        .profiles()
        .iter()
        .find(|p| p.metadata().id() == destination && !p.metadata().deleted())
        .ok_or(StoreError::HostConflict)?;
    let mut profiles = graph
        .ssh_jump_chain(destination)
        .map_err(|_| StoreError::HostConflict)?;
    // This API describes configured jumps only; a missing route cannot quietly become a direct dial.
    if profiles.is_empty() {
        return Err(StoreError::HostConflict);
    }
    profiles.push(destination_profile);
    let mut hops = Vec::with_capacity(profiles.len());
    for profile in profiles {
        let entrance = source
            .hosts()
            .records()
            .iter()
            .find(|r| r.id() == profile.endpoint_id())
            .ok_or(StoreError::HostConflict)?;
        let endpoint = graph
            .endpoints()
            .iter()
            .find(|e| e.metadata().id() == entrance.id() && !e.metadata().deleted())
            .ok_or(StoreError::HostConflict)?;
        if endpoint.protocol() != EndpointProtocol::Ssh
            || endpoint.target() != entrance.endpoint().target()
            || profile.username() != entrance.endpoint().username()
        {
            return Err(StoreError::HostConflict);
        }
        let identity = if profile.metadata().id() == destination {
            leaf_identity
        } else {
            default_identity(
                &source,
                profile.identity_id().ok_or(StoreError::IdentityConflict)?,
            )?
        };
        if !identity_exists(&source, identity, profile.username()) {
            return Err(StoreError::IdentityConflict);
        }
        hops.push(SshRouteHop {
            profile: profile.metadata().id(),
            entrance: entrance.id(),
            endpoint: entrance.endpoint().clone(),
            identity,
        });
    }
    Ok(SshRouteSource {
        owner,
        destination,
        leaf_identity,
        source,
        hops,
    })
}
fn load(
    c: &Connection,
    owner: Uuid,
    destination: Uuid,
    identity: SshRouteIdentity,
) -> Result<SshRouteSource> {
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    let result = read(&tx, owner, destination, identity)?;
    tx.rollback().map_err(|_| StoreError::Database)?;
    Ok(result)
}
fn validate(
    c: &Connection,
    owner: Uuid,
    expected: &SshRouteSource,
    allowed: impl Fn() -> bool,
) -> Result<()> {
    if expected.owner != owner {
        return Err(StoreError::IdentityConflict);
    }
    if !allowed() {
        return Err(StoreError::IdentityAuthorization);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = read(&tx, owner, expected.destination, expected.leaf_identity)?;
    if current.hops != expected.hops
        || !ssh_target_source::same(
            current.source.password_source(),
            expected.source.password_source(),
        )?
        || !ssh_key_identities::same_metadata(
            current.source.identities(),
            expected.source.identities(),
        )
    {
        return Err(StoreError::IdentityConflict);
    }
    if !allowed() {
        return Err(StoreError::IdentityAuthorization);
    }
    tx.rollback().map_err(|_| StoreError::Database)?;
    Ok(())
}

pub(crate) enum SourceRequest {
    Load {
        owner: Uuid,
        destination: Uuid,
        identity: SshRouteIdentity,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<SshRouteSource>>,
    },
    Validate {
        owner: Uuid,
        expected: Box<SshRouteSource>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        reply: oneshot::Sender<Result<()>>,
    },
}
fn authorized<T>(
    reply: &oneshot::Sender<Result<T>>,
    authorize: &Arc<dyn Fn() -> bool + Send + Sync>,
) -> bool {
    !reply.is_closed()
        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize())).unwrap_or(false)
}
impl SourceRequest {
    pub(crate) fn execute(self, c: &Connection) {
        match self {
            Self::Load {
                owner,
                destination,
                identity,
                authorize,
                reply,
            } => {
                if reply.is_closed() {
                    return;
                }
                let result = if authorized(&reply, &authorize) {
                    load(c, owner, destination, identity)
                } else {
                    Err(StoreError::IdentityAuthorization)
                };
                let result = if authorized(&reply, &authorize) {
                    result
                } else {
                    Err(StoreError::IdentityAuthorization)
                };
                // Caller I/O credit lives in this predicate until the actual SQL operation has exited.
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
                let result = validate(c, owner, &expected, || authorized(&reply, &authorize));
                drop(authorize);
                let _sent = reply.send(result);
            }
        }
    }
}
impl StateStore {
    /// Resolve a saved jump chain from the already captured joint SQL view, without rereading it.
    /// Rejects foreign-store, temporary and derived-hop sources. Metadata grants no use authority.
    pub fn resolve_ssh_route_source(
        &self,
        source: SshKeyTargetSource,
        destination: Uuid,
        identity: SshRouteIdentity,
    ) -> Result<SshRouteSource> {
        resolve(self.host_owner, source, destination, identity)
    }
    /// Capture one configured saved jump chain. Does not open sockets or authorize credential use.
    /// The final identity is explicit; intermediate defaults must be active, unique and username-matched.
    pub async fn load_ssh_route_source(
        &self,
        destination: Uuid,
        identity: SshRouteIdentity,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<SshRouteSource> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshRouteSource(Box::new(SourceRequest::Load {
            owner: self.host_owner,
            destination,
            identity,
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Recheck the complete original chain, both metadata directories, ciphertext and raw trust.
    /// This comparison cannot be upgraded into an execution, peer or identity-use receipt.
    pub async fn validate_ssh_route_source(
        &self,
        expected: SshRouteSource,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshRouteSource(Box::new(SourceRequest::Validate {
            owner: self.host_owner,
            expected: Box::new(expected),
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}

#[cfg(test)]
#[path = "ssh_route_source_tests.rs"]
mod tests;
