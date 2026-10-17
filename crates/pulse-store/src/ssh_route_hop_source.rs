//! Private, exact-hop metadata binding. This is not route or peer-use authorization.
use super::{SshRouteIdentity, SshRouteSource};
use crate::{
    Result, StoreError,
    ssh_key_identities::{self, KeyIdentitySnapshot},
    ssh_key_target_source::SshKeyTargetSource,
    ssh_target_source::SshTargetSource,
};
use rusqlite::Connection;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct RouteBinding {
    destination: Uuid,
    leaf_identity: SshRouteIdentity,
    index: usize,
    hops: Vec<super::SshRouteHop>,
    keys: KeyIdentitySnapshot,
}
impl RouteBinding {
    pub(crate) fn forward_target(&self) -> Option<&pulse_core::ssh_target::SshTarget> {
        self.hops
            .get(self.index + 1)
            .map(|hop| hop.endpoint().target())
    }
    pub(crate) fn accepts(&self, entrance: Uuid, identity: SshRouteIdentity) -> bool {
        self.hops
            .get(self.index)
            .is_some_and(|hop| hop.entrance_id() == entrance && hop.identity() == identity)
    }
    pub(crate) fn accepts_entrance(&self, entrance: Uuid) -> bool {
        self.hops
            .get(self.index)
            .is_some_and(|hop| hop.entrance_id() == entrance)
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.destination == other.destination
            && self.leaf_identity == other.leaf_identity
            && self.index == other.index
            && self.hops == other.hops
            && ssh_key_identities::same_metadata(&self.keys, &other.keys)
    }
    pub(crate) fn reread(&self, c: &Connection, owner: Uuid) -> Result<SshTargetSource> {
        // The caller already holds the transaction. No separate preflight or nested transaction.
        let route = super::read(c, owner, self.destination, self.leaf_identity)?;
        Ok(route.hop_source(self.index)?.password_source().clone())
    }
}
impl SshRouteSource {
    /// Accept only the exact hop's opaque first-pin reply. All non-trust source data and old pins
    /// remain frozen. The upper adapter must still bind approval to the actual connection.
    pub fn after_first_pin(&self, index: usize, updated: &SshTargetSource) -> Result<Self> {
        let expected = self.hop_source(index)?;
        let target = self
            .hops
            .get(index)
            .ok_or(StoreError::HostConflict)?
            .endpoint()
            .target();
        let common = expected
            .password_source()
            .first_pin_successor(updated, target)?;
        let mut result = self.clone();
        result.source = result.source.with_common(common);
        Ok(result)
    }
}
impl SshRouteSource {
    /// Derive metadata for exactly one frozen hop, carrying the whole route and both directories.
    /// This neither advances a route nor grants dialing, actual-peer proof or secret use.
    /// Existing page/original-epoch actions and connection-bound protocol sinks remain required.
    pub fn hop_source(&self, index: usize) -> Result<SshKeyTargetSource> {
        if self.hops.get(index).is_none() {
            return Err(StoreError::HostConflict);
        }
        let mut common = self.source.password_source().clone();
        common.bind_route(RouteBinding {
            destination: self.destination,
            leaf_identity: self.leaf_identity,
            index,
            hops: self.hops.clone(),
            keys: self.source.identities().clone(),
        });
        Ok(self.source.clone().with_common(common))
    }
}
