//! Revocable, backend-issued temporary endpoint metadata. No SQL write or connection permission.
#[cfg(test)]
#[path = "ssh_temporary_target_tests.rs"]
mod tests;
use crate::{Result, StateStore, StoreError, ssh_hosts::HostSnapshot};
use pulse_core::ssh_endpoint::SshEndpoint;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use uuid::Uuid;
struct Target {
    owner: Uuid,
    id: Uuid,
    endpoint: SshEndpoint,
    deadline: Instant,
    revoked: AtomicBool,
    scope: Arc<dyn Fn() -> bool + Send + Sync>,
}
/// Private store-instance and original-page metadata scope. Clones share revocation.
/// This cannot be serialized, logged with Debug, or used as peer or credential approval.
/// ```compile_fail
/// fn send(t: pulse_store::ssh_target_source::TemporarySshTarget) { let _ = serde_json::to_string(&t); }
/// ```
#[derive(Clone)]
pub struct TemporarySshTarget(Arc<Target>);
impl TemporarySshTarget {
    /// Backend-generated display identity; this UUID does not authorize startup.
    pub fn id(&self) -> Uuid {
        self.0.id
    }
    /// Validated endpoint metadata only, never a credential or network capability.
    pub fn endpoint(&self) -> &SshEndpoint {
        &self.0.endpoint
    }
    /// Cancel all copies of this metadata source.
    pub fn revoke(&self) {
        self.0.revoked.store(true, Ordering::Release);
    }
    /// Metadata-scope liveness only; this point check grants no credential or network use.
    pub fn is_current(&self) -> bool {
        !self.0.revoked.load(Ordering::Acquire)
            && Instant::now() < self.0.deadline
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.0.scope)()))
                .unwrap_or(false)
    }
    pub(crate) fn same_handle(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub(crate) fn snapshot(&self, owner: Uuid) -> Result<HostSnapshot> {
        if owner != self.0.owner {
            return Err(StoreError::IdentityConflict);
        }
        if !self.is_current() {
            return Err(StoreError::IdentityAuthorization);
        }
        Ok(HostSnapshot::temporary(
            owner,
            self.id(),
            self.0.endpoint.clone(),
        ))
    }
}
impl StateStore {
    /// Issue metadata for a temporary endpoint under an original-page scope without writing SQL.
    /// The scope must not hold a SQL read credit; actual-operation authorization is passed separately.
    /// Actual peer verification, original Vault epoch and explicit credential use are still required.
    pub fn temporary_ssh_target(
        &self,
        endpoint: SshEndpoint,
        scope: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> TemporarySshTarget {
        TemporarySshTarget(Arc::new(Target {
            owner: self.host_owner,
            id: Uuid::now_v7(),
            endpoint,
            deadline: Instant::now() + Duration::from_secs(300),
            revoked: AtomicBool::new(false),
            scope,
        }))
    }
}
