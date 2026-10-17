//! Bounded automatic Profile configuration; saved metadata never grants execution authority.
use crate::{
    Request, Result, StateStore, StoreError,
    profile_default::DefaultProfileSnapshot,
    profiles::AutomaticShell,
    settings::{SettingsPatch, TerminalPatch},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};
use tokio::sync::oneshot;
use uuid::Uuid;
const LIMIT: usize = 32 * 1024;
/// Sparse drawing overrides captured when a session starts, independent of later Profile edits.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileAppearance {
    /// Omission inherits workspace font size.
    pub font_size: Option<u16>,
    /// Omission inherits workspace font family.
    pub font_family: Option<String>,
    /// Omission inherits workspace cursor blink.
    pub cursor_blink: Option<bool>,
    /// Inherit macOS Option handling when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_option_is_meta: Option<bool>,
    /// Inherit Backspace encoding when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backspace_send: Option<String>,
    /// Inherit Delete encoding when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_send: Option<String>,
    /// Fixed terminal base palette; omission inherits the workspace startup palette.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_preset: Option<crate::settings::TerminalPreset>,
}
/// Literal local-process options; never an executable, shell-source parser, or credential store.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileConfig {
    /// Ordered literal arguments, including empty strings.
    pub args: Vec<String>,
    /// Absolute cwd; no tilde, environment expansion or automatic fallback.
    pub cwd: Option<String>,
    /// Explicit environment overrides, with reserved app identity keys rejected.
    pub env: Vec<(String, String)>,
    /// Names to remove from the inherited environment.
    pub env_remove: Vec<String>,
    /// Drawing-only sparse override.
    pub appearance: ProfileAppearance,
}
impl std::fmt::Debug for ProfileConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileConfig")
            .field("argument_count", &self.args.len())
            .field("environment_count", &self.env.len())
            .finish_non_exhaustive()
    }
}
impl ProfileConfig {
    /// Apply shared display validation and bounded literal-process policy before saving or use.
    pub fn validate(&self) -> Result<()> {
        let mut total = 0usize;
        let mut admit = |s: &str| -> Result<()> {
            total = total.checked_add(s.len() + 1).ok_or(StoreError::Record)?;
            if s.contains('\0') || total > 16 * 1024 {
                return Err(StoreError::Record);
            }
            Ok(())
        };
        if self.args.len() > 64 || self.env.len() + self.env_remove.len() > 64 {
            return Err(StoreError::Record);
        }
        for arg in &self.args {
            admit(arg)?;
        }
        if let Some(cwd) = &self.cwd {
            if !std::path::Path::new(cwd).is_absolute() || cwd.len() > 4096 {
                return Err(StoreError::Record);
            }
            admit(cwd)?;
        }
        let mut keys = BTreeSet::new();
        for (key, value) in &self.env {
            validate_key(key)?;
            if !keys.insert(key.to_ascii_uppercase()) {
                return Err(StoreError::Record);
            }
            admit(key)?;
            admit(value)?;
        }
        for key in &self.env_remove {
            validate_key(key)?;
            if !keys.insert(key.to_ascii_uppercase()) {
                return Err(StoreError::Record);
            }
            admit(key)?;
        }
        if let Some(family) = &self.appearance.font_family {
            admit(family)?;
        }
        SettingsPatch {
            terminal: TerminalPatch {
                color_preset: self.appearance.color_preset,
                font_size: self.appearance.font_size,
                font_family: self.appearance.font_family.clone(),
                cursor_blink: self.appearance.cursor_blink,
                mac_option_as_meta: self.appearance.mac_option_is_meta,
                backspace_send: self.appearance.backspace_send.clone(),
                delete_send: self.appearance.delete_send.clone(),
                ..Default::default()
            },
            ..Default::default()
        }
        .validate()
        .map_err(|_| StoreError::Record)
    }
}
fn validate_key(key: &str) -> Result<()> {
    if key.is_empty()
        || key.to_ascii_uppercase().starts_with("PULSE_")
        || key.len() > 128
        || !key
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
        || [
            "TERM_PROGRAM",
            "TERM_PROGRAM_VERSION",
            "PULSE_PANE_ID",
            "PULSE_SOCKET",
        ]
        .iter()
        .any(|name| key.eq_ignore_ascii_case(name))
    {
        return Err(StoreError::Record);
    }
    Ok(())
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u8,
    revision: u64,
    config: ProfileConfig,
}
/// Joint store-instance-bound configuration and lifecycle source. Cannot be supplied by a page.
#[derive(Clone, PartialEq, Eq)]
pub struct ProfileEditorSnapshot {
    owner: Uuid,
    shell: AutomaticShell,
    record: Option<Record>,
    lifecycle: DefaultProfileSnapshot,
}
impl ProfileEditorSnapshot {
    /// Current configuration, with absence explicitly meaning inherited defaults.
    pub fn config(&self) -> ProfileConfig {
        self.record
            .as_ref()
            .map(|r| r.config.clone())
            .unwrap_or_default()
    }
    /// Joint lifecycle source; used to reject deleted identities before startup.
    pub fn lifecycle(&self) -> &DefaultProfileSnapshot {
        &self.lifecycle
    }
}
type Gate = Arc<dyn Fn() -> bool + Send + Sync>;
pub(crate) enum EditorRequest {
    Load(
        Uuid,
        AutomaticShell,
        Box<dyn Send>,
        oneshot::Sender<Result<ProfileEditorSnapshot>>,
    ),
    Save(
        Box<ProfileEditorSnapshot>,
        ProfileConfig,
        Gate,
        oneshot::Sender<Result<ProfileEditorSnapshot>>,
    ),
}
impl StateStore {
    /// Capture bounded configuration and lifecycle in one SQLite read transaction.
    pub async fn load_profile_editor(
        &self,
        shell: AutomaticShell,
    ) -> Result<ProfileEditorSnapshot> {
        self.load_profile_editor_retaining(shell, ()).await
    }
    /// Hold native I/O credit until actual worker exit, even when the waiter cancels.
    pub async fn load_profile_editor_retaining<R: Send + 'static>(
        &self,
        shell: AutomaticShell,
        resource: R,
    ) -> Result<ProfileEditorSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::ProfileEditor(Box::new(EditorRequest::Load(
            self.host_owner,
            shell,
            Box::new(resource),
            reply,
        ))))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// CAS original joint source and check live authorization at the SQL commit boundary.
    pub async fn save_profile_editor(
        &self,
        expected: ProfileEditorSnapshot,
        config: ProfileConfig,
        authorize: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<ProfileEditorSnapshot> {
        if expected.owner != self.host_owner {
            return Err(StoreError::Conflict);
        }
        config.validate()?;
        let (reply, result) = oneshot::channel();
        self.admit(Request::ProfileEditor(Box::new(EditorRequest::Save(
            Box::new(expected),
            config,
            Arc::new(authorize),
            reply,
        ))))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
fn key(shell: AutomaticShell) -> String {
    format!("profiles.editor.unix.{shell:?}.v1")
}
fn read(
    connection: &Connection,
    owner: Uuid,
    shell: AutomaticShell,
) -> Result<ProfileEditorSnapshot> {
    let value: Option<(i64, String)> = connection
        .query_row(
            "SELECT length(CAST(value AS BLOB)), substr(value,1,32769) FROM app_state WHERE key=?1",
            [key(shell)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| StoreError::Database)?;
    let record = match value {
        None => None,
        Some((size, text)) if (1..=LIMIT as i64).contains(&size) => {
            let r: Record = serde_json::from_str(&text).map_err(|_| StoreError::Record)?;
            if r.schema_version != 1 || r.revision == 0 || r.revision > i64::MAX as u64 {
                return Err(StoreError::Record);
            }
            r.config.validate()?;
            Some(r)
        }
        _ => return Err(StoreError::Record),
    };
    Ok(ProfileEditorSnapshot {
        owner,
        shell,
        record,
        lifecycle: crate::profile_default::read(connection, owner)?,
    })
}
fn load(
    connection: &Connection,
    owner: Uuid,
    shell: AutomaticShell,
) -> Result<ProfileEditorSnapshot> {
    let tx = rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    let snapshot = read(&tx, owner, shell)?;
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(snapshot)
}
fn save(
    connection: &Connection,
    expected: ProfileEditorSnapshot,
    config: ProfileConfig,
    allowed: impl Fn() -> bool,
) -> Result<ProfileEditorSnapshot> {
    config.validate()?;
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    let tx = rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    if read(&tx, expected.owner, expected.shell)? != expected {
        return Err(StoreError::Conflict);
    }
    if expected
        .lifecycle
        .preferences()
        .deleted()
        .any(|s| s == expected.shell)
    {
        return Err(StoreError::Record);
    }
    let revision = expected
        .record
        .as_ref()
        .map_or(0, |r| r.revision)
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or(StoreError::Record)?;
    let text = serde_json::to_string(&Record {
        schema_version: 1,
        revision,
        config,
    })
    .map_err(|_| StoreError::Record)?;
    if text.len() > LIMIT {
        return Err(StoreError::Record);
    }
    tx.execute("INSERT INTO app_state(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![key(expected.shell),text]).map_err(|_| StoreError::Database)?;
    crate::profile_default::advance_lifecycle_revision(&tx)?;
    let saved = read(&tx, expected.owner, expected.shell)?;
    if !allowed() {
        return Err(StoreError::ProfileAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(saved)
}
pub(crate) fn serve(connection: &Connection, request: EditorRequest) {
    match request {
        EditorRequest::Load(owner, shell, _resource, reply) if !reply.is_closed() => {
            let _ = reply.send(load(connection, owner, shell));
        }
        EditorRequest::Save(expected, config, gate, reply) if !reply.is_closed() => {
            let result = save(connection, *expected, config, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gate()))
                        .unwrap_or(false)
                    && !reply.is_closed()
            });
            let _ = reply.send(result);
        }
        _ => {}
    }
}
#[cfg(test)]
#[path = "profile_editor_tests.rs"]
mod tests;
