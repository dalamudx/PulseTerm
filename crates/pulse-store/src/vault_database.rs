//! Versioned encrypted Vault metadata on app_state. No decrypt/KDF or plaintext SQL.
use crate::{
    Result, StoreError,
    vault_record::{CredentialRecord, RECORD_BYTES_LIMIT},
    vault_wrap::ValidationWrappedDek,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
fn deleted_rows<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<String>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("bounded deleted credential identities")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut result = Vec::new();
            while let Some(id) = seq.next_element::<String>()? {
                if result.len() >= MAX_DELETED || id.len() != 36 {
                    return Err(serde::de::Error::custom("vault.deleted_budget"));
                }
                result.push(id);
            }
            Ok(result)
        }
    }
    d.deserialize_seq(V)
}
pub(crate) const KEY: &str = "vault.ciphertext.v1";
const DOCUMENT_LIMIT: usize = 4_194_304;
const MAX_CREDENTIALS: usize = 256;
const MAX_DELETED: usize = 4096;
/// Instance-bound durable capture, including absent Vault state. No Debug/serde/keys.
#[derive(Clone)]
pub struct VaultSnapshot {
    pub(crate) owner: Uuid,
    revision: u64,
    vault_id: Option<Uuid>,
    envelope: Option<ValidationWrappedDek>,
    credentials: Vec<CredentialRecord>,
    deleted: Vec<Uuid>,
}
impl VaultSnapshot {
    /// Opaque source-instance equality only; not authorization or current-version equality.
    pub fn same_store(&self, other: &Self) -> bool {
        self.owner == other.owner
    }
    pub(crate) fn credential_deleted(&self, id: Uuid) -> bool {
        self.deleted.contains(&id)
    }
    /// Monotonic durable revision, not a permission or unlock epoch.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Opaque persisted identity; only password authentication validates its wrapped DEK.
    pub fn vault_id(&self) -> Option<Uuid> {
        self.vault_id
    }
    /// Derive bounded presence metadata from this validated ciphertext source only.
    /// A caller-provided has_value flag is never trusted; no tag authentication/use permission.
    /// Missing Vault returns None; deleted/missing credentials return a false presence reference.
    pub fn secret_ref(
        &self,
        id: Uuid,
        kind: pulse_core::secret_ref::SecretKind,
    ) -> Result<Option<pulse_core::secret_ref::SecretRef>> {
        if id.is_nil() {
            return Err(StoreError::VaultRecord);
        }
        let Some(vault_id) = self.vault_id else {
            return Ok(None);
        };
        let record = self.credentials.iter().find(|r| r.id() == id);
        if record.is_some_and(|r| r.kind() != kind.as_str()) {
            return Err(StoreError::VaultConflict);
        }
        Ok(Some(
            pulse_core::secret_ref::SecretRef::new(vault_id, id, kind, record.is_some())
                .map_err(|_| StoreError::VaultRecord)?,
        ))
    }
    /// Encrypted envelope only. Missing is typed absence, not an automatic create instruction.
    pub fn envelope(&self) -> Option<&ValidationWrappedDek> {
        self.envelope.as_ref()
    }
    /// Validated ciphertext shape, not authenticated credentials or frontend-readable plaintext.
    pub fn credentials(&self) -> &[CredentialRecord] {
        &self.credentials
    }
}
/// Trusted backend intent; neither key availability nor credential-use authorization.
pub enum VaultEdit {
    /// Explicit one-time initialization only for a same-store absent Vault capture.
    Initialize(Uuid, ValidationWrappedDek),
    /// Add a previously absent credential UUID, never overwrite an existing reference.
    Create(CredentialRecord),
    /// Replace ciphertext for an existing credential; no implicit resurrection.
    Replace(CredentialRecord),
    /// Explicitly remove an existing credential; keep Vault/envelope and revision.
    Remove(Uuid),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    revision: u64,
    vault_id: String,
    envelope: Blob,
    #[serde(deserialize_with = "rows")]
    credentials: Vec<Blob>,
    #[serde(deserialize_with = "deleted_rows")]
    deleted: Vec<String>,
}
#[derive(Serialize)]
#[serde(transparent)]
struct Blob(Vec<u8>);
impl<'de> Deserialize<'de> for Blob {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Blob;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bounded encrypted bytes")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Blob, A::Error> {
                if seq.size_hint().is_some_and(|n| n > RECORD_BYTES_LIMIT) {
                    return Err(serde::de::Error::custom("vault.blob_budget"));
                }
                let mut bytes = Vec::new();
                while let Some(b) = seq.next_element::<u8>()? {
                    if bytes.len() >= RECORD_BYTES_LIMIT {
                        return Err(serde::de::Error::custom("vault.blob_budget"));
                    }
                    bytes.push(b);
                }
                Ok(Blob(bytes))
            }
        }
        d.deserialize_seq(V)
    }
}
fn rows<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Vec<Blob>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Vec<Blob>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("bounded credential list")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            if seq.size_hint().is_some_and(|n| n > MAX_CREDENTIALS) {
                return Err(serde::de::Error::custom("vault.row_budget"));
            }
            let mut result = Vec::new();
            while let Some(row) = seq.next_element::<Blob>()? {
                if result.len() >= MAX_CREDENTIALS {
                    return Err(serde::de::Error::custom("vault.row_budget"));
                }
                result.push(row);
            }
            Ok(result)
        }
    }
    d.deserialize_seq(V)
}
fn encode(snapshot: &VaultSnapshot) -> Result<String> {
    if snapshot.revision == 0
        || snapshot.revision > i64::MAX as u64
        || snapshot.credentials.len() > MAX_CREDENTIALS
    {
        return Err(StoreError::VaultRecord);
    }
    let id = snapshot.vault_id.ok_or(StoreError::VaultRecord)?;
    let envelope = snapshot
        .envelope
        .as_ref()
        .ok_or(StoreError::VaultRecord)?
        .encode_ciphertext()
        .map_err(|_| StoreError::VaultRecord)?;
    let credentials = snapshot
        .credentials
        .iter()
        .map(|r| {
            r.encode_ciphertext()
                .map(Blob)
                .map_err(|_| StoreError::VaultRecord)
        })
        .collect::<Result<Vec<_>>>()?;
    let value = serde_json::to_string(&Document {
        version: 1,
        revision: snapshot.revision,
        vault_id: id.to_string(),
        envelope: Blob(envelope),
        credentials,
        deleted: snapshot.deleted.iter().map(Uuid::to_string).collect(),
    })
    .map_err(|_| StoreError::VaultRecord)?;
    if value.len() > DOCUMENT_LIMIT {
        return Err(StoreError::VaultRecord);
    }
    Ok(value)
}
fn decode(owner: Uuid, bytes: &[u8]) -> Result<VaultSnapshot> {
    if bytes.len() > DOCUMENT_LIMIT {
        return Err(StoreError::VaultRecord);
    }
    let doc: Document = serde_json::from_slice(bytes).map_err(|_| StoreError::VaultRecord)?;
    if doc.version != 1 || doc.revision == 0 || doc.revision > i64::MAX as u64 {
        return Err(StoreError::VaultRecord);
    }
    let id = Uuid::parse_str(&doc.vault_id).map_err(|_| StoreError::VaultRecord)?;
    if id.is_nil() || id.to_string() != doc.vault_id {
        return Err(StoreError::VaultRecord);
    }
    let envelope = ValidationWrappedDek::decode_ciphertext(&doc.envelope.0, id)
        .map_err(|_| StoreError::VaultRecord)?;
    let mut deleted = Vec::with_capacity(doc.deleted.len());
    for value in doc.deleted {
        let id = Uuid::parse_str(&value).map_err(|_| StoreError::VaultRecord)?;
        if id.is_nil() || id.to_string() != value || deleted.contains(&id) {
            return Err(StoreError::VaultRecord);
        }
        deleted.push(id);
    }
    let mut credentials = Vec::<CredentialRecord>::with_capacity(doc.credentials.len());
    for bytes in doc.credentials {
        let row = CredentialRecord::decode_ciphertext(&bytes.0, id)
            .map_err(|_| StoreError::VaultRecord)?;
        if deleted.contains(&row.id()) || credentials.iter().any(|r| r.id() == row.id()) {
            return Err(StoreError::VaultRecord);
        }
        credentials.push(row);
    }
    Ok(VaultSnapshot {
        owner,
        revision: doc.revision,
        vault_id: Some(id),
        envelope: Some(envelope),
        credentials,
        deleted,
    })
}
pub(crate) fn load(connection: &Connection, owner: Uuid) -> Result<VaultSnapshot> {
    if !relational::exists(connection)? {
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| StoreError::Schema)?;
        if version >= 2 {
            return Err(StoreError::VaultRecord);
        }
        return load_legacy(connection, owner);
    }
    if !connection.is_autocommit() {
        return relational::load_rows(connection, owner);
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    relational::load_rows(&transaction, owner)
}
fn load_legacy(connection: &Connection, owner: Uuid) -> Result<VaultSnapshot> {
    let row:Option<(i64,Vec<u8>)>=connection.query_row("SELECT length(CAST(value AS BLOB)),substr(CAST(value AS BLOB),1,4194305) FROM app_state WHERE key=?1",[KEY],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|StoreError::Database)?;
    match row {
        None => Ok(VaultSnapshot {
            owner,
            revision: 0,
            vault_id: None,
            envelope: None,
            credentials: Vec::new(),
            deleted: Vec::new(),
        }),
        Some((len, bytes)) if (0..=DOCUMENT_LIMIT as i64).contains(&len) => decode(owner, &bytes),
        _ => Err(StoreError::VaultRecord),
    }
}
/// Compare a validated read capture, including legitimate absence; not unlock or source-use authority.
#[cfg(feature = "ssh-trust")]
pub(crate) fn same_capture(a: &VaultSnapshot, b: &VaultSnapshot) -> Result<bool> {
    if a.owner != b.owner {
        return Ok(false);
    }
    match (a.vault_id, b.vault_id) {
        (None, None) => Ok(a.revision == 0
            && b.revision == 0
            && a.envelope.is_none()
            && b.envelope.is_none()
            && a.credentials.is_empty()
            && b.credentials.is_empty()
            && a.deleted.is_empty()
            && b.deleted.is_empty()),
        (Some(_), Some(_)) => same_source(a, b),
        _ => Ok(false),
    }
}

pub(crate) fn same_source(a: &VaultSnapshot, b: &VaultSnapshot) -> Result<bool> {
    if a.owner != b.owner {
        return Ok(false);
    }
    let canonical = |snapshot: &VaultSnapshot| -> Result<String> {
        let mut s = snapshot.clone();
        s.credentials.sort_by_key(|r| r.id());
        s.deleted.sort();
        encode(&s)
    };
    Ok(canonical(a)? == canonical(b)?)
}
pub(crate) fn confirm_unlock(
    connection: &Connection,
    owner: Uuid,
    expected: &VaultSnapshot,
    scope: &crate::vault_lifecycle::VaultKeyLifecycle,
    ready: crate::vault_unlock::ReadyUnlock,
    authorize: impl Fn() -> bool,
) -> Result<crate::vault_lifecycle::UnlockActivation> {
    if expected.owner != owner
        || expected.vault_id.is_none()
        || expected.vault_id != Some(scope.expected_id()?)
    {
        return Err(StoreError::VaultConflict);
    }
    if !authorize() {
        return Err(StoreError::VaultAuthorization);
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = load(&transaction, owner)?;
    let canonical = |snapshot: &VaultSnapshot| -> Result<String> {
        let mut s = snapshot.clone();
        s.credentials.sort_by_key(|r| r.id());
        s.deleted.sort();
        encode(&s)
    };
    if canonical(&current)? != canonical(expected)? {
        return Err(StoreError::VaultConflict);
    }
    let envelope = current
        .envelope
        .as_ref()
        .ok_or(StoreError::VaultRecord)?
        .encode_ciphertext()
        .map_err(|_| StoreError::VaultRecord)?;
    if !authorize() {
        return Err(StoreError::VaultAuthorization);
    }
    let activation = ready.confirm_bound(scope, &envelope)?;
    transaction.rollback().map_err(|_| StoreError::Database)?;
    Ok(activation)
}

pub(crate) fn edit(
    connection: &Connection,
    owner: Uuid,
    expected: &VaultSnapshot,
    edit: VaultEdit,
    authorize: impl Fn() -> bool,
) -> Result<VaultSnapshot> {
    edit_preflight(connection, owner, expected, edit, authorize, |_, _| Ok(()))
}
pub(crate) fn edit_preflight(
    connection: &Connection,
    owner: Uuid,
    expected: &VaultSnapshot,
    edit: VaultEdit,
    authorize: impl Fn() -> bool,
    preflight: impl FnOnce(&Connection, &VaultSnapshot) -> Result<()>,
) -> Result<VaultSnapshot> {
    edit_batch_preflight(
        connection,
        owner,
        expected,
        vec![edit],
        authorize,
        preflight,
    )
}
pub(crate) fn edit_batch_preflight(
    connection: &Connection,
    owner: Uuid,
    expected: &VaultSnapshot,
    edits: Vec<VaultEdit>,
    authorize: impl Fn() -> bool,
    preflight: impl FnOnce(&Connection, &VaultSnapshot) -> Result<()>,
) -> Result<VaultSnapshot> {
    if !(1..=2).contains(&edits.len())
        || (edits.len() != 1
            && edits
                .iter()
                .any(|edit| matches!(edit, VaultEdit::Initialize(_, _))))
    {
        return Err(StoreError::VaultRecord);
    }
    if expected.owner != owner {
        return Err(StoreError::VaultConflict);
    }
    if !authorize() {
        return Err(StoreError::VaultAuthorization);
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let mut current = load(&transaction, owner)?;
    if current.revision != expected.revision {
        return Err(StoreError::VaultConflict);
    }
    preflight(&transaction, &current)?;
    for edit in edits {
        match edit {
            VaultEdit::Initialize(id, envelope) => {
                if current.vault_id.is_some() {
                    return Err(StoreError::VaultConflict);
                }
                let bytes = envelope
                    .encode_ciphertext()
                    .map_err(|_| StoreError::VaultRecord)?;
                let envelope = ValidationWrappedDek::decode_ciphertext(&bytes, id)
                    .map_err(|_| StoreError::VaultRecord)?;
                current.vault_id = Some(id);
                current.envelope = Some(envelope);
            }
            VaultEdit::Create(record) => {
                let id = current.vault_id.ok_or(StoreError::VaultConflict)?;
                let bytes = record
                    .encode_ciphertext()
                    .map_err(|_| StoreError::VaultRecord)?;
                let record = CredentialRecord::decode_ciphertext(&bytes, id)
                    .map_err(|_| StoreError::VaultRecord)?;
                if current.credentials.len() >= MAX_CREDENTIALS
                    || current.deleted.contains(&record.id())
                    || current.credentials.iter().any(|r| r.id() == record.id())
                {
                    return Err(StoreError::VaultConflict);
                }
                current.credentials.push(record);
            }
            VaultEdit::Replace(record) => {
                let id = current.vault_id.ok_or(StoreError::VaultConflict)?;
                let bytes = record
                    .encode_ciphertext()
                    .map_err(|_| StoreError::VaultRecord)?;
                let record = CredentialRecord::decode_ciphertext(&bytes, id)
                    .map_err(|_| StoreError::VaultRecord)?;
                let old = current
                    .credentials
                    .iter_mut()
                    .find(|r| r.id() == record.id())
                    .ok_or(StoreError::VaultConflict)?;
                if old.kind() != record.kind() {
                    return Err(StoreError::VaultConflict);
                }
                *old = record;
            }
            VaultEdit::Remove(id) => {
                let pos = current
                    .credentials
                    .iter()
                    .position(|r| r.id() == id)
                    .ok_or(StoreError::VaultConflict)?;
                if current.deleted.len() >= MAX_DELETED {
                    return Err(StoreError::VaultRecord);
                }
                current.credentials.remove(pos);
                current.deleted.push(id);
            }
        }
    }
    current.revision = current
        .revision
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or(StoreError::VaultRevision)?;
    let value = encode(&current)?;
    if !authorize() {
        return Err(StoreError::VaultAuthorization);
    }
    if relational::exists(&transaction)? {
        relational::save(&transaction, &current)?;
    } else {
        transaction.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![KEY,value]).map_err(|_|StoreError::Database)?;
    }
    if !authorize() {
        return Err(StoreError::VaultAuthorization);
    }
    transaction.commit().map_err(|_| StoreError::Database)?;
    Ok(current)
}

#[path = "vault_relational.rs"]
pub(crate) mod relational;
