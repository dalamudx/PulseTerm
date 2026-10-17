//! Bounded, independently identified local Profiles. No process or page authority.
use crate::{Request, Result, StateStore, StoreError, profile_editor::ProfileConfig};
use pulse_core::{hlc::Hlc, host_catalogue::RecordIdentity};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;
use uuid::Uuid;
const KEY: &str = "profiles.custom.v1";
const MAX_RECORDS: usize = 64;
const MAX_BYTES: usize = 512 * 1024;
/// Literal metadata supplied by an explicitly authorized editor, never a launch request.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomProfileDraft {
    /// User label; not a command or path.
    pub name: String,
    /// Absolute literal executable path; no shell, tilde or environment expansion.
    pub executable: String,
    /// Hide from the sidebar while preserving explicit selection and saved identity.
    #[serde(default)]
    pub hidden: bool,
    /// Existing bounded literal arguments, cwd, environment and drawing preferences.
    pub config: ProfileConfig,
}
impl CustomProfileDraft {
    /// Validate before persistence and again before launch; no file access or execution.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || self.name.chars().any(char::is_control)
            || self.executable.len() > 4096
            || self.executable.chars().any(char::is_control)
            || !std::path::Path::new(&self.executable).is_absolute()
        {
            return Err(StoreError::Record);
        }
        self.config.validate()
    }
}
/// Backend-generated UUIDv7 and reserved sync metadata, retained on deletion and restoration.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomProfile {
    #[serde(with = "uuid_string")]
    id: Uuid,
    created_at: u64,
    clock: String,
    deleted: bool,
    draft: CustomProfileDraft,
}
impl CustomProfile {
    /// Opaque persistent identity, not permission to execute.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Literal saved metadata; never log arguments, environment or paths.
    pub fn draft(&self) -> &CustomProfileDraft {
        &self.draft
    }
    /// Tombstones retain identities and are excluded from launch offers.
    pub fn is_deleted(&self) -> bool {
        self.deleted
    }
    /// Validated creation/HLC/tombstone metadata for future synchronization.
    pub fn identity(&self) -> Result<RecordIdentity> {
        RecordIdentity::new(
            self.id,
            self.created_at,
            Hlc::parse(&self.clock).map_err(|_| StoreError::Record)?,
            self.deleted,
        )
        .map_err(|_| StoreError::Record)
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u8,
    revision: u64,
    #[serde(with = "uuid_string")]
    node: Uuid,
    profiles: Vec<CustomProfile>,
}
// Encode canonical UUID strings without relying on uuid feature unification.
mod uuid_string {
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;
    pub fn serialize<S: Serializer>(id: &Uuid, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(id)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Uuid, D::Error> {
        let value = String::deserialize(deserializer)?;
        let id = Uuid::parse_str(&value).map_err(serde::de::Error::custom)?;
        if id.to_string() != value {
            return Err(serde::de::Error::custom("profile.uuid_invalid"));
        }
        Ok(id)
    }
}
#[cfg(test)]
#[path = "custom_profiles_tests.rs"]
mod tests;
/// Store-instance-bound CAS source; callers cannot manufacture a persistence snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct CustomProfileSnapshot {
    owner: Uuid,
    record: Option<Record>,
}
impl CustomProfileSnapshot {
    /// Includes tombstones for explicit restore; normal launch offers must exclude them.
    pub fn profiles(&self) -> &[CustomProfile] {
        self.record
            .as_ref()
            .map_or(&[], |record| record.profiles.as_slice())
    }
    /// Only an active saved identity can be selected for a new session.
    pub fn active(&self, id: Uuid) -> Result<&CustomProfile> {
        self.profiles()
            .iter()
            .find(|p| p.id == id && !p.deleted)
            .ok_or(StoreError::Record)
    }
}
/// Explicit one-record metadata change; no path is accepted by session-start commands.
pub enum CustomProfileEdit {
    /// Generate the identity and sync stamp in storage.
    Create(CustomProfileDraft),
    /// Replace literal configuration against the original catalogue snapshot.
    Update(Uuid, CustomProfileDraft),
    /// Retain identity and sync metadata without offering the Profile for new sessions.
    Delete(Uuid),
    /// Restore the same saved identity; never generate a replacement.
    Restore(Uuid),
}
type Gate = Arc<dyn Fn() -> bool + Send + Sync>;
pub(crate) enum CustomRequest {
    Load(
        Uuid,
        Box<dyn Send>,
        oneshot::Sender<Result<CustomProfileSnapshot>>,
    ),
    Edit(
        Box<CustomProfileSnapshot>,
        Box<CustomProfileEdit>,
        Gate,
        oneshot::Sender<Result<CustomProfileSnapshot>>,
    ),
}
impl StateStore {
    /// Keep I/O admission alive through actual worker completion, including cancelled callers.
    pub async fn load_custom_profiles_retaining<R: Send + 'static>(
        &self,
        resource: R,
    ) -> Result<CustomProfileSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::CustomProfiles(Box::new(CustomRequest::Load(
            self.host_owner,
            Box::new(resource),
            reply,
        ))))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Capture validated records without granting launch or editor authority.
    pub async fn load_custom_profiles(&self) -> Result<CustomProfileSnapshot> {
        self.load_custom_profiles_retaining(()).await
    }
    /// Compare the original store and catalogue, then recheck original authorization at commit.
    pub async fn edit_custom_profiles(
        &self,
        expected: CustomProfileSnapshot,
        edit: CustomProfileEdit,
        allowed: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<CustomProfileSnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::Conflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::CustomProfiles(Box::new(CustomRequest::Edit(
            Box::new(expected),
            Box::new(edit),
            Arc::new(allowed),
            reply,
        ))))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
pub(crate) fn read(connection: &Connection, owner: Uuid) -> Result<CustomProfileSnapshot> {
    let raw: Option<(i64, String)> = connection
        .query_row(
            "SELECT length(CAST(value AS BLOB)), substr(value,1,?2) FROM app_state WHERE key=?1",
            rusqlite::params![
                KEY,
                i64::try_from(MAX_BYTES + 1).map_err(|_| StoreError::Record)?
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| StoreError::Database)?;
    let record = raw
        .map(|(size, raw)| -> Result<Record> {
            if !(1..=MAX_BYTES as i64).contains(&size) || raw.len() > MAX_BYTES {
                return Err(StoreError::Record);
            }
            let record: Record = serde_json::from_str(&raw).map_err(|_| StoreError::Record)?;
            validate(&record)?;
            Ok(record)
        })
        .transpose()?;
    Ok(CustomProfileSnapshot { owner, record })
}
fn validate(record: &Record) -> Result<()> {
    if record.schema_version != 1
        || record.revision == 0
        || record.revision > i64::MAX as u64
        || record.node.is_nil()
        || record.profiles.len() > MAX_RECORDS
    {
        return Err(StoreError::Record);
    }
    let mut ids = HashSet::new();
    for profile in &record.profiles {
        profile.draft.validate()?;
        let identity = profile.identity()?;
        if identity.clock().node() != record.node || !ids.insert(profile.id) {
            return Err(StoreError::Record);
        }
    }
    Ok(())
}
fn edit(
    connection: &Connection,
    expected: CustomProfileSnapshot,
    edit: CustomProfileEdit,
    allowed: &dyn Fn() -> bool,
) -> Result<CustomProfileSnapshot> {
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    let tx = rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = read(&tx, expected.owner)?;
    if current != expected {
        return Err(StoreError::Conflict);
    }
    let now = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Record)?
            .as_millis(),
    )
    .map_err(|_| StoreError::Record)?;
    let mut record = current.record.unwrap_or_else(|| Record {
        schema_version: 1,
        revision: 0,
        node: Uuid::now_v7(),
        profiles: Vec::new(),
    });
    record.revision = record.revision.checked_add(1).ok_or(StoreError::Record)?;
    match edit {
        CustomProfileEdit::Create(draft) => {
            draft.validate()?;
            if record.profiles.len() >= MAX_RECORDS {
                return Err(StoreError::Record);
            }
            let clock = Hlc::new(now, 0, record.node).map_err(|_| StoreError::Record)?;
            record.profiles.push(CustomProfile {
                id: Uuid::now_v7(),
                created_at: now,
                clock: clock.encode(),
                deleted: false,
                draft,
            });
        }
        edit => {
            let id = match &edit {
                CustomProfileEdit::Update(id, _)
                | CustomProfileEdit::Delete(id)
                | CustomProfileEdit::Restore(id) => *id,
                _ => return Err(StoreError::Record),
            };
            let profile = record
                .profiles
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or(StoreError::Record)?;
            match edit {
                CustomProfileEdit::Update(_, draft) if !profile.deleted => {
                    draft.validate()?;
                    profile.draft = draft;
                }
                CustomProfileEdit::Delete(_) if !profile.deleted => profile.deleted = true,
                CustomProfileEdit::Restore(_) if profile.deleted => profile.deleted = false,
                _ => return Err(StoreError::Record),
            }
            profile.clock = Hlc::parse(&profile.clock)
                .and_then(|clock| clock.next(now))
                .map_err(|_| StoreError::Record)?
                .encode();
        }
    }
    validate(&record)?;
    let raw = serde_json::to_string(&record).map_err(|_| StoreError::Record)?;
    if raw.len() > MAX_BYTES {
        return Err(StoreError::Record);
    }
    tx.execute("INSERT INTO app_state(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![KEY, raw]).map_err(|_| StoreError::Database)?;
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(CustomProfileSnapshot {
        owner: expected.owner,
        record: Some(record),
    })
}
pub(crate) fn serve(connection: &Connection, request: CustomRequest) {
    match request {
        CustomRequest::Load(owner, resource, reply) => {
            if !reply.is_closed() {
                let _ = reply.send(read(connection, owner));
            }
            drop(resource);
        }
        CustomRequest::Edit(expected, change, allowed, reply) => {
            if !reply.is_closed() {
                let _ = reply.send(edit(connection, *expected, *change, &*allowed));
            }
        }
    }
}
