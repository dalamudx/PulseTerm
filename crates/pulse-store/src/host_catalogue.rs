//! Same-worker migration from legacy SSH rows into independent machine/entrance/Profile records.
use crate::{Request, Result, StateStore, StoreError, ssh_hosts::HostSnapshot};
use pulse_core::{
    hlc::Hlc,
    host_catalogue::{Endpoint, EndpointProtocol, Host, HostCatalogue, RecordIdentity, SshProfile},
};
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;
use uuid::Uuid;
pub(crate) const KEY: &str = "catalogue.host-endpoints.v1";
const LIMIT: usize = 1_048_576;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Meta {
    id: String,
    created_at: u64,
    updated_at: u64,
    hlc: String,
    deleted: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostRow {
    metadata: Meta,
    name: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EndpointRow {
    metadata: Meta,
    host: String,
    name: String,
    protocol: String,
    address: String,
    port: u16,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileRow {
    metadata: Meta,
    endpoint: String,
    name: String,
    username: String,
    identity: Option<String>,
    #[serde(default, skip_serializing_if = "JumpReference::absent")]
    via: JumpReference,
}
// Preserve the distinction between a v1 missing field and a v2 explicit null.
// A route field cannot be smuggled into a legacy schema or omitted from the new schema.
#[derive(Clone, Default)]
enum JumpReference {
    #[default]
    Absent,
    Present(Option<String>),
}
impl JumpReference {
    fn absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
    fn value(&self) -> Option<&str> {
        match self {
            Self::Absent => None,
            Self::Present(value) => value.as_deref(),
        }
    }
}
impl Serialize for JumpReference {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Absent => Option::<String>::None.serialize(serializer),
            Self::Present(value) => value.serialize(serializer),
        }
    }
}
impl<'de> Deserialize<'de> for JumpReference {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Option::<String>::deserialize(deserializer).map(Self::Present)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u8,
    node: String,
    clock: String,
    legacy_revision: u64,
    hosts: Vec<HostRow>,
    endpoints: Vec<EndpointRow>,
    profiles: Vec<ProfileRow>,
}
/// Read-only validated graph. Metadata does not establish source or connection permission.
#[derive(Clone)]
pub struct Snapshot {
    catalogue: HostCatalogue,
    legacy_revision: u64,
}
impl Snapshot {
    /// Machines, entrances and SSH preferences, including deletion tombstones.
    pub fn catalogue(&self) -> &HostCatalogue {
        &self.catalogue
    }
    /// Legacy projection revision; a migration invalidates prior write sources.
    pub fn legacy_revision(&self) -> u64 {
        self.legacy_revision
    }
}
fn record() -> StoreError {
    StoreError::HostRecord
}
fn id(value: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| record())?;
    if id.to_string() != value
        || id.get_version_num() != 7
        || id.get_variant() != uuid::Variant::RFC4122
    {
        return Err(record());
    }
    Ok(id)
}
fn metadata(m: &Meta) -> Result<RecordIdentity> {
    let clock = Hlc::parse(&m.hlc).map_err(|_| record())?;
    if clock.wall() != m.updated_at {
        return Err(record());
    }
    RecordIdentity::new(id(&m.id)?, m.created_at, clock, m.deleted).map_err(|_| record())
}
fn protocol(value: &str) -> Result<EndpointProtocol> {
    match value {
        "ssh" => Ok(EndpointProtocol::Ssh),
        "sftp" => Ok(EndpointProtocol::Sftp),
        "rdp" => Ok(EndpointProtocol::Rdp),
        "vnc" => Ok(EndpointProtocol::Vnc),
        "telnet" => Ok(EndpointProtocol::Telnet),
        _ => Err(record()),
    }
}
fn graph(d: &Document) -> Result<HostCatalogue> {
    if !matches!(d.version, 1 | 2)
        || d.profiles
            .iter()
            .any(|p| (d.version == 1) != p.via.absent())
        || d.legacy_revision > i64::MAX as u64
        || d.hosts.len() > 256
        || d.endpoints.len() > 1024
        || d.profiles.len() > 1024
    {
        return Err(record());
    }
    let clock = Hlc::parse(&d.clock).map_err(|_| record())?;
    if clock.node() != id(&d.node)? {
        return Err(record());
    }
    for m in d
        .hosts
        .iter()
        .map(|h| &h.metadata)
        .chain(d.endpoints.iter().map(|e| &e.metadata))
        .chain(d.profiles.iter().map(|p| &p.metadata))
    {
        if metadata(m)?.clock() > clock {
            return Err(record());
        }
    }
    let hosts = d
        .hosts
        .iter()
        .map(|h| Host::new(metadata(&h.metadata)?, &h.name).map_err(|_| record()))
        .collect::<Result<Vec<_>>>()?;
    let endpoints = d
        .endpoints
        .iter()
        .map(|e| {
            Endpoint::new(
                metadata(&e.metadata)?,
                id(&e.host)?,
                &e.name,
                protocol(&e.protocol)?,
                &e.address,
                e.port,
            )
            .map_err(|_| record())
        })
        .collect::<Result<Vec<_>>>()?;
    let profiles = d
        .profiles
        .iter()
        .map(|p| {
            SshProfile::new(
                metadata(&p.metadata)?,
                id(&p.endpoint)?,
                &p.name,
                &p.username,
                p.identity.as_deref().map(id).transpose()?,
            )
            .map_err(|_| record())?
            .with_jump_profile(p.via.value().map(id).transpose()?)
            .map_err(|_| record())
        })
        .collect::<Result<Vec<_>>>()?;
    HostCatalogue::new(hosts, endpoints, profiles).map_err(|_| record())
}
fn document(connection: &Connection) -> Result<Option<Document>> {
    let row:Option<(i64,Vec<u8>)>=connection.query_row("SELECT length(CAST(value AS BLOB)),substr(CAST(value AS BLOB),1,1048577) FROM app_state WHERE key=?1",[KEY],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|StoreError::Database)?;
    let Some((len, data)) = row else {
        return Ok(None);
    };
    if !(0..=LIMIT as i64).contains(&len) {
        return Err(record());
    }
    let doc: Document = serde_json::from_slice(&data).map_err(|_| record())?;
    graph(&doc)?;
    Ok(Some(doc))
}
fn matches_legacy(d: &Document, legacy: &HostSnapshot) -> bool {
    let active = d
        .endpoints
        .iter()
        .filter(|e| !e.metadata.deleted && e.protocol == "ssh")
        .collect::<Vec<_>>();
    if d.legacy_revision != legacy.revision() || active.len() != legacy.records().len() {
        return false;
    }
    legacy.records().iter().all(|row| {
        let Some(e) = active
            .iter()
            .find(|e| e.metadata.id == row.id().to_string())
        else {
            return false;
        };
        let mut profiles = d
            .profiles
            .iter()
            .filter(|p| !p.metadata.deleted && p.endpoint == e.metadata.id);
        let Some(p) = profiles.next() else {
            return false;
        };
        profiles.next().is_none()
            && e.name == row.endpoint().label()
            && e.address == row.endpoint().target().host()
            && e.port == row.endpoint().target().port()
            && p.username == row.endpoint().username()
    })
}
/// Validate machine associations at the same transaction boundary as the saved entrances.
pub(crate) fn projection(
    connection: &Connection,
    source: &crate::ssh_hosts::HostSnapshot,
) -> Result<Option<Box<HostCatalogue>>> {
    let Some(doc) = document(connection)? else {
        return Ok(None);
    };
    if !matches_legacy(&doc, source) {
        return Err(StoreError::HostConflict);
    }
    Ok(Some(Box::new(graph(&doc)?)))
}
/// Same transaction read; an absent graph means legacy-only metadata, never an implicit write.
pub(crate) fn load(connection: &Connection, owner: Uuid) -> Result<Option<Snapshot>> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Deferred)
            .map_err(|_| StoreError::Database)?;
    let legacy = crate::ssh_hosts::load(&transaction, owner)?;
    let Some(doc) = document(&transaction)? else {
        return Ok(None);
    };
    if !matches_legacy(&doc, &legacy) {
        return Err(StoreError::HostConflict);
    }
    let result = Snapshot {
        catalogue: graph(&doc)?,
        legacy_revision: doc.legacy_revision,
    };
    transaction.commit().map_err(|_| StoreError::Database)?;
    Ok(Some(result))
}
impl StateStore {
    /// Authorized metadata read using existing SQL admission and native credit lifetime.
    /// Old databases return None until an explicit authorized host save migrates them.
    pub async fn load_host_catalogue_authorized(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Option<Snapshot>> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::HostCatalogueLoad(
            self.host_owner,
            authorize,
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| record())?
        .as_millis()
        .try_into()
        .map_err(|_| record())
}
fn stamp(d: &Document, old: Option<&Meta>, chosen: Option<Uuid>, deleted: bool) -> Result<Meta> {
    let clock = Hlc::parse(&d.clock).map_err(|_| record())?;
    Ok(Meta {
        id: old.map_or_else(
            || chosen.unwrap_or_else(Uuid::now_v7).to_string(),
            |m| m.id.clone(),
        ),
        created_at: old.map_or(clock.wall(), |m| m.created_at),
        updated_at: clock.wall(),
        hlc: d.clock.clone(),
        deleted,
    })
}
fn append(
    d: &mut Document,
    row: &crate::ssh_hosts::SavedSshHost,
    parent: Option<Uuid>,
) -> Result<()> {
    if d.endpoints
        .iter()
        .any(|e| e.metadata.id == row.id().to_string())
    {
        return Err(StoreError::HostConflict);
    }
    let host_id = if let Some(parent) = parent {
        let id = parent.to_string();
        if !d
            .hosts
            .iter()
            .any(|h| h.metadata.id == id && !h.metadata.deleted)
        {
            return Err(StoreError::HostConflict);
        }
        id
    } else {
        let host = stamp(d, None, None, false)?;
        let id = host.id.clone();
        d.hosts.push(HostRow {
            metadata: host,
            name: row.endpoint().label().to_owned(),
        });
        id
    };
    d.endpoints.push(EndpointRow {
        metadata: stamp(d, None, Some(row.id()), false)?,
        host: host_id,
        name: row.endpoint().label().to_owned(),
        protocol: "ssh".to_owned(),
        address: row.endpoint().target().host().to_owned(),
        port: row.endpoint().target().port(),
    });
    d.profiles.push(ProfileRow {
        metadata: stamp(d, None, None, false)?,
        endpoint: row.id().to_string(),
        name: row.endpoint().label().to_owned(),
        username: row.endpoint().username().to_owned(),
        identity: None,
        via: JumpReference::Present(None),
    });
    Ok(())
}
/// Called inside the original host save transaction, before its final authorization check.
/// The legacy document is a checked compatibility projection; mismatch fails closed.
pub(crate) fn reconcile(
    transaction: &Transaction<'_>,
    before: &HostSnapshot,
    after: &HostSnapshot,
    parent: Option<Uuid>,
    rename: Option<(Uuid, String)>,
    profile: Option<&pulse_core::host_catalogue::SshProfile>,
) -> Result<()> {
    let mut d = if let Some(doc) = document(transaction)? {
        if !matches_legacy(&doc, before) {
            return Err(StoreError::HostConflict);
        }
        doc
    } else {
        let node = Uuid::now_v7();
        let mut doc = Document {
            version: 2,
            node: node.to_string(),
            clock: Hlc::new(now()?, 0, node).map_err(|_| record())?.encode(),
            legacy_revision: before.revision(),
            hosts: vec![],
            endpoints: vec![],
            profiles: vec![],
        };
        for row in before.records() {
            append(&mut doc, row, None)?;
        }
        doc
    };
    // Reading never migrates. The already-authorized save upgrades all rows together.
    d.version = 2;
    for p in &mut d.profiles {
        if p.via.absent() {
            p.via = JumpReference::Present(None);
        }
    }
    let physical_time = now()?;
    d.clock = Hlc::parse(&d.clock)
        .and_then(|h| h.next(physical_time))
        .map_err(|_| record())?
        .encode();
    for index in 0..d.endpoints.len() {
        let e = &d.endpoints[index];
        if e.metadata.deleted || e.protocol != "ssh" {
            continue;
        }
        if after
            .records()
            .iter()
            .any(|r| r.id().to_string() == e.metadata.id)
        {
            continue;
        }
        let endpoint_id = e.metadata.id.clone();
        d.endpoints[index].metadata = stamp(&d, Some(&d.endpoints[index].metadata), None, true)?;
        for p in 0..d.profiles.len() {
            if !d.profiles[p].metadata.deleted && d.profiles[p].endpoint == endpoint_id {
                d.profiles[p].metadata = stamp(&d, Some(&d.profiles[p].metadata), None, true)?;
            }
        }
    }
    for row in after.records() {
        let Some(index) = d
            .endpoints
            .iter()
            .position(|e| !e.metadata.deleted && e.metadata.id == row.id().to_string())
        else {
            append(&mut d, row, parent)?;
            continue;
        };
        let e = &d.endpoints[index];
        if e.name != row.endpoint().label()
            || e.address != row.endpoint().target().host()
            || e.port != row.endpoint().target().port()
        {
            d.endpoints[index].metadata =
                stamp(&d, Some(&d.endpoints[index].metadata), None, false)?;
            d.endpoints[index].name = row.endpoint().label().to_owned();
            d.endpoints[index].address = row.endpoint().target().host().to_owned();
            d.endpoints[index].port = row.endpoint().target().port();
        }
        let p = d
            .profiles
            .iter()
            .position(|p| !p.metadata.deleted && p.endpoint == row.id().to_string())
            .ok_or_else(record)?;
        if d.profiles[p].username != row.endpoint().username() {
            d.profiles[p].metadata = stamp(&d, Some(&d.profiles[p].metadata), None, false)?;
            d.profiles[p].username = row.endpoint().username().to_owned();
        }
    }
    for h in 0..d.hosts.len() {
        if !d.hosts[h].metadata.deleted
            && !d
                .endpoints
                .iter()
                .any(|e| !e.metadata.deleted && e.host == d.hosts[h].metadata.id)
        {
            d.hosts[h].metadata = stamp(&d, Some(&d.hosts[h].metadata), None, true)?;
        }
    }
    if let Some((id, name)) = rename {
        let index = d
            .hosts
            .iter()
            .position(|h| h.metadata.id == id.to_string() && !h.metadata.deleted)
            .ok_or(StoreError::HostConflict)?;
        d.hosts[index].metadata = stamp(&d, Some(&d.hosts[index].metadata), None, false)?;
        d.hosts[index].name = name;
    }
    if let Some(profile) = profile {
        let index = d
            .profiles
            .iter()
            .position(|p| {
                !p.metadata.deleted
                    && p.metadata.id == profile.metadata().id().to_string()
                    && p.endpoint == profile.endpoint_id().to_string()
            })
            .ok_or(StoreError::HostConflict)?;
        d.profiles[index].metadata = stamp(&d, Some(&d.profiles[index].metadata), None, false)?;
        d.profiles[index].name = profile.label().to_owned();
        d.profiles[index].username = profile.username().to_owned();
        d.profiles[index].identity = profile.identity_id().map(|id| id.to_string());
        d.profiles[index].via =
            JumpReference::Present(profile.jump_profile_id().map(|id| id.to_string()));
    }
    d.legacy_revision = after.revision();
    graph(&d)?;
    if !matches_legacy(&d, after) {
        return Err(StoreError::HostConflict);
    }
    let text = serde_json::to_string(&d).map_err(|_| record())?;
    if text.len() > LIMIT {
        return Err(record());
    }
    transaction.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![KEY,text]).map_err(|_|StoreError::Database)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_hosts::{self, HostEdit};
    use pulse_core::ssh_endpoint::SshEndpoint;
    use std::sync::atomic::{AtomicUsize, Ordering};
    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
    fn directory() -> std::io::Result<tempfile::TempDir> {
        let d = tempfile::tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(d)
    }
    #[test]
    fn legacy_migration_references_and_tombstones_are_atomic_and_reopenable() -> TestResult {
        let d = directory()?;
        let path = d.path().join("state.db");
        let db = crate::database::open(&path)?;
        let owner = Uuid::new_v4();
        let empty = ssh_hosts::load(&db, owner)?;
        ssh_hosts::edit(
            &db,
            owner,
            &empty,
            HostEdit::Create(SshEndpoint::new("legacy", "fixture.invalid", 22, "user")?),
            || true,
        )?;
        db.execute("DELETE FROM app_state WHERE key=?1", [KEY])?;
        assert!(load(&db, owner)?.is_none());
        let first = ssh_hosts::load(&db, owner)?;
        let endpoint_id = first.records()[0].id();
        let migrated = ssh_hosts::edit(&db, owner, &first, HostEdit::MigrateCatalogue, || true)?;
        assert!(migrated.records()[0].endpoint() == first.records()[0].endpoint());
        assert!(migrated.catalogue().is_some());
        let updated = ssh_hosts::edit(
            &db,
            owner,
            &migrated,
            HostEdit::Update(
                endpoint_id,
                SshEndpoint::new("changed", "fixture.invalid", 2222, "other")?,
            ),
            || true,
        )?;
        let graph = load(&db, owner)?.ok_or("missing migration")?;
        assert_eq!(graph.legacy_revision(), updated.revision());
        assert_eq!(
            graph.catalogue().endpoints()[0].metadata().id(),
            endpoint_id
        );
        assert!(graph.catalogue().hosts()[0].metadata().id() != endpoint_id);
        assert!(graph.catalogue().profiles()[0].metadata().id() != endpoint_id);
        assert_eq!(graph.catalogue().profiles()[0].username(), "other");
        let removed =
            ssh_hosts::edit(&db, owner, &updated, HostEdit::Remove(endpoint_id), || true)?;
        let deleted = load(&db, owner)?.ok_or("missing tombstone")?;
        assert!(deleted.catalogue().hosts()[0].metadata().deleted());
        assert!(deleted.catalogue().endpoints()[0].metadata().deleted());
        assert!(deleted.catalogue().profiles()[0].metadata().deleted());
        assert_eq!(removed.records().len(), 0);
        drop(db);
        let reopened = crate::database::open(&path)?;
        assert!(
            load(&reopened, owner)?.ok_or("missing reopen")?.catalogue() == deleted.catalogue()
        );
        Ok(())
    }
    #[tokio::test]
    async fn worker_uses_original_read_credit_and_preserves_legacy_only_reads() -> TestResult {
        let d = directory()?;
        let (store, task) = StateStore::open(d.path().join("state.db")).await?;
        let allowed: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| true);
        assert!(
            store
                .load_host_catalogue_authorized(allowed.clone())
                .await?
                .is_none()
        );
        let before = store.load_ssh_hosts().await?;
        let saved = store
            .edit_ssh_hosts(
                before,
                HostEdit::Create(SshEndpoint::new("worker", "fixture.invalid", 22, "user")?),
                allowed.clone(),
            )
            .await?;
        let graph = store
            .load_host_catalogue_authorized(allowed)
            .await?
            .ok_or("missing worker graph")?;
        assert_eq!(
            graph.catalogue().endpoints()[0].metadata().id(),
            saved.records()[0].id()
        );
        assert!(matches!(
            store
                .load_host_catalogue_authorized(Arc::new(|| false))
                .await,
            Err(StoreError::HostAuthorization)
        ));
        drop(store);
        task.join().await?;
        Ok(())
    }
    #[test]
    fn entrances_share_a_machine_but_keep_distinct_sources_and_names() -> TestResult {
        let dir = directory()?;
        let db = crate::database::open(&dir.path().join("state.db"))?;
        let owner = Uuid::new_v4();
        let empty = ssh_hosts::load(&db, owner)?;
        let first = ssh_hosts::edit(
            &db,
            owner,
            &empty,
            HostEdit::Create(SshEndpoint::new("primary", "first.invalid", 22, "first")?),
            || true,
        )?;
        let machine = first.catalogue().ok_or("missing graph")?.hosts()[0]
            .metadata()
            .id();
        let next = ssh_hosts::edit(
            &db,
            owner,
            &first,
            HostEdit::AttachSshEndpoint(
                machine,
                SshEndpoint::new("secondary", "second.invalid", 2222, "second")?,
            ),
            || true,
        )?;
        assert_eq!(next.catalogue().ok_or("missing graph")?.hosts().len(), 1);
        assert_eq!(next.records().len(), 2);
        assert_ne!(next.records()[0].id(), next.records()[1].id());
        assert!(
            next.catalogue()
                .ok_or("missing graph")?
                .endpoints()
                .iter()
                .all(|e| e.host_id() == machine)
        );
        let renamed = ssh_hosts::edit(
            &db,
            owner,
            &next,
            HostEdit::RenameMachine(machine, "workstation".to_owned()),
            || true,
        )?;
        assert!(renamed.records()[0].endpoint() == next.records()[0].endpoint());
        assert!(renamed.records()[1].endpoint() == next.records()[1].endpoint());
        assert_eq!(
            renamed.catalogue().ok_or("missing graph")?.hosts()[0].label(),
            "workstation"
        );
        assert!(matches!(
            ssh_hosts::edit(
                &db,
                owner,
                &next,
                HostEdit::Remove(next.records()[0].id()),
                || true
            ),
            Err(StoreError::HostConflict)
        ));
        assert!(matches!(
            ssh_hosts::edit(
                &db,
                owner,
                &renamed,
                HostEdit::AttachSshEndpoint(
                    Uuid::now_v7(),
                    SshEndpoint::new("bad", "invalid.invalid", 22, "user")?
                ),
                || true
            ),
            Err(StoreError::HostConflict)
        ));
        db.execute(
            "UPDATE app_state SET value=json_set(value,'$.hosts[0].name','external') WHERE key=?1",
            [KEY],
        )?;
        assert!(matches!(
            ssh_hosts::edit(
                &db,
                owner,
                &renamed,
                HostEdit::Remove(renamed.records()[0].id()),
                || true
            ),
            Err(StoreError::HostConflict)
        ));
        let current = ssh_hosts::load(&db, owner)?;
        let one = ssh_hosts::edit(
            &db,
            owner,
            &current,
            HostEdit::Remove(current.records()[0].id()),
            || true,
        )?;
        assert!(
            !one.catalogue().ok_or("missing graph")?.hosts()[0]
                .metadata()
                .deleted()
        );
        assert_eq!(one.records()[0].id(), next.records()[1].id());
        let removed = ssh_hosts::edit(
            &db,
            owner,
            &one,
            HostEdit::Remove(one.records()[0].id()),
            || true,
        )?;
        assert!(
            removed.catalogue().ok_or("missing graph")?.hosts()[0]
                .metadata()
                .deleted()
        );
        assert!(matches!(
            ssh_hosts::edit(
                &db,
                owner,
                &removed,
                HostEdit::AttachSshEndpoint(
                    machine,
                    SshEndpoint::new("bad", "invalid.invalid", 22, "user")?
                ),
                || true
            ),
            Err(StoreError::HostConflict)
        ));
        Ok(())
    }
    #[test]
    fn final_revocation_corruption_and_same_revision_source_change_never_partially_commit()
    -> TestResult {
        let d = directory()?;
        let db = crate::database::open(&d.path().join("state.db"))?;
        let owner = Uuid::new_v4();
        let empty = ssh_hosts::load(&db, owner)?;
        let calls = AtomicUsize::new(0);
        assert!(matches!(
            ssh_hosts::edit(
                &db,
                owner,
                &empty,
                HostEdit::Create(SshEndpoint::new(
                    "cancelled",
                    "fixture.invalid",
                    22,
                    "user"
                )?),
                || calls.fetch_add(1, Ordering::SeqCst) < 2
            ),
            Err(StoreError::HostAuthorization)
        ));
        assert!(load(&db, owner)?.is_none());
        assert_eq!(ssh_hosts::load(&db, owner)?.revision(), 0);
        let first = ssh_hosts::edit(
            &db,
            owner,
            &empty,
            HostEdit::Create(SshEndpoint::new("saved", "fixture.invalid", 22, "user")?),
            || true,
        )?;
        ssh_hosts::edit(
            &db,
            owner,
            &first,
            HostEdit::Update(
                first.records()[0].id(),
                SshEndpoint::new("altered", "fixture.invalid", 22, "user")?,
            ),
            || true,
        )?;
        db.execute(
            "UPDATE app_state SET value=json_set(value,'$.revision',?1) WHERE key=?2",
            rusqlite::params![i64::try_from(first.revision())?, ssh_hosts::KEY],
        )?;
        assert!(matches!(
            ssh_hosts::edit(
                &db,
                owner,
                &first,
                HostEdit::Remove(first.records()[0].id()),
                || true
            ),
            Err(StoreError::HostConflict)
        ));
        db.execute(
            "UPDATE app_state SET value=json_set(value,'$.revision',?1) WHERE key=?2",
            rusqlite::params![i64::try_from(first.revision() + 1)?, ssh_hosts::KEY],
        )?;
        let current = ssh_hosts::load(&db, owner)?;
        db.execute(
            "UPDATE app_state SET value=?1 WHERE key=?2",
            rusqlite::params!["{bad", KEY],
        )?;
        assert!(matches!(
            ssh_hosts::edit(
                &db,
                owner,
                &current,
                HostEdit::Remove(current.records()[0].id()),
                || true
            ),
            Err(StoreError::HostRecord)
        ));
        assert!(matches!(load(&db, owner), Err(StoreError::HostRecord)));
        Ok(())
    }
}
