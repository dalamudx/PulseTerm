//! Local default automatic-profile selection, separate from the visibility document.
use crate::{
    Request, Result, StateStore, StoreError, database,
    profiles::{AutoProfilePreferences, AutomaticShell},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;

const KEY: &str = "profiles.default.unix.v1";

/// A saved default selects an automatic Shell kind or a backend-generated custom Profile UUID.
/// The string representation preserves existing automatic-profile configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DefaultProfileSelection {
    /// Existing backend-discovered automatic Profile.
    Automatic(AutomaticShell),
    /// Saved custom identity; it is never an executable or execution permission.
    Custom(Uuid),
}
impl DefaultProfileSelection {
    /// Parse only a discovered Shell name or canonical backend-generated v7 UUID, never a path.
    pub fn parse(value: &str) -> Option<Self> {
        let shell = match value {
            "bash" => AutomaticShell::Bash,
            "zsh" => AutomaticShell::Zsh,
            "fish" => AutomaticShell::Fish,
            "sh" => AutomaticShell::Sh,
            "pwsh" => AutomaticShell::Pwsh,
            "powershell" => AutomaticShell::PowerShell,
            "cmd" => AutomaticShell::Cmd,
            "wsl" => AutomaticShell::Wsl,
            _ => {
                let id = Uuid::parse_str(value).ok()?;
                return (id.get_version_num() == 7
                    && id.get_variant() == uuid::Variant::RFC4122
                    && id.to_string() == value)
                    .then_some(Self::Custom(id));
            }
        };
        Some(Self::Automatic(shell))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u8,
    revision: u64,
    shell: Option<DefaultProfileSelection>,
}

/// One store-instance-bound read of the default and current visibility/deletion preferences.
/// This snapshot is metadata, never discovery or permission to start a process.
#[derive(Clone, PartialEq, Eq)]
pub struct DefaultProfileSnapshot {
    owner: Uuid,
    record: Option<Record>,
    preferences: AutoProfilePreferences,
    custom: crate::custom_profiles::CustomProfileSnapshot,
}

impl std::fmt::Debug for DefaultProfileSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DefaultProfileSnapshot(..)")
    }
}
impl DefaultProfileSnapshot {
    /// Saved choice. Hidden, deleted and currently missing identities are not replaced.
    pub fn shell(&self) -> Option<AutomaticShell> {
        match self.selection() {
            Some(DefaultProfileSelection::Automatic(shell)) => Some(shell),
            _ => None,
        }
    }
    /// Saved selection without falling back from a missing custom Profile to an automatic Shell.
    pub fn selection(&self) -> Option<DefaultProfileSelection> {
        self.record.as_ref().and_then(|record| record.shell)
    }
    /// Jointly captured custom catalogue for stale-source rejection and explicit default selection.
    pub fn custom_profiles(&self) -> &[crate::custom_profiles::CustomProfile] {
        self.custom.profiles()
    }
    /// Jointly captured preferences; discovery must still establish availability.
    pub fn preferences(&self) -> &AutoProfilePreferences {
        &self.preferences
    }
}

/// Explicit local automatic-profile deletion or tombstone restoration.
/// Neither operation grants discovery, process launch, or session-close authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileLifecycleEdit {
    /// Suppress rediscovery; an already running session remains independently owned.
    Delete(AutomaticShell),
    /// Remove an existing tombstone; discovery still determines availability.
    Restore(AutomaticShell),
}

type Gate = Arc<dyn Fn() -> bool + Send + Sync>;
pub(crate) enum DefaultRequest {
    Load(
        Uuid,
        Box<dyn Send>,
        oneshot::Sender<Result<DefaultProfileSnapshot>>,
    ),
    Save(
        DefaultProfileSnapshot,
        Option<DefaultProfileSelection>,
        Gate,
        oneshot::Sender<Result<DefaultProfileSnapshot>>,
    ),
    Lifecycle(
        DefaultProfileSnapshot,
        ProfileLifecycleEdit,
        Gate,
        oneshot::Sender<Result<DefaultProfileSnapshot>>,
    ),
}
impl StateStore {
    /// Capture local default and visibility in the shared SQLite worker.
    pub async fn load_default_profile(&self) -> Result<DefaultProfileSnapshot> {
        self.load_default_profile_retaining(()).await
    }
    /// Retain a native-operation reservation until the queued/active SQL request actually exits.
    /// Cancelling the awaiting future cannot release it early. This is not authorization.
    pub async fn load_default_profile_retaining<R: Send + 'static>(
        &self,
        resource: R,
    ) -> Result<DefaultProfileSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::ProfileDefault(Box::new(DefaultRequest::Load(
            self.host_owner,
            Box::new(resource),
            reply,
        ))))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Compatibility entry point for an automatic default; use the same single selection record.
    pub async fn save_default_profile(
        &self,
        expected: DefaultProfileSnapshot,
        shell: Option<AutomaticShell>,
        authorize: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<DefaultProfileSnapshot> {
        self.save_default_profile_selection(
            expected,
            shell.map(DefaultProfileSelection::Automatic),
            authorize,
        )
        .await
    }
    /// CAS one explicit choice or clear it while retaining its monotonic revision.
    /// A hidden/deleted target is rejected. Callers must confirm trusted discovery separately.
    /// The worker retains the authorization closure through native exit, rechecking before commit.
    pub async fn save_default_profile_selection(
        &self,
        expected: DefaultProfileSnapshot,
        shell: Option<DefaultProfileSelection>,
        authorize: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<DefaultProfileSnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::Conflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::ProfileDefault(Box::new(DefaultRequest::Save(
            expected,
            shell,
            Arc::new(authorize),
            reply,
        ))))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Compare the joint source and change its deletion state in one transaction.
    /// Keep the saved default identity; advance its revision even on delete/restore round trips.
    /// The authorization closure and its native reservations remain owned until worker exit.
    pub async fn change_profile_lifecycle(
        &self,
        expected: DefaultProfileSnapshot,
        edit: ProfileLifecycleEdit,
        authorize: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<DefaultProfileSnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::Conflict);
        }
        let (reply, result) = oneshot::channel();
        self.admit(Request::ProfileDefault(Box::new(
            DefaultRequest::Lifecycle(expected, edit, Arc::new(authorize), reply),
        )))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
pub(crate) fn read(connection: &Connection, owner: Uuid) -> Result<DefaultProfileSnapshot> {
    let value: Option<(i64, String)> = connection
        .query_row(
            "SELECT length(CAST(value AS BLOB)), substr(value,1,257) FROM app_state WHERE key=?1",
            [KEY],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| StoreError::Database)?;
    let record = match value {
        None => None,
        Some((size, text)) if (1..=256).contains(&size) => {
            let record: Record = serde_json::from_str(&text).map_err(|_| StoreError::Record)?;
            if record.schema_version != 1
                || record.revision == 0
                || record.revision > i64::MAX as u64
                || record.shell.is_some_and(|selection| matches!(selection, DefaultProfileSelection::Custom(id) if id.get_version_num() != 7))
            {
                return Err(StoreError::Record);
            }
            Some(record)
        }
        _ => return Err(StoreError::Record),
    };
    Ok(DefaultProfileSnapshot {
        owner,
        record,
        preferences: database::load(connection)?,
        custom: crate::custom_profiles::read(connection, owner)?,
    })
}
fn capture(connection: &Connection, owner: Uuid) -> Result<DefaultProfileSnapshot> {
    let tx = rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    let snapshot = read(&tx, owner)?;
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(snapshot)
}
fn save(
    connection: &Connection,
    expected: DefaultProfileSnapshot,
    shell: Option<DefaultProfileSelection>,
    allowed: impl Fn() -> bool,
) -> Result<DefaultProfileSnapshot> {
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    let tx = rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    let mut current = read(&tx, expected.owner)?;
    if current != expected {
        return Err(StoreError::Conflict);
    }
    if shell.is_some_and(|selection| match selection {
        DefaultProfileSelection::Automatic(shell) => current
            .preferences
            .hidden()
            .chain(current.preferences.deleted())
            .any(|item| item == shell),
        DefaultProfileSelection::Custom(id) => {
            id.get_version_num() != 7
                || current.custom.active(id).is_err()
                || current
                    .custom
                    .active(id)
                    .is_ok_and(|profile| profile.draft().hidden)
        }
    }) {
        return Err(StoreError::Record);
    }
    let record = write_record(&tx, &current.record, shell)?;
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    current.record = Some(record);
    Ok(current)
}
fn write_record(
    connection: &Connection,
    previous: &Option<Record>,
    shell: Option<DefaultProfileSelection>,
) -> Result<Record> {
    let revision = previous
        .as_ref()
        .map_or(0, |record| record.revision)
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or(StoreError::Record)?;
    let record = Record {
        schema_version: 1,
        revision,
        shell,
    };
    let text = serde_json::to_string(&record).map_err(|_| StoreError::Record)?;
    connection.execute("INSERT INTO app_state(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![KEY, text])
        .map_err(|_| StoreError::Database)?;
    Ok(record)
}

// Called only inside the preferences write transaction, including legacy edit/CAS paths.
pub(crate) fn advance_lifecycle_revision(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    let previous = read(transaction, Uuid::nil())?;
    write_record(transaction, &previous.record, previous.selection())?;
    Ok(())
}

fn change_lifecycle(
    connection: &Connection,
    expected: DefaultProfileSnapshot,
    edit: ProfileLifecycleEdit,
    allowed: impl Fn() -> bool,
) -> Result<DefaultProfileSnapshot> {
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    let tx = rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    let mut current = read(&tx, expected.owner)?;
    if current != expected {
        return Err(StoreError::Conflict);
    }
    let mutation = match edit {
        ProfileLifecycleEdit::Delete(shell)
            if !current.preferences.deleted().any(|item| item == shell) =>
        {
            crate::profiles::AutoProfileEdit::Suppress(shell)
        }
        ProfileLifecycleEdit::Restore(shell)
            if current.preferences.deleted().any(|item| item == shell) =>
        {
            crate::profiles::AutoProfileEdit::RestoreDeleted(shell)
        }
        _ => return Err(StoreError::Record),
    };
    mutation.apply(&mut current.preferences)?;
    database::write(&tx, &current.preferences)?;
    let result = read(&tx, expected.owner)?;
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(result)
}

pub(crate) fn serve(connection: &Connection, request: DefaultRequest) {
    match request {
        DefaultRequest::Load(owner, _resource, reply) if !reply.is_closed() => {
            let _ = reply.send(capture(connection, owner));
        }
        DefaultRequest::Save(expected, shell, authorize, reply) if !reply.is_closed() => {
            let result = save(connection, expected, shell, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
                    && !reply.is_closed()
            });
            let _ = reply.send(result);
        }
        DefaultRequest::Lifecycle(expected, edit, authorize, reply) if !reply.is_closed() => {
            let result = change_lifecycle(connection, expected, edit, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
                    && !reply.is_closed()
            });
            let _ = reply.send(result);
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "profile_default_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "profile_lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "profile_default_custom_tests.rs"]
mod custom_tests;
