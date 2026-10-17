//! Bounded organization metadata, separate from endpoint, credential and execution authority.
use crate::{Request, Result, StateStore, StoreError};
use pulse_core::hlc::Hlc;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;
const KEY: &str = "catalogue.organization.v1";
const LIMIT: usize = 1_048_576;
/// Which saved metadata directory owns a reference. Never an authentication mode.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Saved SSH endpoint metadata.
    Host,
    /// Password-identity metadata, without ciphertext or unlock.
    PasswordIdentity,
    /// Private-key identity metadata, without key bytes or activation.
    KeyIdentity,
}
/// Independent sync-ready identity, backend times, hybrid timestamp and tombstone.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Metadata {
    /// Backend-generated UUIDv7.
    pub id: String,
    /// Creation time in backend milliseconds.
    pub created_at: u64,
    /// Last edit time; never precedes creation.
    pub updated_at: u64,
    /// Backend hybrid timestamp, not supplied as write authority.
    pub hlc: String,
    /// Deleted identities cannot be reused.
    pub deleted: bool,
}
/// Named group or tag. Tags never have a parent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Named {
    /// Identity and synchronization metadata.
    pub metadata: Metadata,
    /// Literal bounded display name.
    pub name: String,
    /// Parent group identity, or root.
    pub parent: Option<String>,
}
/// Organization overlay for an existing saved item. Contains no credential bytes.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Membership {
    /// Independent synchronization identity.
    pub metadata: Metadata,
    /// Directory owning the target.
    pub kind: Kind,
    /// Saved backend item identity.
    pub item: String,
    /// Optional group reference.
    pub group: Option<String>,
    /// Tag identities, unique and bounded.
    pub tags: Vec<String>,
    /// Explicit user favorite.
    pub favorite: bool,
}
/// Literal saved-item projection; does not grant dialing or secret-read permission.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Directory owning this item.
    pub kind: Kind,
    /// Canonical saved UUIDv7.
    pub id: String,
    /// Non-secret display name.
    pub label: String,
    /// Non-secret username/address description.
    pub description: String,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u8,
    revision: u64,
    node: String,
    clock: String,
    groups: Vec<Named>,
    tags: Vec<Named>,
    memberships: Vec<Membership>,
}
/// Store-instance-bound immutable organization and three-directory capture.
#[derive(Clone)]
pub struct Snapshot {
    owner: Uuid,
    document: Document,
    items: Vec<Item>,
    source_revisions: [u64; 3],
    host_source: Box<crate::ssh_hosts::HostSnapshot>,
}
impl Snapshot {
    /// Current saved items from the same SQL capture.
    pub fn items(&self) -> &[Item] {
        &self.items
    }
    /// Full organization document for a bounded literal drawing projection.
    pub fn projection(&self) -> serde_json::Value {
        let catalogue = self.host_source.catalogue();
        let machines: Vec<serde_json::Value> = catalogue.map(|c| c.hosts().iter().filter(|h| !h.metadata().deleted()).map(|h| serde_json::json!({"id":h.metadata().id().to_string(),"label":h.label()})).collect()).unwrap_or_default();
        let entrances: Vec<serde_json::Value> = catalogue.map(|c| c.endpoints().iter().filter(|e| !e.metadata().deleted()).map(|e| serde_json::json!({"id":e.metadata().id().to_string(),"machineId":e.host_id().to_string()})).collect()).unwrap_or_default();
        serde_json::json!({"groups": self.document.groups, "tags": self.document.tags, "memberships": self.document.memberships, "items": self.items, "hostGraph":{"version":1,"migrated":catalogue.is_some(),"machines":machines,"entrances":entrances}})
    }
}
/// Explicit metadata-only edit. No revision, clock, secret or path input.
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    /// Add a backend-identified group below an existing parent.
    CreateGroup {
        /// Literal group name, at most 128 UTF-8 bytes.
        name: String,
        /// Existing active parent, or root.
        parent: Option<String>,
    },
    /// Rename or move a group; cycles are rejected.
    UpdateGroup {
        /// Backend-generated active group identity.
        id: String,
        /// Literal group name, at most 128 UTF-8 bytes.
        name: String,
        /// Existing active parent, or root.
        parent: Option<String>,
    },
    /// Delete an empty group, retaining its tombstone. Referenced groups must first be emptied.
    RemoveGroup {
        /// Backend-generated active group identity.
        id: String,
    },
    /// Replace one existing item's group, tag names and favorite in one transaction.
    Assign {
        /// Saved-item directory.
        kind: Kind,
        /// Existing canonical saved item identity.
        item: String,
        /// Existing active group, or no group.
        group: Option<String>,
        /// Up to sixteen distinct literal tag names.
        tags: Vec<String>,
        /// Explicit user preference.
        favorite: bool,
    },
}
pub(crate) struct OrganizationRequest {
    owner: Uuid,
    expected: Option<Snapshot>,
    edit: Option<Edit>,
    authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    reply: oneshot::Sender<Result<Snapshot>>,
}
impl StateStore {
    /// Read consistent metadata; retain actual-operation credit in the predicate.
    pub async fn load_catalogue_organization(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Snapshot> {
        self.organization(None, None, authorize).await
    }
    /// Single-source CAS. Revocation is rechecked at SQL commit; no automatic retry.
    pub async fn edit_catalogue_organization(
        &self,
        expected: Snapshot,
        edit: Edit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Snapshot> {
        self.organization(Some(expected), Some(edit), authorize)
            .await
    }
    async fn organization(
        &self,
        expected: Option<Snapshot>,
        edit: Option<Edit>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Snapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::Organization(Box::new(OrganizationRequest {
            owner: self.host_owner,
            expected,
            edit,
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
fn id(s: &str) -> Result<()> {
    let parsed = Uuid::parse_str(s).map_err(|_| StoreError::OrganizationRecord)?;
    if parsed.get_version_num() != 7
        || parsed.get_variant() != uuid::Variant::RFC4122
        || parsed.to_string() != s
    {
        return Err(StoreError::OrganizationRecord);
    }
    Ok(())
}
fn name(s: &str) -> Result<()> {
    if s.is_empty() || s.len()>128 || s.trim()!=s || s.chars().any(|c| c.is_control() || matches!(c,'\u{ad}'|'\u{61c}'|'\u{200b}'..='\u{200f}'|'\u{2028}'..='\u{202e}'|'\u{2060}'..='\u{206f}'|'\u{feff}')) { return Err(StoreError::OrganizationRecord); }
    Ok(())
}
fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::OrganizationRecord)?
        .as_millis()
        .try_into()
        .map_err(|_| StoreError::OrganizationRecord)
}
fn blank() -> Result<Document> {
    let node = Uuid::new_v4();
    let clock = Hlc::new(now()?, 0, node)
        .map_err(|_| StoreError::OrganizationRecord)?
        .encode();
    Ok(Document {
        version: 1,
        revision: 0,
        node: node.to_string(),
        clock,
        groups: vec![],
        tags: vec![],
        memberships: vec![],
    })
}
fn validate(d: &Document) -> Result<()> {
    if d.version != 1
        || d.revision > i64::MAX as u64
        || d.groups.len() > 256
        || d.tags.len() > 256
        || d.memberships.len() > 1024
    {
        return Err(StoreError::OrganizationRecord);
    }
    let node = Uuid::parse_str(&d.node).map_err(|_| StoreError::OrganizationRecord)?;
    let clock = Hlc::parse(&d.clock).map_err(|_| StoreError::OrganizationRecord)?;
    if node.is_nil()
        || node.to_string() != d.node
        || clock.encode() != d.clock
        || clock.node() != node
    {
        return Err(StoreError::OrganizationRecord);
    }
    let mut ids = std::collections::BTreeSet::new();
    for m in d
        .groups
        .iter()
        .chain(&d.tags)
        .map(|r| &r.metadata)
        .chain(d.memberships.iter().map(|r| &r.metadata))
    {
        id(&m.id)?;
        let stamp = Hlc::parse(&m.hlc).map_err(|_| StoreError::OrganizationRecord)?;
        if !ids.insert(&m.id)
            || m.created_at == 0
            || m.updated_at < m.created_at
            || m.updated_at > i64::MAX as u64
            || m.updated_at != stamp.wall()
            || stamp > clock
            || stamp.node() != node
        {
            return Err(StoreError::OrganizationRecord);
        }
    }
    for r in &d.groups {
        name(&r.name)?;
        if !r.metadata.deleted {
            group_parent(d, &r.metadata.id, r.parent.as_deref())?;
        }
    }
    for (i, r) in d.groups.iter().enumerate() {
        if !r.metadata.deleted
            && d.groups[..i]
                .iter()
                .any(|x| !x.metadata.deleted && x.name == r.name && x.parent == r.parent)
        {
            return Err(StoreError::OrganizationRecord);
        }
    }
    for (i, r) in d.tags.iter().enumerate() {
        name(&r.name)?;
        if r.parent.is_some()
            || (!r.metadata.deleted
                && d.tags[..i]
                    .iter()
                    .any(|x| !x.metadata.deleted && x.name == r.name))
        {
            return Err(StoreError::OrganizationRecord);
        }
    }
    for (i, r) in d.memberships.iter().enumerate() {
        id(&r.item)?;
        if r.tags.len() > 16
            || d.memberships[..i]
                .iter()
                .any(|x| x.kind == r.kind && x.item == r.item)
        {
            return Err(StoreError::OrganizationRecord);
        }
        if let Some(group) = &r.group {
            active(&d.groups, group)?;
        }
        let mut unique = std::collections::BTreeSet::new();
        for tag in &r.tags {
            active(&d.tags, tag)?;
            if !unique.insert(tag) {
                return Err(StoreError::OrganizationRecord);
            }
        }
    }
    Ok(())
}
fn active<'a>(rows: &'a [Named], id: &str) -> Result<&'a Named> {
    rows.iter()
        .find(|r| r.metadata.id == id && !r.metadata.deleted)
        .ok_or(StoreError::OrganizationRecord)
}
fn group_parent(d: &Document, own: &str, parent: Option<&str>) -> Result<()> {
    let mut next = parent;
    let mut depth = 0;
    while let Some(parent) = next {
        if parent == own || depth >= 7 {
            return Err(StoreError::OrganizationRecord);
        }
        next = active(&d.groups, parent)?.parent.as_deref();
        depth += 1;
    }
    Ok(())
}
struct Sources {
    items: Vec<Item>,
    revisions: [u64; 3],
    host_source: Box<crate::ssh_hosts::HostSnapshot>,
}
fn sources(connection: &Connection, owner: Uuid) -> Result<Sources> {
    let hosts = crate::ssh_hosts::load(connection, owner)?;
    let identities = crate::ssh_identities::load(connection, owner)?;
    let keys = crate::ssh_key_identities::load(connection, owner)?;
    let mut rows = vec![];
    for r in hosts.records() {
        rows.push(Item {
            kind: Kind::Host,
            id: r.id().to_string(),
            label: r.endpoint().label().to_owned(),
            description: format!(
                "{}@{}:{}",
                r.endpoint().username(),
                r.endpoint().target().host(),
                r.endpoint().target().port()
            ),
        });
    }
    for r in identities.records() {
        rows.push(Item {
            kind: Kind::PasswordIdentity,
            id: r.id().to_string(),
            label: r.identity().label().to_owned(),
            description: r.identity().username().to_owned(),
        });
    }
    for r in keys.records() {
        rows.push(Item {
            kind: Kind::KeyIdentity,
            id: r.id().to_string(),
            label: r.identity().label().to_owned(),
            description: r.identity().username().to_owned(),
        });
    }
    Ok(Sources {
        items: rows,
        revisions: [hosts.revision(), identities.revision(), keys.revision()],
        host_source: Box::new(hosts),
    })
}
pub(crate) fn load(connection: &Connection, owner: Uuid) -> Result<Snapshot> {
    let raw: Option<Vec<u8>> = connection
        .query_row(
            "SELECT substr(CAST(value AS BLOB),1,1048577) FROM app_state WHERE key=?1",
            [KEY],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| StoreError::Database)?;
    let document = match raw {
        None => blank()?,
        Some(raw) if raw.len() <= LIMIT => {
            serde_json::from_slice(&raw).map_err(|_| StoreError::OrganizationRecord)?
        }
        _ => return Err(StoreError::OrganizationRecord),
    };
    validate(&document)?;
    let sources = sources(connection, owner)?;
    Ok(Snapshot {
        owner,
        document,
        items: sources.items,
        source_revisions: sources.revisions,
        host_source: sources.host_source,
    })
}
fn stamp(d: &Document, old: Option<&Metadata>) -> Result<Metadata> {
    let clock = Hlc::parse(&d.clock).map_err(|_| StoreError::OrganizationRecord)?;
    Ok(Metadata {
        id: old.map_or_else(|| Uuid::now_v7().to_string(), |m| m.id.clone()),
        created_at: old.map_or(clock.wall(), |m| m.created_at),
        updated_at: clock.wall(),
        hlc: d.clock.clone(),
        deleted: false,
    })
}
fn apply(d: &mut Document, available: &[Item], edit: Edit) -> Result<()> {
    let physical = now()?;
    d.clock = Hlc::parse(&d.clock)
        .and_then(|x| x.next(physical))
        .map_err(|_| StoreError::OrganizationRecord)?
        .encode();
    match edit {
        Edit::CreateGroup {
            name: label,
            parent,
        } => {
            name(&label)?;
            let metadata = stamp(d, None)?;
            group_parent(d, &metadata.id, parent.as_deref())?;
            d.groups.push(Named {
                metadata,
                name: label,
                parent,
            });
        }
        Edit::UpdateGroup {
            id,
            name: label,
            parent,
        } => {
            name(&label)?;
            group_parent(d, &id, parent.as_deref())?;
            let old = active(&d.groups, &id)?;
            let metadata = stamp(d, Some(&old.metadata))?;
            let index = d
                .groups
                .iter()
                .position(|r| r.metadata.id == id)
                .ok_or(StoreError::OrganizationConflict)?;
            d.groups[index] = Named {
                metadata,
                name: label,
                parent,
            };
        }
        Edit::RemoveGroup { id } => {
            let old = active(&d.groups, &id)?;
            let mut metadata = stamp(d, Some(&old.metadata))?;
            if d.groups
                .iter()
                .any(|r| !r.metadata.deleted && r.parent.as_ref() == Some(&id))
                || d.memberships.iter().any(|r| {
                    !r.metadata.deleted
                        && r.group.as_ref() == Some(&id)
                        && available.iter().any(|i| i.kind == r.kind && i.id == r.item)
                })
            {
                return Err(StoreError::OrganizationConflict);
            }
            metadata.deleted = true;
            d.groups
                .iter_mut()
                .find(|r| r.metadata.id == id)
                .ok_or(StoreError::OrganizationConflict)?
                .metadata = metadata;
            for membership in &mut d.memberships {
                if membership.group.as_ref() == Some(&id) {
                    membership.group = None;
                    membership.metadata.updated_at = Hlc::parse(&d.clock)
                        .map_err(|_| StoreError::OrganizationRecord)?
                        .wall();
                    membership.metadata.hlc = d.clock.clone();
                }
            }
        }
        Edit::Assign {
            kind,
            item,
            group,
            tags,
            favorite,
        } => {
            if !available.iter().any(|r| r.kind == kind && r.id == item) {
                return Err(StoreError::OrganizationConflict);
            }
            if let Some(id) = &group {
                active(&d.groups, id)?;
            }
            if tags.len() > 16 {
                return Err(StoreError::OrganizationRecord);
            }
            let mut tag_ids = vec![];
            for label in tags {
                name(&label)?;
                let found = d
                    .tags
                    .iter()
                    .find(|r| !r.metadata.deleted && r.name == label)
                    .map(|r| r.metadata.id.clone());
                let id = if let Some(id) = found {
                    id
                } else {
                    let metadata = stamp(d, None)?;
                    let id = metadata.id.clone();
                    d.tags.push(Named {
                        metadata,
                        name: label,
                        parent: None,
                    });
                    id
                };
                if tag_ids.contains(&id) {
                    return Err(StoreError::OrganizationRecord);
                }
                tag_ids.push(id);
            }
            let index = d
                .memberships
                .iter()
                .position(|r| r.kind == kind && r.item == item);
            let metadata = stamp(d, index.map(|i| &d.memberships[i].metadata))?;
            let row = Membership {
                metadata,
                kind,
                item,
                group,
                tags: tag_ids,
                favorite,
            };
            if let Some(i) = index {
                d.memberships[i] = row;
            } else {
                d.memberships.push(row);
            }
        }
    }
    d.revision = d
        .revision
        .checked_add(1)
        .ok_or(StoreError::OrganizationRecord)?;
    validate(d)
}
pub(crate) fn edit(
    connection: &Connection,
    owner: Uuid,
    expected: &Snapshot,
    intent: Edit,
    authorize: impl Fn() -> bool,
) -> Result<Snapshot> {
    if expected.owner != owner {
        return Err(StoreError::OrganizationConflict);
    }
    if !authorize() {
        return Err(StoreError::OrganizationAuthorization);
    }
    let tx = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let mut current = load(&tx, owner)?;
    // An absent document has no durable random clock; compare absence/revision explicitly.
    if !current.host_source.source_matches(&expected.host_source)
        || current.source_revisions != expected.source_revisions
        || current.items != expected.items
        || (current.document.revision != 0 && current.document != expected.document)
        || current.document.revision != expected.document.revision
    {
        return Err(StoreError::OrganizationConflict);
    }
    if current.document.revision == 0 {
        current.document = expected.document.clone();
    }
    apply(&mut current.document, &current.items, intent)?;
    let text =
        serde_json::to_string(&current.document).map_err(|_| StoreError::OrganizationRecord)?;
    if text.len() > LIMIT {
        return Err(StoreError::OrganizationRecord);
    }
    if !authorize() {
        return Err(StoreError::OrganizationAuthorization);
    }
    tx.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![KEY,text]).map_err(|_|StoreError::Database)?;
    if !authorize() {
        return Err(StoreError::OrganizationAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(current)
}
pub(crate) fn serve(connection: &Connection, request: OrganizationRequest) {
    let allowed = || {
        !request.reply.is_closed()
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (request.authorize)()))
                .unwrap_or(false)
    };
    let value = if !allowed() {
        Err(StoreError::OrganizationAuthorization)
    } else {
        match (request.expected, request.edit) {
            (Some(expected), Some(intent)) => {
                edit(connection, request.owner, &expected, intent, allowed)
            }
            (None, None) => {
                let tx = Transaction::new_unchecked(connection, TransactionBehavior::Deferred)
                    .map_err(|_| StoreError::Database);
                tx.and_then(|tx| {
                    let result = load(&tx, request.owner)?;
                    if !allowed() {
                        return Err(StoreError::OrganizationAuthorization);
                    }
                    tx.commit().map_err(|_| StoreError::Database)?;
                    Ok(result)
                })
            }
            _ => Err(StoreError::OrganizationRecord),
        }
    };
    let _ = request.reply.send(value);
}

impl Snapshot {
    pub(crate) fn behavior_group(
        &self,
        scope: crate::settings_scope::SettingsScope,
    ) -> Result<Option<Uuid>> {
        use crate::settings_scope::SettingsScope;
        match scope {
            SettingsScope::Group(id)
                if self
                    .document
                    .groups
                    .iter()
                    .any(|g| g.metadata.id == id.to_string() && !g.metadata.deleted) =>
            {
                Ok(None)
            }
            SettingsScope::Host(id)
                if self
                    .items
                    .iter()
                    .any(|item| item.kind == Kind::Host && item.id == id.to_string()) =>
            {
                self.document
                    .memberships
                    .iter()
                    .find(|link| {
                        link.kind == Kind::Host
                            && link.item == id.to_string()
                            && !link.metadata.deleted
                    })
                    .and_then(|link| link.group.as_deref())
                    .map(Uuid::parse_str)
                    .transpose()
                    .map_err(|_| StoreError::OrganizationRecord)
            }
            _ => Err(StoreError::OrganizationRecord),
        }
    }
    pub(crate) fn behavior_source_matches(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.document.revision == other.document.revision
            && (self.document.revision == 0 || self.document == other.document)
            && self.items == other.items
            && self.source_revisions == other.source_revisions
            && self.host_source.source_matches(&other.host_source)
    }
}
#[cfg(test)]
#[path = "catalogue_organization_tests.rs"]
mod tests;
