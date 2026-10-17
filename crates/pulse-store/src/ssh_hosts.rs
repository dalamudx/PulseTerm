//! Versioned SSH endpoint metadata. Saving is neither key approval nor permission to dial.
use crate::{Result, StoreError};
use pulse_core::ssh_endpoint::SshEndpoint;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
pub(crate) const KEY: &str = "ssh.endpoints.v1";
const LIMIT: usize = 262_144;
const MAX_HOSTS: usize = 256;
/// Backend-generated catalogue identity and validated metadata. No credential or Debug path.
#[derive(Clone)]
pub struct SavedSshHost {
    id: Uuid,
    endpoint: SshEndpoint,
}
impl SavedSshHost {
    /// Opaque UUIDv7, not a filesystem path or authorization ticket.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Non-secret endpoint metadata; do not log usernames/addresses.
    pub fn endpoint(&self) -> &SshEndpoint {
        &self.endpoint
    }
}
/// Immutable CAS capture including store-instance provenance even when the row is absent.
#[derive(Clone)]
pub struct HostSnapshot {
    pub(crate) owner: Uuid,
    revision: u64,
    records: Vec<SavedSshHost>,
    catalogue: Option<Box<pulse_core::host_catalogue::HostCatalogue>>,
}
impl HostSnapshot {
    #[cfg(feature = "ssh-trust")]
    pub(crate) fn temporary(owner: Uuid, id: Uuid, endpoint: SshEndpoint) -> Self {
        Self {
            owner,
            revision: 0,
            records: vec![SavedSshHost { id, endpoint }],
            catalogue: None,
        }
    }
    /// Machine associations captured in the same SQL read as the legacy entrances.
    pub fn catalogue(&self) -> Option<&pulse_core::host_catalogue::HostCatalogue> {
        self.catalogue.as_deref()
    }
    /// Existing direct-only adapters must refuse a configured jump until a routed adapter is used.
    /// This is transport availability metadata, never a host, credential, or network permission.
    pub fn supports_direct_ssh(&self, endpoint: Uuid) -> bool {
        if !self.records.iter().any(|r| r.id == endpoint) {
            return false;
        }
        self.catalogue.as_ref().is_none_or(|g| {
            let mut profiles = g
                .profiles()
                .iter()
                .filter(|p| !p.metadata().deleted() && p.endpoint_id() == endpoint);
            let Some(profile) = profiles.next() else {
                return false;
            };
            profile.jump_profile_id().is_none() && profiles.next().is_none()
        })
    }
    /// Compare the complete immutable directory and associated machine graph.
    pub(crate) fn source_matches(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.revision == other.revision
            && self.catalogue == other.catalogue
            && self.records.len() == other.records.len()
            && self
                .records
                .iter()
                .zip(&other.records)
                .all(|(a, b)| a.id == b.id && a.endpoint == b.endpoint)
    }
    /// Durable revision, retained after the last record is removed.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Validated records. Empty is only legitimate for an absent row or explicit empty catalogue.
    pub fn records(&self) -> &[SavedSshHost] {
        &self.records
    }
}
/// Trusted Rust intent; no user or window permission is inferred from host identity.
pub enum HostEdit {
    /// Generate an independent UUIDv7 inside the backend transaction.
    Create(SshEndpoint),
    /// Add a separately identified SSH entrance to a machine in the exact source graph.
    AttachSshEndpoint(Uuid, SshEndpoint),
    /// Explicitly create the graph for a legacy-only catalogue without changing targets.
    MigrateCatalogue,
    /// Rename only a captured machine, leaving all entrance/SSH settings intact.
    RenameMachine(Uuid, String),
    /// Update only an existing identity; cannot resurrect a deleted ID.
    Update(Uuid, SshEndpoint),
    /// Remove an existing record and retain the catalogue revision.
    Remove(Uuid),
    /// Remove a nonempty bounded unique set in one catalogue CAS transaction.
    RemoveMany(Vec<Uuid>),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(rename = "schemaVersion")]
    schema_version: u8,
    revision: u64,
    hosts: Vec<Row>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: String,
    label: String,
    host: String,
    port: u16,
    username: String,
}
fn encode(snapshot: &HostSnapshot) -> Result<String> {
    if snapshot.revision == 0
        || snapshot.revision > i64::MAX as u64
        || snapshot.records.len() > MAX_HOSTS
    {
        return Err(StoreError::HostRecord);
    }
    let doc = Document {
        schema_version: 1,
        revision: snapshot.revision,
        hosts: snapshot
            .records
            .iter()
            .map(|r| Row {
                id: r.id.to_string(),
                label: r.endpoint.label().to_owned(),
                host: r.endpoint.target().host().to_owned(),
                port: r.endpoint.target().port(),
                username: r.endpoint.username().to_owned(),
            })
            .collect(),
    };
    let text = serde_json::to_string(&doc).map_err(|_| StoreError::HostRecord)?;
    if text.len() > LIMIT {
        return Err(StoreError::HostRecord);
    }
    Ok(text)
}
fn decode(owner: Uuid, data: &[u8]) -> Result<HostSnapshot> {
    if data.len() > LIMIT {
        return Err(StoreError::HostRecord);
    }
    let doc: Document = serde_json::from_slice(data).map_err(|_| StoreError::HostRecord)?;
    if doc.schema_version != 1
        || doc.revision == 0
        || doc.revision > i64::MAX as u64
        || doc.hosts.len() > MAX_HOSTS
    {
        return Err(StoreError::HostRecord);
    }
    let mut records: Vec<SavedSshHost> = Vec::with_capacity(doc.hosts.len());
    for row in doc.hosts {
        let id = Uuid::parse_str(&row.id).map_err(|_| StoreError::HostRecord)?;
        if id.get_variant() != uuid::Variant::RFC4122
            || id.get_version_num() != 7
            || id.to_string() != row.id
            || records.iter().any(|r| r.id == id)
        {
            return Err(StoreError::HostRecord);
        }
        let endpoint = SshEndpoint::new(&row.label, &row.host, row.port, &row.username)
            .map_err(|_| StoreError::HostRecord)?;
        if endpoint.target().host() != row.host {
            return Err(StoreError::HostRecord);
        }
        records.push(SavedSshHost { id, endpoint });
    }
    Ok(HostSnapshot {
        owner,
        revision: doc.revision,
        records,
        catalogue: None,
    })
}
pub(crate) fn load(connection: &Connection, owner: Uuid) -> Result<HostSnapshot> {
    fn capture(connection: &Connection, owner: Uuid) -> Result<HostSnapshot> {
        let mut snapshot = load_legacy(connection, owner)?;
        snapshot.catalogue = crate::host_catalogue::projection(connection, &snapshot)?;
        Ok(snapshot)
    }
    if !connection.is_autocommit() {
        return capture(connection, owner);
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    let value = capture(&transaction, owner)?;
    transaction.commit().map_err(|_| StoreError::Database)?;
    Ok(value)
}
fn load_legacy(connection: &Connection, owner: Uuid) -> Result<HostSnapshot> {
    let row:Option<(i64,Vec<u8>)>=connection.query_row("SELECT length(CAST(value AS BLOB)),substr(CAST(value AS BLOB),1,262145) FROM app_state WHERE key=?1",[KEY],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(|_|StoreError::Database)?;
    match row {
        None => Ok(HostSnapshot {
            owner,
            revision: 0,
            records: Vec::new(),
            catalogue: None,
        }),
        Some((len, data)) if (0..=LIMIT as i64).contains(&len) => decode(owner, &data),
        _ => Err(StoreError::HostRecord),
    }
}
pub(crate) fn edit(
    connection: &Connection,
    owner: Uuid,
    expected: &HostSnapshot,
    edit: HostEdit,
    authorize: impl Fn() -> bool,
) -> Result<HostSnapshot> {
    if expected.owner != owner {
        return Err(StoreError::HostConflict);
    }
    if !authorize() {
        return Err(StoreError::HostAuthorization);
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = edit_in_transaction(&transaction, owner, expected, edit, None, authorize)?;
    transaction.commit().map_err(|_| StoreError::Database)?;
    Ok(current)
}
/// Reuse the original host CAS and final admission checks in a caller-owned transaction.
/// Callers check initial authorization before opening the transaction and commit only on success.
pub(crate) fn edit_in_transaction(
    transaction: &Transaction<'_>,
    owner: Uuid,
    expected: &HostSnapshot,
    edit: HostEdit,
    profile: Option<pulse_core::host_catalogue::SshProfile>,
    authorize: impl Fn() -> bool,
) -> Result<HostSnapshot> {
    if expected.owner != owner {
        return Err(StoreError::HostConflict);
    }
    let mut current = load(transaction, owner)?;
    if current.catalogue != expected.catalogue
        || current.revision != expected.revision
        || current.records.len() != expected.records.len()
        || !current
            .records
            .iter()
            .zip(&expected.records)
            .all(|(a, b)| a.id == b.id && a.endpoint == b.endpoint)
    {
        return Err(StoreError::HostConflict);
    }
    let before = current.clone();
    let parent = match &edit {
        HostEdit::AttachSshEndpoint(id, _) => Some(*id),
        _ => None,
    };
    if let Some(id) = parent
        && !before.catalogue().is_some_and(|g| {
            g.hosts()
                .iter()
                .any(|h| h.metadata().id() == id && !h.metadata().deleted())
        })
    {
        return Err(StoreError::HostConflict);
    }
    let mut rename = None;
    match edit {
        HostEdit::RenameMachine(id, name) => {
            let machine = before
                .catalogue()
                .and_then(|g| {
                    g.hosts()
                        .iter()
                        .find(|h| h.metadata().id() == id && !h.metadata().deleted())
                })
                .ok_or(StoreError::HostConflict)?;
            let validated =
                pulse_core::host_catalogue::Host::new(machine.metadata().clone(), &name)
                    .map_err(|_| StoreError::HostRecord)?;
            rename = Some((id, validated.label().to_owned()));
        }
        HostEdit::MigrateCatalogue => {
            if before.catalogue.is_some() {
                return Err(StoreError::HostConflict);
            }
        }
        HostEdit::Create(endpoint) | HostEdit::AttachSshEndpoint(_, endpoint) => {
            if current.records.len() >= MAX_HOSTS {
                return Err(StoreError::HostRecord);
            }
            let id = Uuid::now_v7();
            if current.records.iter().any(|r| r.id == id) {
                return Err(StoreError::HostRecord);
            }
            current.records.push(SavedSshHost { id, endpoint });
        }
        HostEdit::Update(id, endpoint) => {
            let entry = current
                .records
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or(StoreError::HostConflict)?;
            entry.endpoint = endpoint;
        }
        HostEdit::RemoveMany(ids) => {
            let ids = crate::metadata_batch::unique_ids(&ids).ok_or(StoreError::HostRecord)?;
            if ids
                .iter()
                .any(|id| !current.records.iter().any(|row| row.id == *id))
            {
                return Err(StoreError::HostConflict);
            }
            current.records.retain(|row| !ids.contains(&row.id));
        }
        HostEdit::Remove(id) => {
            let index = current
                .records
                .iter()
                .position(|r| r.id == id)
                .ok_or(StoreError::HostConflict)?;
            current.records.remove(index);
        }
    }
    current.revision = current
        .revision
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or(StoreError::HostRevision)?;
    let text = encode(&current)?;
    crate::host_catalogue::reconcile(
        transaction,
        &before,
        &current,
        parent,
        rename,
        profile.as_ref(),
    )?;
    current.catalogue = crate::host_catalogue::projection(transaction, &current)?;
    if !authorize() {
        return Err(StoreError::HostAuthorization);
    }
    transaction.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![KEY,text]).map_err(|_|StoreError::Database)?;
    if !authorize() {
        return Err(StoreError::HostAuthorization);
    }
    Ok(current)
}
