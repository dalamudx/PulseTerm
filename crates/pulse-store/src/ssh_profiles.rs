//! Joint source for SSH Profile metadata editing. No serialization, credential or network use.
use crate::{
    Request, Result, StateStore, StoreError,
    ssh_hosts::{self, HostEdit, HostSnapshot},
    ssh_identities::{self, IdentitySnapshot, SavedSshIdentity},
    ssh_key_identities::{self, KeyIdentitySnapshot, SavedSshKeyIdentity},
};
use pulse_core::{host_catalogue::SshProfile, ssh_endpoint::SshEndpoint};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;

/// Original store instance, graph and all three identity catalogues captured in one SQL read.
/// Contains metadata and opaque references only, never plaintext or use permission.
#[derive(Clone)]
pub struct ProfileSnapshot {
    hosts: Box<HostSnapshot>,
    passwords: IdentitySnapshot,
    keys: KeyIdentitySnapshot,
    agents: crate::ssh_agent_identities::AgentIdentitySnapshot,
}
impl ProfileSnapshot {
    /// Explicit Agent metadata references, never socket paths or signing permission.
    pub fn agent_identities(
        &self,
    ) -> impl Iterator<Item = &crate::ssh_agent_identities::SavedSshAgentIdentity> {
        self.agents.records()
    }
    /// Saved entrances and machine associations from this same source.
    pub fn hosts(&self) -> &HostSnapshot {
        &self.hosts
    }
    /// Active SSH preferences only. Legacy-only storage returns no generated profiles.
    pub fn profiles(&self) -> impl Iterator<Item = &SshProfile> {
        self.hosts
            .catalogue()
            .into_iter()
            .flat_map(|g| g.profiles().iter())
            .filter(|p| !p.metadata().deleted())
    }
    /// Active password identity metadata; references grant no password read permission.
    pub fn password_identities(&self) -> impl Iterator<Item = &SavedSshIdentity> {
        self.passwords.records()
    }
    /// Active private-key identity metadata; references grant no key read permission.
    pub fn key_identities(&self) -> impl Iterator<Item = &SavedSshKeyIdentity> {
        self.keys.records()
    }
}
/// Literal bounded preference edit; IDs must resolve in the captured source.
pub struct ProfileEdit {
    /// Backend-issued Profile ID, distinct from its entrance and machine.
    pub id: Uuid,
    /// Independent Profile display name.
    pub label: String,
    /// Login username, also reflected in the checked legacy entrance projection.
    pub username: String,
    /// Optional active identity ID. No automatic credential creation or activation.
    pub identity: Option<Uuid>,
}
pub(crate) enum ProfileMutation {
    Preferences(ProfileEdit),
    Jump { id: Uuid, via: Option<Uuid> },
    Complete(ProfileEdit, Option<Uuid>),
}
impl From<ProfileEdit> for ProfileMutation {
    fn from(edit: ProfileEdit) -> Self {
        Self::Preferences(edit)
    }
}
pub(crate) fn load(connection: &Connection, owner: Uuid) -> Result<ProfileSnapshot> {
    if !connection.is_autocommit() {
        return load_rows(connection, owner);
    }
    let tx = Transaction::new_unchecked(connection, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    let result = load_rows(&tx, owner)?;
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(result)
}
fn load_rows(connection: &Connection, owner: Uuid) -> Result<ProfileSnapshot> {
    Ok(ProfileSnapshot {
        hosts: Box::new(ssh_hosts::load(connection, owner)?),
        passwords: ssh_identities::load(connection, owner)?,
        keys: ssh_key_identities::load(connection, owner)?,
        agents: crate::ssh_agent_identities::load(connection, owner)?,
    })
}
pub(crate) fn edit(
    connection: &Connection,
    owner: Uuid,
    expected: &ProfileSnapshot,
    change: impl Into<ProfileMutation>,
    authorize: impl Fn() -> bool,
) -> Result<ProfileSnapshot> {
    let change = change.into();
    let selected_id = match &change {
        ProfileMutation::Preferences(v) | ProfileMutation::Complete(v, _) => v.id,
        ProfileMutation::Jump { id, .. } => *id,
    };
    if expected.hosts.owner != owner {
        return Err(StoreError::HostConflict);
    }
    if !authorize() {
        return Err(StoreError::HostAuthorization);
    }
    let tx = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = load_rows(&tx, owner)?;
    // Metadata edits, tombstones, Vault namespace or presence changes invalidate the source,
    // including same-revision replacement. Ciphertext activation is a separate upper boundary.
    if current.passwords != expected.passwords
        || current.keys != expected.keys
        || current.agents != expected.agents
    {
        return Err(StoreError::HostConflict);
    }
    let original = expected
        .profiles()
        .find(|p| p.metadata().id() == selected_id)
        .ok_or(StoreError::HostConflict)?;
    let (preferences, jump) = match change {
        ProfileMutation::Preferences(change) => (Some(change), original.jump_profile_id()),
        ProfileMutation::Complete(change, via) => (Some(change), via),
        ProfileMutation::Jump { via, .. } => (None, via),
    };
    let profile = if let Some(change) = preferences {
        if let Some(id) = change.identity {
            let matches = expected
                .password_identities()
                .filter(|r| r.id() == id)
                .count()
                + expected.key_identities().filter(|r| r.id() == id).count()
                + expected.agent_identities().filter(|r| r.id() == id).count();
            if matches != 1 {
                return Err(StoreError::HostRecord);
            }
        }
        SshProfile::new(
            original.metadata().clone(),
            original.endpoint_id(),
            &change.label,
            &change.username,
            change.identity,
        )
        .and_then(|p| p.with_jump_profile(jump))
    } else {
        original.clone().with_jump_profile(jump)
    }
    .map_err(|_| StoreError::HostRecord)?;
    let entrance = expected
        .hosts
        .records()
        .iter()
        .find(|r| r.id() == original.endpoint_id())
        .ok_or(StoreError::HostConflict)?;
    let endpoint = SshEndpoint::new(
        entrance.endpoint().label(),
        entrance.endpoint().target().host(),
        entrance.endpoint().target().port(),
        profile.username(),
    )
    .map_err(|_| StoreError::HostRecord)?;
    ssh_hosts::edit_in_transaction(
        &tx,
        owner,
        &expected.hosts,
        HostEdit::Update(entrance.id(), endpoint),
        Some(profile),
        &authorize,
    )?;
    let next = load_rows(&tx, owner)?;
    // The joint-source read also stays inside the final authorization/commit boundary.
    if !authorize() {
        return Err(StoreError::HostAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(next)
}
impl StateStore {
    /// Authorized joint metadata read; execution credit must live in this predicate.
    pub async fn load_ssh_profiles_authorized(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ProfileSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshProfilesLoad(self.host_owner, authorize, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Configure or detach a saved jump Profile using the same complete-source CAS and worker.
    /// No dial, credential use, or implicit direct fallback is authorized by this operation.
    pub async fn edit_ssh_profile_jump(
        &self,
        expected: ProfileSnapshot,
        id: Uuid,
        via: Option<Uuid>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ProfileSnapshot> {
        if expected.hosts.owner != self.host_owner {
            return Err(StoreError::HostConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshProfileEdit(
            self.host_owner,
            Box::new(expected),
            ProfileMutation::Jump { id, via },
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Update preferences and the explicit jump reference at one revision/commit boundary.
    pub async fn edit_ssh_profile_with_jump(
        &self,
        expected: ProfileSnapshot,
        change: ProfileEdit,
        via: Option<Uuid>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ProfileSnapshot> {
        if expected.hosts.owner != self.host_owner {
            return Err(StoreError::HostConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshProfileEdit(
            self.host_owner,
            Box::new(expected),
            ProfileMutation::Complete(change, via),
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Original-source CAS edit on the shared worker. Saving grants no credential-use authority.
    pub async fn edit_ssh_profile(
        &self,
        expected: ProfileSnapshot,
        change: ProfileEdit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ProfileSnapshot> {
        if expected.hosts.owner != self.host_owner {
            return Err(StoreError::HostConflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::SshProfileEdit(
            self.host_owner,
            Box::new(expected),
            ProfileMutation::Preferences(change),
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
#[cfg(test)]
#[path = "ssh_profiles_tests.rs"]
mod tests;
