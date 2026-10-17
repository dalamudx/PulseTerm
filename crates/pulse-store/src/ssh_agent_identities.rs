//! Explicit Agent metadata directory. Reads do not migrate tables or contact an Agent.
use crate::{Result, StoreError};
use pulse_core::{hlc::Hlc, ssh_agent_identity::SshAgentIdentity};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;
const ACTIVE: usize = 256;
const DELETED: usize = 4096;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS agent_identity_catalog(slot INTEGER PRIMARY KEY CHECK(slot=1),format INTEGER NOT NULL CHECK(format=1),revision INTEGER NOT NULL CHECK(revision>0),node_id TEXT NOT NULL,clock TEXT NOT NULL) STRICT; CREATE TABLE IF NOT EXISTS agent_identities(id TEXT PRIMARY KEY,created_at INTEGER NOT NULL CHECK(created_at>0),hlc TEXT NOT NULL,deleted INTEGER NOT NULL CHECK(deleted IN(0,1)),label TEXT NOT NULL,username TEXT NOT NULL,algorithm TEXT NOT NULL,fingerprint TEXT NOT NULL) STRICT;";
/// Public-key metadata only. Not a signing, socket or authentication capability.
#[derive(Clone, PartialEq, Eq)]
pub struct SavedSshAgentIdentity {
    id: Uuid,
    created_at: u64,
    stamp: Hlc,
    deleted: bool,
    value: SshAgentIdentity,
}
impl SavedSshAgentIdentity {
    /// Backend-generated stable UUIDv7.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Creation timestamp retained across edits and deletion.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    /// Reserved sync metadata; no synchronization transport.
    pub fn stamp(&self) -> Hlc {
        self.stamp
    }
    /// Exact selected algorithm and SHA256 public-key fingerprint.
    pub fn identity(&self) -> &SshAgentIdentity {
        &self.value
    }
}
/// Instance-bound complete directory including tombstones; no Debug or serialization.
#[derive(Clone, PartialEq, Eq)]
pub struct AgentIdentitySnapshot {
    pub(crate) owner: Uuid,
    revision: u64,
    clock: Option<Hlc>,
    rows: Vec<SavedSshAgentIdentity>,
}
impl AgentIdentitySnapshot {
    /// Monotonic local revision, not signing authority.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Active identities only. Deletion never revives an earlier ID.
    pub fn records(&self) -> impl Iterator<Item = &SavedSshAgentIdentity> {
        self.rows.iter().filter(|row| !row.deleted)
    }
}
/// Native metadata edit. A selection from an untrusted Agent still needs explicit save approval.
pub enum AgentIdentityEdit {
    /// Generate a new UUIDv7.
    Create(SshAgentIdentity),
    /// Replace complete bounded public metadata.
    Update(Uuid, SshAgentIdentity),
    /// Retain a tombstone and reject deletion of a referenced Profile identity.
    Remove(Uuid),
}
fn id(raw: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(raw).map_err(|_| StoreError::IdentityRecord)?;
    if id.is_nil() || id.get_version_num() != 7 || id.to_string() != raw {
        return Err(StoreError::IdentityRecord);
    }
    Ok(id)
}
pub(crate) fn load(c: &Connection, owner: Uuid) -> Result<AgentIdentitySnapshot> {
    if !c.is_autocommit() {
        return rows(c, owner);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    rows(&tx, owner)
}
fn rows(c: &Connection, owner: Uuid) -> Result<AgentIdentitySnapshot> {
    let count:i64=c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN('agent_identity_catalog','agent_identities')",[],|r|r.get(0)).map_err(|_|StoreError::Database)?;
    if count == 0 {
        return Ok(AgentIdentitySnapshot {
            owner,
            revision: 0,
            clock: None,
            rows: Vec::new(),
        });
    }
    if count != 2 {
        return Err(StoreError::IdentityRecord);
    }
    let catalogue_count: i64 = c
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM agent_identity_catalog LIMIT 2)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::IdentityRecord)?;
    let meta:Option<(i64,i64,String,String)>=c.query_row("SELECT format,revision,substr(node_id,1,37),substr(clock,1,68) FROM agent_identity_catalog WHERE slot=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_|StoreError::IdentityRecord)?;
    let row_count: i64 = c
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM agent_identities LIMIT 4353)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::IdentityRecord)?;
    let Some((format, revision, node, clock)) = meta else {
        if catalogue_count != 0 || row_count != 0 {
            return Err(StoreError::IdentityRecord);
        }
        return Ok(AgentIdentitySnapshot {
            owner,
            revision: 0,
            clock: None,
            rows: Vec::new(),
        });
    };
    let clock = Hlc::parse(&clock).map_err(|_| StoreError::IdentityRecord)?;
    if catalogue_count != 1
        || format != 1
        || revision <= 0
        || clock.node() != id(&node)?
        || row_count > 4352
    {
        return Err(StoreError::IdentityRecord);
    }
    let mut stmt=c.prepare("SELECT substr(id,1,37),created_at,substr(hlc,1,68),deleted,substr(label,1,257),substr(username,1,256),substr(algorithm,1,65),substr(fingerprint,1,51) FROM agent_identities ORDER BY id LIMIT 4353").map_err(|_|StoreError::IdentityRecord)?;
    let mut result = stmt.query([]).map_err(|_| StoreError::IdentityRecord)?;
    let mut values: Vec<SavedSshAgentIdentity> = Vec::new();
    let mut deleted_count = 0;
    let mut bytes = 0usize;
    while let Some(row) = result.next().map_err(|_| StoreError::IdentityRecord)? {
        let raw: String = row.get(0).map_err(|_| StoreError::IdentityRecord)?;
        let id = id(&raw)?;
        let created: i64 = row.get(1).map_err(|_| StoreError::IdentityRecord)?;
        let raw_stamp: String = row.get(2).map_err(|_| StoreError::IdentityRecord)?;
        let stamp = Hlc::parse(&raw_stamp).map_err(|_| StoreError::IdentityRecord)?;
        let deleted: i64 = row.get(3).map_err(|_| StoreError::IdentityRecord)?;
        let label: String = row.get(4).map_err(|_| StoreError::IdentityRecord)?;
        let username: String = row.get(5).map_err(|_| StoreError::IdentityRecord)?;
        let algorithm: String = row.get(6).map_err(|_| StoreError::IdentityRecord)?;
        let fingerprint: String = row.get(7).map_err(|_| StoreError::IdentityRecord)?;
        if created <= 0
            || created as u64 > stamp.wall()
            || stamp > clock
            || stamp.node() != clock.node()
            || !matches!(deleted, 0 | 1)
            || values.iter().any(|row| row.id == id)
        {
            return Err(StoreError::IdentityRecord);
        }
        let value = SshAgentIdentity::new(&label, &username, &algorithm, &fingerprint)
            .map_err(|_| StoreError::IdentityRecord)?;
        bytes = bytes
            .checked_add(
                raw.len()
                    + raw_stamp.len()
                    + label.len()
                    + username.len()
                    + algorithm.len()
                    + fingerprint.len(),
            )
            .ok_or(StoreError::IdentityRecord)?;
        if bytes > 4_194_304 {
            return Err(StoreError::IdentityRecord);
        }
        deleted_count += usize::from(deleted == 1);
        values.push(SavedSshAgentIdentity {
            id,
            created_at: created as u64,
            stamp,
            deleted: deleted == 1,
            value,
        });
    }
    if deleted_count > DELETED || values.len() - deleted_count > ACTIVE {
        return Err(StoreError::IdentityRecord);
    }
    Ok(AgentIdentitySnapshot {
        owner,
        revision: revision as u64,
        clock: Some(clock),
        rows: values,
    })
}
pub(crate) fn same_metadata(a: &AgentIdentitySnapshot, b: &AgentIdentitySnapshot) -> bool {
    a.revision == b.revision && a.clock == b.clock && a.rows == b.rows
}
pub(crate) fn edit(
    c: &Connection,
    owner: Uuid,
    expected: &AgentIdentitySnapshot,
    intent: AgentIdentityEdit,
    authorize: impl Fn() -> bool,
) -> Result<AgentIdentitySnapshot> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::IdentityExhausted)?
        .as_millis()
        .try_into()
        .map_err(|_| StoreError::IdentityExhausted)?;
    if expected.owner != owner {
        return Err(StoreError::IdentityConflict);
    }
    if !authorize() {
        return Err(StoreError::IdentityAuthorization);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let mut current = rows(&tx, owner)?;
    if !same_metadata(&current, expected) {
        return Err(StoreError::IdentityConflict);
    }
    let stamp = match current.clock {
        Some(clock) => clock.next(now),
        None => Hlc::new(now, 0, Uuid::now_v7()),
    }
    .map_err(|_| StoreError::IdentityExhausted)?;
    match intent {
        AgentIdentityEdit::Create(value) => {
            if current.records().count() >= ACTIVE {
                return Err(StoreError::IdentityExhausted);
            }
            current.rows.push(SavedSshAgentIdentity {
                id: Uuid::now_v7(),
                created_at: stamp.wall(),
                stamp,
                deleted: false,
                value,
            });
        }
        AgentIdentityEdit::Update(id, value) => {
            let row = current
                .rows
                .iter_mut()
                .find(|row| row.id == id && !row.deleted)
                .ok_or(StoreError::IdentityConflict)?;
            row.value = value;
            row.stamp = stamp;
        }
        AgentIdentityEdit::Remove(id) => {
            if current.rows.iter().filter(|row| row.deleted).count() >= DELETED {
                return Err(StoreError::IdentityExhausted);
            }
            let legacy = crate::ssh_hosts::load(&tx, owner)?;
            if let Some(graph) = crate::host_catalogue::projection(&tx, &legacy)?
                && graph.profiles().iter().any(|profile| {
                    !profile.metadata().deleted() && profile.identity_id() == Some(id)
                })
            {
                return Err(StoreError::IdentityConflict);
            }
            let row = current
                .rows
                .iter_mut()
                .find(|row| row.id == id && !row.deleted)
                .ok_or(StoreError::IdentityConflict)?;
            row.deleted = true;
            row.stamp = stamp;
        }
    }
    current.revision = current
        .revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(StoreError::IdentityExhausted)?;
    current.clock = Some(stamp);
    tx.execute_batch(SCHEMA).map_err(|_| StoreError::Database)?;
    tx.execute("INSERT INTO agent_identity_catalog(slot,format,revision,node_id,clock) VALUES(1,1,?1,?2,?3) ON CONFLICT(slot) DO UPDATE SET revision=excluded.revision,clock=excluded.clock",rusqlite::params![current.revision as i64,stamp.node().to_string(),stamp.encode()]).map_err(|_|StoreError::Database)?;
    for row in &current.rows {
        tx.execute("INSERT INTO agent_identities(id,created_at,hlc,deleted,label,username,algorithm,fingerprint) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET hlc=excluded.hlc,deleted=excluded.deleted,label=excluded.label,username=excluded.username,algorithm=excluded.algorithm,fingerprint=excluded.fingerprint",rusqlite::params![row.id.to_string(),row.created_at as i64,row.stamp.encode(),i64::from(row.deleted),row.value.label(),row.value.username(),row.value.algorithm(),row.value.fingerprint()]).map_err(|_|StoreError::Database)?;
    }
    let committed = rows(&tx, owner)?;
    if !authorize() {
        return Err(StoreError::IdentityAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(committed)
}
#[path = "ssh_agent_identity_worker.rs"]
mod worker;
pub(crate) use worker::AgentRequest;

#[cfg(test)]
#[path = "ssh_agent_identities_tests.rs"]
mod tests;
