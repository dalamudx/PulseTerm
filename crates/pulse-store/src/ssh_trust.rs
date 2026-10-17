//! Optional SSH trust records only. Public-key validation belongs to a higher layer.
use crate::{Result, StoreError};
#[cfg(feature = "ssh-trust")]
use rusqlite::TransactionBehavior;
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[path = "ssh_trust_relational.rs"]
pub(crate) mod relational;
const KEY: &str = "ssh.trusted_hosts.v1";
const LIMIT: usize = 65_536;
const MAX_HOSTS: usize = 128;

/// Final-dial identity, never derived from an untrusted SSH server message.
#[derive(Clone, Eq, PartialEq)]
pub struct TrustTarget {
    host: String,
    port: u16,
}
impl TrustTarget {
    /// Match the backend SSH host spelling policy; consumers must also validate before use.
    pub fn new(host: &str, port: u16) -> Result<Self> {
        let target = pulse_core::ssh_target::SshTarget::new(host, port)
            .map_err(|_| StoreError::HostTrustRecord)?;
        Ok(Self {
            host: target.host().to_owned(),
            port: target.port(),
        })
    }
    /// Canonical host or unbracketed IP. Trusted backend only; do not log identifiers.
    pub fn host(&self) -> &str {
        &self.host
    }
    /// Nonzero TCP port included in identity.
    pub fn port(&self) -> u16 {
        self.port
    }
}
/// Bounded text record. It is **not** a validated SSH public key or a trust decision.
#[derive(Clone)]
pub struct TrustRecord {
    target: TrustTarget,
    key: String,
}
impl TrustRecord {
    /// Validate storage shape; the SSH adapter must parse every key before authentication.
    pub fn new(target: TrustTarget, key: String) -> Result<Self> {
        if key.len() > 8192 || key.len() < 5 || !key.is_ascii() {
            return Err(StoreError::HostTrustRecord);
        }
        let Some((kind, encoded)) = key.split_once(' ') else {
            return Err(StoreError::HostTrustRecord);
        };
        if kind.is_empty()
            || encoded.is_empty()
            || !kind
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"@.-_".contains(&b))
            || !encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        {
            return Err(StoreError::HostTrustRecord);
        }
        Ok(Self { target, key })
    }
    /// Final dial target; record itself is not cryptographically validated.
    pub fn target(&self) -> &TrustTarget {
        &self.target
    }
    /// Unparsed bounded key text; only the SSH adapter may turn it into a HostPin.
    pub fn key(&self) -> &str {
        &self.key
    }
}
/// Opaque revision/owner read receipt. No page serialization or authority.
#[derive(Clone)]
pub struct TrustSnapshot {
    pub(crate) owner: Uuid,
    pub(crate) revision: u64,
    pub(crate) records: Vec<TrustRecord>,
    pub(crate) rows: Vec<relational::StoredPin>,
    pub(crate) clock: Option<pulse_core::hlc::Hlc>,
}
impl TrustSnapshot {
    /// Durable revision, zero before first write.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Raw records; the caller must fail closed unless every key parses as SSH public key.
    pub fn records(&self) -> &[TrustRecord] {
        &self.records
    }
}
/// Trusted backend intent only. Callers must separately enforce explicit user confirmation.
#[cfg(feature = "ssh-trust")]
pub enum TrustEdit {
    /// Replace the single pin at this target after a separately authorized approval.
    Pin(TrustRecord),
    /// Remove a pin while retaining the revision tombstone.
    Forget(TrustTarget),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(rename = "schemaVersion")]
    schema_version: u8,
    revision: u64,
    pins: Vec<Row>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    host: String,
    port: u16,
    key: String,
}
fn encode(snapshot: &TrustSnapshot) -> Result<String> {
    if snapshot.revision == 0
        || snapshot.revision > i64::MAX as u64
        || snapshot.records.len() > MAX_HOSTS
    {
        return Err(StoreError::HostTrustRecord);
    }
    let pins = snapshot
        .records
        .iter()
        .map(|r| Row {
            host: r.target.host.clone(),
            port: r.target.port,
            key: r.key.clone(),
        })
        .collect();
    let text = serde_json::to_string(&Document {
        schema_version: 1,
        revision: snapshot.revision,
        pins,
    })
    .map_err(|_| StoreError::HostTrustRecord)?;
    if text.len() > LIMIT {
        return Err(StoreError::HostTrustRecord);
    }
    Ok(text)
}
fn decode(owner: Uuid, data: &[u8]) -> Result<TrustSnapshot> {
    if data.len() > LIMIT {
        return Err(StoreError::HostTrustRecord);
    }
    let doc: Document = serde_json::from_slice(data).map_err(|_| StoreError::HostTrustRecord)?;
    if doc.schema_version != 1
        || doc.revision == 0
        || doc.revision > i64::MAX as u64
        || doc.pins.len() > MAX_HOSTS
    {
        return Err(StoreError::HostTrustRecord);
    }
    let mut records: Vec<TrustRecord> = Vec::with_capacity(doc.pins.len());
    for row in doc.pins {
        let target = TrustTarget::new(&row.host, row.port)?;
        if target.host() != row.host || records.iter().any(|r| r.target == target) {
            return Err(StoreError::HostTrustRecord);
        }
        records.push(TrustRecord::new(target, row.key)?);
    }
    Ok(TrustSnapshot {
        owner,
        revision: doc.revision,
        records,
        rows: Vec::new(),
        clock: None,
    })
}
pub(crate) fn load(connection: &Connection, owner: Uuid) -> Result<TrustSnapshot> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| StoreError::Schema)?;
    match version {
        4 | 5 => relational::load(connection, owner),
        1..=3 => load_legacy(connection, owner),
        _ => Err(StoreError::Schema),
    }
}
fn load_legacy(connection: &Connection, owner: Uuid) -> Result<TrustSnapshot> {
    let row: Option<(i64, Vec<u8>)> = connection.query_row(
        "SELECT length(CAST(value AS BLOB)), substr(CAST(value AS BLOB),1,65537) FROM app_state WHERE key=?1",
        [KEY], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(|_| StoreError::Database)?;
    match row {
        None => Ok(TrustSnapshot {
            owner,
            revision: 0,
            records: Vec::new(),
            rows: Vec::new(),
            clock: None,
        }),
        Some((length, data)) if (0..=LIMIT as i64).contains(&length) => decode(owner, &data),
        _ => Err(StoreError::HostTrustRecord),
    }
}
/// Insert only an absent target inside the caller's existing source-checked transaction.
#[cfg(feature = "ssh-trust")]
pub(crate) fn insert_first(
    tx: &Transaction<'_>,
    owner: Uuid,
    record: TrustRecord,
) -> Result<TrustSnapshot> {
    let mut current = load(tx, owner)?;
    if current
        .records
        .iter()
        .any(|prior| prior.target() == record.target())
    {
        return Err(StoreError::HostTrustConflict);
    }
    current.records.push(record);
    current.revision = current
        .revision
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(StoreError::HostTrustRevision)?;
    write_snapshot(tx, &current)?;
    load(tx, owner)
}

#[cfg(feature = "ssh-trust")]
fn write_snapshot(tx: &Transaction<'_>, snapshot: &TrustSnapshot) -> Result<()> {
    let version: i64 = tx
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| StoreError::Schema)?;
    if matches!(version, 4 | 5) {
        return relational::save(tx, snapshot);
    }
    let text = encode(snapshot)?;
    tx.execute("INSERT INTO app_state(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![KEY,text]).map_err(|_| StoreError::Database)?;
    Ok(())
}
#[cfg(feature = "ssh-trust")]
pub(crate) fn edit(
    connection: &Connection,
    owner: Uuid,
    expected: &TrustSnapshot,
    edit: TrustEdit,
) -> Result<TrustSnapshot> {
    if expected.owner != owner {
        return Err(StoreError::HostTrustConflict);
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let mut current = load(&transaction, owner)?;
    if current.revision != expected.revision
        || current.clock != expected.clock
        || current.rows != expected.rows
        || current.records.len() != expected.records.len()
        || current
            .records
            .iter()
            .zip(&expected.records)
            .any(|(a, b)| a.target() != b.target() || a.key() != b.key())
    {
        return Err(StoreError::HostTrustConflict);
    }
    let target = match &edit {
        TrustEdit::Pin(record) => record.target(),
        TrustEdit::Forget(target) => target,
    };
    let prior = current
        .records
        .iter()
        .position(|record| record.target() == target);
    if prior.is_none() && matches!(&edit, TrustEdit::Forget(_)) {
        return Err(StoreError::HostTrustConflict);
    }
    if let Some(index) = prior {
        current.records.remove(index);
    }
    if let TrustEdit::Pin(record) = edit {
        current.records.push(record);
    }
    current.revision = current
        .revision
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(StoreError::HostTrustRevision)?;
    write_snapshot(&transaction, &current)?;
    let result = load(&transaction, owner)?;
    transaction.commit().map_err(|_| StoreError::Database)?;
    Ok(result)
}
