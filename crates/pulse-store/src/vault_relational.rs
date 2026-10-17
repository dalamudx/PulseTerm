//! Strict relational Vault schema and ciphertext-only v1 import; no key or plaintext operations.
use super::{
    CredentialRecord, MAX_CREDENTIALS, MAX_DELETED, ValidationWrappedDek, VaultSnapshot, encode,
};
use super::{KEY, load_legacy};
use crate::{Result, StoreError, vault_crypto::SECRET_BYTES_LIMIT};
use rusqlite::{Connection, OptionalExtension, Transaction};
use uuid::Uuid;
pub(crate) const SCHEMA:&str="CREATE TABLE vault_envelopes(slot INTEGER PRIMARY KEY CHECK(slot=1),id TEXT NOT NULL UNIQUE CHECK(length(id)=36),revision INTEGER NOT NULL CHECK(revision>0),format INTEGER NOT NULL CHECK(format=1),envelope BLOB NOT NULL CHECK(length(envelope)=110)) STRICT;
CREATE TABLE secrets(id TEXT PRIMARY KEY CHECK(length(id)=36),vault_id TEXT NOT NULL REFERENCES vault_envelopes(id),kind TEXT NOT NULL CHECK(length(kind) BETWEEN 1 AND 32),record_version INTEGER NOT NULL CHECK(record_version=1),nonce BLOB NOT NULL CHECK(length(nonce)=24),ciphertext BLOB NOT NULL CHECK(length(ciphertext) BETWEEN 16 AND 65552)) STRICT;
CREATE TABLE vault_deleted_credentials(id TEXT PRIMARY KEY CHECK(length(id)=36),vault_id TEXT NOT NULL REFERENCES vault_envelopes(id)) STRICT;";
pub(crate) fn exists(c: &Connection) -> Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='vault_envelopes')",
        [],
        |r| r.get(0),
    )
    .map_err(|_| StoreError::Database)
}
fn uuid(value: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| StoreError::VaultRecord)?;
    if id.is_nil() || id.to_string() != value {
        return Err(StoreError::VaultRecord);
    }
    Ok(id)
}
pub(crate) fn load_rows(c: &Connection, owner: Uuid) -> Result<VaultSnapshot> {
    let legacy_exists: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM app_state WHERE key=?1)",
            [KEY],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::Database)?;
    if legacy_exists {
        return Err(StoreError::VaultRecord);
    }
    let metadata:Option<(i64,String,i64,i64,i64,Vec<u8>)>=c.query_row("SELECT slot,substr(id,1,37),revision,format,length(envelope),substr(envelope,1,111) FROM vault_envelopes LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(|_|StoreError::Database)?;
    let meta_count: i64 = c
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM vault_envelopes LIMIT 2)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::Database)?;
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM secrets LIMIT 257)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::Database)?;
    let deleted_count: i64 = c
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM vault_deleted_credentials LIMIT 4097)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::Database)?;
    if meta_count > 1 || count > MAX_CREDENTIALS as i64 || deleted_count > MAX_DELETED as i64 {
        return Err(StoreError::VaultRecord);
    }
    let Some((slot, id, revision, format, len, bytes)) = metadata else {
        if count != 0 || deleted_count != 0 {
            return Err(StoreError::VaultRecord);
        }
        return Ok(VaultSnapshot {
            owner,
            revision: 0,
            vault_id: None,
            envelope: None,
            credentials: Vec::new(),
            deleted: Vec::new(),
        });
    };
    if slot != 1 || revision <= 0 || format != 1 || len != 110 {
        return Err(StoreError::VaultRecord);
    }
    let vault_id = uuid(&id)?;
    let envelope = ValidationWrappedDek::decode_ciphertext(&bytes, vault_id)
        .map_err(|_| StoreError::VaultRecord)?;
    let mut deleted = Vec::new();
    let mut statement = c
        .prepare("SELECT substr(id,1,37),substr(vault_id,1,37) FROM vault_deleted_credentials LIMIT 4097")
        .map_err(|_| StoreError::Database)?;
    let mut rows = statement.query([]).map_err(|_| StoreError::Database)?;
    while let Some(row) = rows.next().map_err(|_| StoreError::Database)? {
        let raw: String = row.get(0).map_err(|_| StoreError::VaultRecord)?;
        let owner_id: String = row.get(1).map_err(|_| StoreError::VaultRecord)?;
        if owner_id != id {
            return Err(StoreError::VaultRecord);
        }
        let deleted_id = uuid(&raw)?;
        if deleted.contains(&deleted_id) {
            return Err(StoreError::VaultRecord);
        }
        deleted.push(deleted_id);
    }
    let mut credentials = Vec::new();
    let mut statement=c.prepare("SELECT substr(id,1,37),substr(vault_id,1,37),substr(kind,1,33),record_version,length(nonce),substr(nonce,1,25),length(ciphertext),substr(ciphertext,1,65553) FROM secrets LIMIT 257").map_err(|_|StoreError::Database)?;
    let mut rows = statement.query([]).map_err(|_| StoreError::Database)?;
    let mut allocated = 0usize;
    while let Some(row) = rows.next().map_err(|_| StoreError::Database)? {
        let record_id: String = row.get(0).map_err(|_| StoreError::VaultRecord)?;
        let owner_id: String = row.get(1).map_err(|_| StoreError::VaultRecord)?;
        let kind: String = row.get(2).map_err(|_| StoreError::VaultRecord)?;
        let version: i64 = row.get(3).map_err(|_| StoreError::VaultRecord)?;
        let nonce_len: i64 = row.get(4).map_err(|_| StoreError::VaultRecord)?;
        let nonce: Vec<u8> = row.get(5).map_err(|_| StoreError::VaultRecord)?;
        let length: i64 = row.get(6).map_err(|_| StoreError::VaultRecord)?;
        if owner_id != id
            || version != 1
            || nonce_len != 24
            || !(16..=(SECRET_BYTES_LIMIT + 16) as i64).contains(&length)
        {
            return Err(StoreError::VaultRecord);
        }
        allocated = allocated
            .checked_add(length as usize)
            .ok_or(StoreError::VaultRecord)?;
        if allocated > super::DOCUMENT_LIMIT {
            return Err(StoreError::VaultRecord);
        }
        let ciphertext: Vec<u8> = row.get(7).map_err(|_| StoreError::VaultRecord)?;
        let record_id = uuid(&record_id)?;
        if deleted.contains(&record_id)
            || credentials
                .iter()
                .any(|r: &CredentialRecord| r.id() == record_id)
        {
            return Err(StoreError::VaultRecord);
        }
        let nonce: [u8; 24] = nonce.try_into().map_err(|_| StoreError::VaultRecord)?;
        let record = CredentialRecord::from_storage(vault_id, record_id, &kind, nonce, ciphertext)
            .map_err(|_| StoreError::VaultRecord)?;
        credentials.push(record);
    }
    let snapshot = VaultSnapshot {
        owner,
        revision: revision as u64,
        vault_id: Some(vault_id),
        envelope: Some(envelope),
        credentials,
        deleted,
    };
    encode(&snapshot)?;
    Ok(snapshot)
}
pub(crate) fn save(c: &Transaction<'_>, snapshot: &VaultSnapshot) -> Result<()> {
    encode(snapshot)?;
    let id = snapshot.vault_id.ok_or(StoreError::VaultRecord)?;
    let envelope = snapshot
        .envelope
        .as_ref()
        .ok_or(StoreError::VaultRecord)?
        .encode_ciphertext()
        .map_err(|_| StoreError::VaultRecord)?;
    c.execute("INSERT INTO vault_envelopes(slot,id,revision,format,envelope) VALUES(1,?1,?2,1,?3) ON CONFLICT(slot) DO UPDATE SET revision=excluded.revision,envelope=excluded.envelope",rusqlite::params![id.to_string(),snapshot.revision as i64,envelope]).map_err(|_|StoreError::Database)?;
    c.execute("DELETE FROM secrets", [])
        .map_err(|_| StoreError::Database)?;
    c.execute("DELETE FROM vault_deleted_credentials", [])
        .map_err(|_| StoreError::Database)?;
    for record in &snapshot.credentials {
        let (nonce, ciphertext) = record.storage_parts();
        c.execute("INSERT INTO secrets(id,vault_id,kind,record_version,nonce,ciphertext) VALUES(?1,?2,?3,1,?4,?5)",rusqlite::params![record.id().to_string(),id.to_string(),record.kind(),nonce.as_slice(),ciphertext]).map_err(|_|StoreError::Database)?;
    }
    for deleted in &snapshot.deleted {
        c.execute(
            "INSERT INTO vault_deleted_credentials(id,vault_id) VALUES(?1,?2)",
            rusqlite::params![deleted.to_string(), id.to_string()],
        )
        .map_err(|_| StoreError::Database)?;
    }
    Ok(())
}
pub(crate) fn import_legacy(transaction: &Transaction<'_>) -> Result<()> {
    let count:i64=transaction.query_row("SELECT (SELECT count(*) FROM (SELECT 1 FROM vault_envelopes LIMIT 2))+(SELECT count(*) FROM (SELECT 1 FROM secrets LIMIT 257))+(SELECT count(*) FROM (SELECT 1 FROM vault_deleted_credentials LIMIT 4097))",[],|r|r.get(0)).map_err(|_|StoreError::Database)?;
    if count != 0 {
        return Err(StoreError::VaultConflict);
    }
    let snapshot = load_legacy(transaction, Uuid::new_v4())?;
    if snapshot.vault_id.is_some() {
        save(transaction, &snapshot)?;
    }
    transaction
        .execute("DELETE FROM app_state WHERE key=?1", [KEY])
        .map_err(|_| StoreError::Database)?;
    Ok(())
}
