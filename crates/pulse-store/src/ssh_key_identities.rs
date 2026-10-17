//! Local identity rows retain sync metadata, but do not implement synchronization or authorization.
use crate::{Result, StoreError, vault_database};
use pulse_core::{hlc::Hlc, secret_ref::SecretKind, ssh_key_identity::SshPrivateKeyIdentity};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;
const MAX_ACTIVE: usize = 256;
const MAX_DELETED: usize = 4096;
pub(crate) const SCHEMA:&str="CREATE TABLE key_identity_catalog(slot INTEGER PRIMARY KEY CHECK(slot=1),format INTEGER NOT NULL CHECK(format=1),revision INTEGER NOT NULL CHECK(revision>0),node_id TEXT NOT NULL CHECK(length(node_id)=36),clock TEXT NOT NULL CHECK(length(clock) BETWEEN 1 AND 67)) STRICT;
CREATE TABLE key_identities(id TEXT PRIMARY KEY CHECK(length(id)=36),created_at INTEGER NOT NULL CHECK(created_at>0),hlc TEXT NOT NULL CHECK(length(hlc) BETWEEN 1 AND 67),deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN (0,1)),label TEXT NOT NULL CHECK(length(label) BETWEEN 1 AND 256),username TEXT NOT NULL CHECK(length(username) BETWEEN 1 AND 255),vault_id TEXT NOT NULL REFERENCES vault_envelopes(id),secret_id TEXT NOT NULL CHECK(length(secret_id)=36),kind TEXT NOT NULL CHECK(kind='ssh_private_key'),format INTEGER NOT NULL CHECK(format=1),passphrase_id TEXT CHECK(passphrase_id IS NULL OR length(passphrase_id)=36)) STRICT;";
/// Private-row-derived identity metadata; IDs/stamps are not permission or wall-clock trust.
#[derive(Clone, PartialEq, Eq)]
pub struct SavedSshKeyIdentity {
    id: Uuid,
    created_at: u64,
    stamp: Hlc,
    deleted: bool,
    value: SshPrivateKeyIdentity,
}
impl SavedSshKeyIdentity {
    /// Backend-created UUIDv7, never a path or authentication ticket.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Backend creation timestamp preserved across edits/deletion.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    /// Row stamp for future sync merging; no sync transport is implemented.
    pub fn stamp(&self) -> Hlc {
        self.stamp
    }
    /// Bounded label/login/reference, not decrypted secret material.
    pub fn identity(&self) -> &SshPrivateKeyIdentity {
        &self.value
    }
}
/// Store-instance-bound CAS snapshot. Includes hidden tombstones internally; no Debug/serde.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyIdentitySnapshot {
    pub(crate) owner: Uuid,
    revision: u64,
    vault_id: Option<Uuid>,
    clock: Option<Hlc>,
    rows: Vec<SavedSshKeyIdentity>,
}
impl KeyIdentitySnapshot {
    /// Vault namespace captured at the same transaction boundary, not unlock state.
    pub fn vault_id(&self) -> Option<Uuid> {
        self.vault_id
    }
    /// Local monotonic catalog revision, not a global synchronization version.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Active metadata only; deleted UUIDs do not reappear in the editor.
    pub fn records(&self) -> impl Iterator<Item = &SavedSshKeyIdentity> {
        self.rows.iter().filter(|r| !r.deleted)
    }
}
/// Trusted native edit intent; supplied presence flags are ignored when rows are read back.
pub enum KeyIdentityEdit {
    /// Generate a new independent UUIDv7 in the backend transaction.
    Create(SshPrivateKeyIdentity),
    /// Update only an active ID, never restore a tombstone implicitly.
    Update(Uuid, SshPrivateKeyIdentity),
    /// Retain ID/created_at and advance its HLC tombstone.
    Remove(Uuid),
    /// Tombstone a bounded nonempty unique set with one revision and commit.
    RemoveMany(Vec<Uuid>),
}
fn uuid(value: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| StoreError::IdentityRecord)?;
    if id.is_nil() || id.to_string() != value {
        return Err(StoreError::IdentityRecord);
    }
    Ok(id)
}
pub(crate) fn load(c: &Connection, owner: Uuid) -> Result<KeyIdentitySnapshot> {
    if !c.is_autocommit() {
        return load_rows(c, owner);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    load_rows(&tx, owner)
}
fn load_rows(c: &Connection, owner: Uuid) -> Result<KeyIdentitySnapshot> {
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM key_identities LIMIT 4353)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::IdentityRecord)?;
    let meta_count: i64 = c
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM key_identity_catalog LIMIT 2)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::IdentityRecord)?;
    if count > 4352 || meta_count > 1 {
        return Err(StoreError::IdentityRecord);
    }
    let metadata:Option<(i64,i64,String,String)>=c.query_row("SELECT format,revision,substr(node_id,1,37),substr(clock,1,68) FROM key_identity_catalog WHERE slot=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_|StoreError::IdentityRecord)?;
    let vault = vault_database::load(c, owner).map_err(|_| StoreError::IdentityRecord)?;
    let Some((format, revision, node, clock)) = metadata else {
        if count != 0 || meta_count != 0 {
            return Err(StoreError::IdentityRecord);
        }
        return Ok(KeyIdentitySnapshot {
            owner,
            revision: 0,
            vault_id: vault.vault_id(),
            clock: None,
            rows: Vec::new(),
        });
    };
    if format != 1 || revision <= 0 {
        return Err(StoreError::IdentityRecord);
    }
    let clock = Hlc::parse(&clock).map_err(|_| StoreError::IdentityRecord)?;
    if clock.node() != uuid(&node)? {
        return Err(StoreError::IdentityRecord);
    }
    let mut statement=c.prepare("SELECT substr(id,1,37),created_at,substr(hlc,1,68),deleted,substr(label,1,257),substr(username,1,256),substr(vault_id,1,37),substr(secret_id,1,37),substr(kind,1,33),format,substr(passphrase_id,1,37) FROM key_identities ORDER BY id LIMIT 4353").map_err(|_|StoreError::IdentityRecord)?;
    let mut rows = statement
        .query([])
        .map_err(|_| StoreError::IdentityRecord)?;
    let mut values = Vec::new();
    let mut deleted_count = 0usize;
    let mut bytes = 0usize;
    while let Some(row) = rows.next().map_err(|_| StoreError::IdentityRecord)? {
        let raw: String = row.get(0).map_err(|_| StoreError::IdentityRecord)?;
        let id = uuid(&raw)?;
        if id.get_version_num() != 7 || values.iter().any(|r: &SavedSshKeyIdentity| r.id == id) {
            return Err(StoreError::IdentityRecord);
        }
        let created: i64 = row.get(1).map_err(|_| StoreError::IdentityRecord)?;
        let raw_stamp: String = row.get(2).map_err(|_| StoreError::IdentityRecord)?;
        let stamp = Hlc::parse(&raw_stamp).map_err(|_| StoreError::IdentityRecord)?;
        let deleted: i64 = row.get(3).map_err(|_| StoreError::IdentityRecord)?;
        let label: String = row.get(4).map_err(|_| StoreError::IdentityRecord)?;
        let username: String = row.get(5).map_err(|_| StoreError::IdentityRecord)?;
        let vault_id: String = row.get(6).map_err(|_| StoreError::IdentityRecord)?;
        let secret_id: String = row.get(7).map_err(|_| StoreError::IdentityRecord)?;
        let kind: String = row.get(8).map_err(|_| StoreError::IdentityRecord)?;
        let format: i64 = row.get(9).map_err(|_| StoreError::IdentityRecord)?;
        if created <= 0
            || created as u64 > stamp.wall()
            || stamp > clock
            || stamp.node() != clock.node()
            || !matches!(deleted, 0 | 1)
            || format != 1
            || kind != "ssh_private_key"
            || vault.vault_id() != Some(uuid(&vault_id)?)
        {
            return Err(StoreError::IdentityRecord);
        }
        let reference = vault
            .secret_ref(uuid(&secret_id)?, SecretKind::SshPrivateKey)
            .map_err(|_| StoreError::IdentityRecord)?
            .ok_or(StoreError::IdentityRecord)?;
        let passphrase_id: Option<String> = row.get(10).map_err(|_| StoreError::IdentityRecord)?;
        let passphrase = passphrase_id
            .as_deref()
            .map(|id| {
                vault
                    .secret_ref(uuid(id)?, SecretKind::SshKeyPassphrase)
                    .map_err(|_| StoreError::IdentityRecord)?
                    .ok_or(StoreError::IdentityRecord)
            })
            .transpose()?;
        let value = SshPrivateKeyIdentity::new(&label, &username, reference, passphrase)
            .map_err(|_| StoreError::IdentityRecord)?;
        bytes = bytes
            .checked_add(
                label.len()
                    + username.len()
                    + raw_stamp.len()
                    + raw.len()
                    + vault_id.len()
                    + secret_id.len()
                    + kind.len()
                    + passphrase_id.as_ref().map_or(0, String::len),
            )
            .ok_or(StoreError::IdentityRecord)?;
        if bytes > 4_194_304 {
            return Err(StoreError::IdentityRecord);
        }
        deleted_count += usize::from(deleted == 1);
        values.push(SavedSshKeyIdentity {
            id,
            created_at: created as u64,
            stamp,
            deleted: deleted == 1,
            value,
        });
    }
    if deleted_count > MAX_DELETED || values.len() - deleted_count > MAX_ACTIVE {
        return Err(StoreError::IdentityRecord);
    }
    Ok(KeyIdentitySnapshot {
        owner,
        revision: revision as u64,
        vault_id: vault.vault_id(),
        clock: Some(clock),
        rows: values,
    })
}
pub(crate) fn same_metadata(a: &KeyIdentitySnapshot, b: &KeyIdentitySnapshot) -> bool {
    a.revision == b.revision
        && a.vault_id == b.vault_id
        && a.clock == b.clock
        && a.rows.len() == b.rows.len()
        && a.rows.iter().zip(&b.rows).all(|(a, b)| {
            a.id == b.id
                && a.created_at == b.created_at
                && a.stamp == b.stamp
                && a.deleted == b.deleted
                && a.value.label() == b.value.label()
                && a.value.username() == b.value.username()
                && a.value.private_key().id() == b.value.private_key().id()
                && a.value.private_key().vault_id() == b.value.private_key().vault_id()
                && a.value.passphrase().map(|r| (r.vault_id(), r.id()))
                    == b.value.passphrase().map(|r| (r.vault_id(), r.id()))
        })
}
pub(crate) fn edit(
    c: &Connection,
    owner: Uuid,
    expected: &KeyIdentitySnapshot,
    intent: KeyIdentityEdit,
    authorize: impl Fn() -> bool,
) -> Result<KeyIdentitySnapshot> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::IdentityExhausted)?
        .as_millis()
        .try_into()
        .map_err(|_| StoreError::IdentityExhausted)?;
    edit_at(c, owner, expected, intent, now, authorize)
}
pub(crate) fn edit_at(
    c: &Connection,
    owner: Uuid,
    expected: &KeyIdentitySnapshot,
    intent: KeyIdentityEdit,
    now: u64,
    authorize: impl Fn() -> bool,
) -> Result<KeyIdentitySnapshot> {
    if expected.owner != owner {
        return Err(StoreError::IdentityConflict);
    }
    if !authorize() {
        return Err(StoreError::IdentityAuthorization);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let mut current = load_rows(&tx, owner)?;
    if !same_metadata(&current, expected) {
        return Err(StoreError::IdentityConflict);
    }
    let vault = vault_database::load(&tx, owner).map_err(|_| StoreError::IdentityRecord)?;
    let stamp = match current.clock {
        Some(h) => h.next(now),
        None => Hlc::new(now, 0, Uuid::now_v7()),
    }
    .map_err(|_| StoreError::IdentityExhausted)?;
    match intent {
        KeyIdentityEdit::Create(value) => {
            if current.records().count() >= MAX_ACTIVE {
                return Err(StoreError::IdentityExhausted);
            }
            validate_reference(&vault, &value, None)?;
            current.rows.push(SavedSshKeyIdentity {
                id: Uuid::now_v7(),
                created_at: stamp.wall(),
                stamp,
                deleted: false,
                value,
            });
        }
        KeyIdentityEdit::Update(id, value) => {
            let row = current
                .rows
                .iter_mut()
                .find(|r| r.id == id && !r.deleted)
                .ok_or(StoreError::IdentityConflict)?;
            validate_reference(&vault, &value, Some(&row.value))?;
            row.value = value;
            row.stamp = stamp;
        }
        KeyIdentityEdit::RemoveMany(ids) => {
            let ids = crate::metadata_batch::unique_ids(&ids).ok_or(StoreError::IdentityRecord)?;
            if current.rows.iter().filter(|r| r.deleted).count() + ids.len() > MAX_DELETED {
                return Err(StoreError::IdentityExhausted);
            }
            if ids
                .iter()
                .any(|id| !current.rows.iter().any(|row| row.id == *id && !row.deleted))
            {
                return Err(StoreError::IdentityConflict);
            }
            for row in &mut current.rows {
                if ids.contains(&row.id) {
                    row.deleted = true;
                    row.stamp = stamp;
                }
            }
        }
        KeyIdentityEdit::Remove(id) => {
            if current.rows.iter().filter(|r| r.deleted).count() >= MAX_DELETED {
                return Err(StoreError::IdentityExhausted);
            }
            let row = current
                .rows
                .iter_mut()
                .find(|r| r.id == id && !r.deleted)
                .ok_or(StoreError::IdentityConflict)?;
            row.deleted = true;
            row.stamp = stamp;
        }
    }
    current.revision = current
        .revision
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or(StoreError::IdentityExhausted)?;
    current.clock = Some(stamp);
    tx.execute("INSERT INTO key_identity_catalog(slot,format,revision,node_id,clock) VALUES(1,1,?1,?2,?3) ON CONFLICT(slot) DO UPDATE SET revision=excluded.revision,clock=excluded.clock",rusqlite::params![current.revision as i64,stamp.node().to_string(),stamp.encode()]).map_err(|_|StoreError::Database)?;
    for row in &current.rows {
        let r = row.value.private_key();
        tx.execute("INSERT INTO key_identities(id,created_at,hlc,deleted,label,username,vault_id,secret_id,kind,format,passphrase_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'ssh_private_key',1,?9) ON CONFLICT(id) DO UPDATE SET hlc=excluded.hlc,deleted=excluded.deleted,label=excluded.label,username=excluded.username,secret_id=excluded.secret_id,passphrase_id=excluded.passphrase_id",rusqlite::params![row.id.to_string(),row.created_at as i64,row.stamp.encode(),i64::from(row.deleted),row.value.label(),row.value.username(),r.vault_id().to_string(),r.id().to_string(),row.value.passphrase().map(|r| r.id().to_string())]).map_err(|_|StoreError::Database)?;
    }
    let committed = load_rows(&tx, owner)?;
    if !authorize() {
        return Err(StoreError::IdentityAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(committed)
}
fn validate_reference(
    vault: &vault_database::VaultSnapshot,
    value: &SshPrivateKeyIdentity,
    existing: Option<&SshPrivateKeyIdentity>,
) -> Result<()> {
    let key = value.private_key();
    if vault.vault_id() != Some(key.vault_id()) {
        return Err(StoreError::IdentityConflict);
    }
    for (reference, previous) in [
        (Some(key), existing.map(SshPrivateKeyIdentity::private_key)),
        (
            value.passphrase(),
            existing.and_then(SshPrivateKeyIdentity::passphrase),
        ),
    ] {
        if let Some(reference) = reference {
            if vault.credential_deleted(reference.id())
                && previous.is_none_or(|old| old.id() != reference.id())
            {
                return Err(StoreError::IdentityConflict);
            }
            vault
                .secret_ref(reference.id(), reference.kind())
                .map_err(|_| StoreError::IdentityConflict)?
                .ok_or(StoreError::IdentityConflict)?;
        }
    }
    Ok(())
}
