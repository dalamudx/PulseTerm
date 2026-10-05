use crate::{
    Result, StoreError,
    settings::SettingsPatch,
    settings_scope::{
        RECORD_LIMIT, ScopedSettingsEdit, ScopedSettingsSnapshot, SettingsLayers, SettingsScope,
    },
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;
pub(crate) fn load(
    connection: &Connection,
    scope: SettingsScope,
) -> Result<ScopedSettingsSnapshot> {
    let key = scope.key()?;
    let row:Option<(i64,Vec<u8>)>=connection.query_row("SELECT length(CAST(value AS BLOB)),substr(CAST(value AS BLOB),1,8193) FROM app_state WHERE key=?1",[key],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(|_|StoreError::Database)?;
    match row {
        None => Ok(ScopedSettingsSnapshot::empty(scope)),
        Some((length, bytes)) if (0..=RECORD_LIMIT as i64).contains(&length) => {
            ScopedSettingsSnapshot::decode(scope, &bytes)
        }
        _ => Err(StoreError::SettingsRecord),
    }
}
fn write(
    transaction: &Transaction<'_>,
    scope: SettingsScope,
    current: ScopedSettingsSnapshot,
    patch: SettingsPatch,
) -> Result<ScopedSettingsSnapshot> {
    patch.validate().map_err(|_| StoreError::SettingsRecord)?;
    if patch == current.patch {
        return Ok(current);
    }
    let revision = current
        .revision
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or(StoreError::SettingsRevision)?;
    let next = ScopedSettingsSnapshot {
        scope,
        revision,
        patch,
    };
    transaction.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![scope.key()?,next.encode()?]).map_err(|_|StoreError::Database)?;
    Ok(next)
}
pub(crate) fn edit(
    connection: &Connection,
    scope: SettingsScope,
    edit: ScopedSettingsEdit,
) -> Result<ScopedSettingsSnapshot> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = load(&transaction, scope)?;
    let mut patch = current.patch.clone();
    edit.apply(&mut patch)?;
    let next = write(&transaction, scope, current, patch)?;
    transaction.commit().map_err(|_| StoreError::Database)?;
    Ok(next)
}
pub(crate) fn replace(
    connection: &Connection,
    expected: &ScopedSettingsSnapshot,
    patch: SettingsPatch,
) -> Result<ScopedSettingsSnapshot> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|_| StoreError::Database)?;
    let current = load(&transaction, expected.scope)?;
    if current.revision != expected.revision || current.patch != expected.patch {
        return Err(StoreError::SettingsConflict);
    }
    let next = write(&transaction, expected.scope, current, patch)?;
    transaction.commit().map_err(|_| StoreError::Database)?;
    Ok(next)
}
pub(crate) fn layers(
    connection: &Connection,
    group: Option<Uuid>,
    host: Option<Uuid>,
) -> Result<SettingsLayers> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    let result = read_layers(&transaction, group, host)?;
    transaction.commit().map_err(|_| StoreError::Database)?;
    Ok(result)
}
pub(crate) fn read_layers(
    transaction: &Transaction<'_>,
    group: Option<Uuid>,
    host: Option<Uuid>,
) -> Result<SettingsLayers> {
    Ok(SettingsLayers {
        group: group
            .map(|id| load(transaction, SettingsScope::Group(id)))
            .transpose()?,
        host: host
            .map(|id| load(transaction, SettingsScope::Host(id)))
            .transpose()?,
    })
}
