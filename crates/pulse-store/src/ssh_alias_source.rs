//! Literal saved-entry alias lookup in a joint SQL capture. No use or connection permission.
use super::{
    Result, SshKeyTargetSource, SshRouteIdentity, StateStore, StoreError, default_identity,
    identity_exists,
};
use uuid::Uuid;

/// Exact metadata selected by one unique active saved entrance and its Profile default.
/// Does not authorize reading a credential, dialing or selecting a machine default entrance.
pub struct SavedSshAliasSelection {
    host: Uuid,
    profile: Uuid,
    identity: SshRouteIdentity,
}
impl SavedSshAliasSelection {
    /// Explicit entrance ID, never the machine ID.
    pub fn host_id(&self) -> Uuid {
        self.host
    }
    /// Exact Profile associated with that entrance in the same capture.
    pub fn profile_id(&self) -> Uuid {
        self.profile
    }
    /// Unique active identity type and ID. Not a credential-use authorization.
    pub fn identity(&self) -> SshRouteIdentity {
        self.identity
    }
    /// Selected saved identity ID.
    pub fn identity_id(&self) -> Uuid {
        match self.identity {
            SshRouteIdentity::Password(id)
            | SshRouteIdentity::PrivateKey(id)
            | SshRouteIdentity::Agent(id) => id,
        }
    }
}
pub(super) fn resolve(
    owner: Uuid,
    source: &SshKeyTargetSource,
    alias: &str,
) -> Result<SavedSshAliasSelection> {
    if source.hosts().owner != owner
        || source.is_temporary()
        || source.password_source().is_route_bound()
        || pulse_core::ssh_endpoint::SshEndpoint::new(alias, "alias.invalid", 22, "metadata")
            .is_err()
    {
        return Err(StoreError::HostConflict);
    }
    let mut entrances = source
        .hosts()
        .records()
        .iter()
        .filter(|r| r.endpoint().label() == alias);
    let entrance = entrances.next().ok_or(StoreError::HostConflict)?;
    if entrances.next().is_some() {
        return Err(StoreError::HostConflict);
    }
    let graph = source.hosts().catalogue().ok_or(StoreError::HostConflict)?;
    let mut profiles = graph
        .profiles()
        .iter()
        .filter(|p| !p.metadata().deleted() && p.endpoint_id() == entrance.id());
    let profile = profiles.next().ok_or(StoreError::HostConflict)?;
    if profiles.next().is_some() || profile.username() != entrance.endpoint().username() {
        return Err(StoreError::HostConflict);
    }
    let identity = default_identity(
        source,
        profile.identity_id().ok_or(StoreError::IdentityConflict)?,
    )?;
    if !identity_exists(source, identity, profile.username()) {
        return Err(StoreError::IdentityConflict);
    }
    Ok(SavedSshAliasSelection {
        host: entrance.id(),
        profile: profile.metadata().id(),
        identity,
    })
}
impl StateStore {
    /// Resolve only this store's original joint snapshot. No SQL reread or fallback by name/address.
    /// The caller must retain this capture and use the existing full-source CAS and approval chain.
    pub fn resolve_ssh_alias_source(
        &self,
        source: &SshKeyTargetSource,
        alias: &str,
    ) -> Result<SavedSshAliasSelection> {
        resolve(self.host_owner, source, alias)
    }
}
