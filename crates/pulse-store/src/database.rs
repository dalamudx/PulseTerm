use crate::{Result, StoreError, profiles::AutoProfilePreferences};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use rusqlite_migration::{M, Migrations};
use std::{
    fs::{self, OpenOptions},
    path::Path,
    time::Duration,
};
const KEY: &str = "profiles.automatic.unix.v1";
const SCHEMA: &str =
    "CREATE TABLE app_state (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL) STRICT;";
fn private_file(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| p.is_dir())
        .ok_or(StoreError::Location)?;
    let metadata = fs::symlink_metadata(parent).map_err(|_| StoreError::Location)?;
    if !metadata.is_dir() {
        return Err(StoreError::Location);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o777 != 0o700 || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(StoreError::Location);
        }
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(StoreError::Location),
    }
    private_regular(&fs::symlink_metadata(path).map_err(|_| StoreError::Location)?)?;
    // SQLite may touch sidecars during open; reject pre-existing aliases/foreign files first.
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        match fs::symlink_metadata(Path::new(&name)) {
            Ok(metadata) => private_regular(&metadata)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(StoreError::Location),
        }
    }
    Ok(())
}
fn private_regular(metadata: &fs::Metadata) -> Result<()> {
    if !metadata.is_file() {
        return Err(StoreError::Location);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o777 != 0o600
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.nlink() != 1
        {
            return Err(StoreError::Location);
        }
    }
    Ok(())
}
fn v2_steps() -> Vec<M<'static>> {
    vec![
        M::up(SCHEMA),
        M::up_with_hook(
            crate::vault_database::relational::SCHEMA,
            |transaction: &rusqlite::Transaction<'_>| {
                crate::vault_database::relational::import_legacy(transaction).map_err(|_| {
                    rusqlite_migration::HookError::Hook("vault.migration_rejected".into())
                })
            },
        ),
    ]
}
fn v3_steps() -> Vec<M<'static>> {
    let mut steps = v2_steps();
    steps.push(M::up_with_hook(
        crate::ssh_identities::SCHEMA,
        |transaction: &rusqlite::Transaction<'_>| {
            crate::vault_database::load(transaction, uuid::Uuid::new_v4())
                .map(|_| ())
                .map_err(|_| rusqlite_migration::HookError::Hook("identity.source_rejected".into()))
        },
    ));
    steps
}
fn v4_steps() -> Vec<M<'static>> {
    let mut steps = v3_steps();
    steps.push(M::up_with_hook(
        crate::ssh_trust::relational::SCHEMA,
        |tx| {
            crate::ssh_trust::relational::import_legacy(tx)
                .map_err(|_| rusqlite_migration::HookError::Hook("trust.migration_rejected".into()))
        },
    ));
    steps
}
pub(crate) fn open(path: &Path) -> Result<Connection> {
    let mut steps = v4_steps();
    steps.push(M::up_with_hook(crate::ssh_key_identities::SCHEMA, |tx| {
        let owner = uuid::Uuid::new_v4();
        crate::ssh_identities::load(tx, owner)
            .and_then(|_| crate::ssh_key_identities::load(tx, owner))
            .map(|_| ())
            .map_err(|_| rusqlite_migration::HookError::Hook("key_identity.source_rejected".into()))
    }));
    let connection = open_with_migrations(path, &Migrations::new(steps), 5)?;
    let owner = uuid::Uuid::new_v4();
    let validated = crate::ssh_trust::load(&connection, owner)?;
    crate::ssh_identities::load(&connection, owner)?;
    crate::ssh_key_identities::load(&connection, owner)?;
    if validated.owner != owner {
        return Err(StoreError::HostTrustConflict);
    }
    Ok(connection)
}
#[cfg(test)]
pub(crate) fn open_v4_trust_fixture(path: &Path) -> Result<Connection> {
    open_with_migrations(path, &Migrations::new(v4_steps()), 4)
}
#[cfg(all(test, feature = "ssh-trust"))]
pub(crate) fn open_v3_fixture(path: &Path) -> Result<Connection> {
    open_with_migrations(path, &Migrations::new(v3_steps()), 3)
}
#[cfg(test)]
pub(crate) fn open_v2_fixture(path: &Path) -> Result<Connection> {
    open_with_migrations(path, &Migrations::new(v2_steps()), 2)
}
#[cfg(test)]
pub(crate) fn open_v1_fixture(path: &Path) -> Result<Connection> {
    open_with_migrations(path, &Migrations::new(vec![M::up(SCHEMA)]), 1)
}
fn open_with_migrations(
    path: &Path,
    migrations: &Migrations<'_>,
    target: u32,
) -> Result<Connection> {
    private_file(path)?;
    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|_| StoreError::Database)?;
    connection
        .busy_timeout(Duration::from_millis(250))
        .map_err(|_| StoreError::Database)?;
    migrate(&mut connection, path, migrations, target)?;
    let journal: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .map_err(|_| StoreError::Database)?;
    if journal != "wal" {
        return Err(StoreError::Database);
    }
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|_| StoreError::Database)?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|_| StoreError::Database)?;
    Ok(connection)
}
pub(crate) fn migrate(
    connection: &mut Connection,
    path: &Path,
    migrations: &Migrations<'_>,
    target: u32,
) -> Result<()> {
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| StoreError::Schema)?;
    if version > target {
        return Err(StoreError::Schema);
    }
    if version == 0 {
        let tables: u32 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Database)?;
        if tables != 0 {
            return Err(StoreError::Schema);
        }
    }
    if version == target {
        return Ok(());
    }
    if version > 0 {
        let mut backup_path = path.as_os_str().to_os_string();
        backup_path.push(format!(".bak-{target}"));
        let backup_path = std::path::PathBuf::from(backup_path);
        let mut options = OpenOptions::new();
        options.create_new(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&backup_path).map_err(|_| StoreError::Backup)?;
        let mut dest = Connection::open_with_flags(
            &backup_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| StoreError::Backup)?;
        {
            let backup = rusqlite::backup::Backup::new(connection, &mut dest)
                .map_err(|_| StoreError::Backup)?;
            if !matches!(
                backup.step(-1).map_err(|_| StoreError::Backup)?,
                rusqlite::backup::StepResult::Done
            ) {
                return Err(StoreError::Backup);
            }
        }
        dest.close().map_err(|_| StoreError::Backup)?;
        file.sync_all().map_err(|_| StoreError::Backup)?;
        #[cfg(unix)]
        {
            let parent = path.parent().ok_or(StoreError::Backup)?;
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|_| StoreError::Backup)?;
        }
    }
    migrations
        .to_latest(connection)
        .map_err(|_| StoreError::Database)
}
pub(crate) fn load(connection: &Connection) -> Result<AutoProfilePreferences> {
    let value: Option<(i64, String)> = connection
        .query_row(
            "SELECT length(CAST(value AS BLOB)), substr(value,1,4097) FROM app_state WHERE key=?1",
            [KEY],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|error| profile_sql_error("read", error))?;
    match value {
        None => Ok(AutoProfilePreferences::default()),
        Some((bytes, text)) if (0..=4096).contains(&bytes) => AutoProfilePreferences::decode(&text),
        _ => Err(StoreError::Record),
    }
}
fn profile_sql_error(_stage: &'static str, _error: rusqlite::Error) -> StoreError {
    // Synthetic tests only: retain numeric SQL category, never message, SQL, path or record.
    #[cfg(test)]
    if let rusqlite::Error::SqliteFailure(code, _) = &_error {
        eprintln!(
            "store.profile_sql_failure stage={_stage} primary={:?} extended={}",
            code.code, code.extended_code
        );
    }
    StoreError::Database
}
fn transaction(connection: &Connection) -> Result<rusqlite::Transaction<'_>> {
    rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| profile_sql_error("begin", error))
}
pub(crate) fn write(
    transaction: &rusqlite::Transaction<'_>,
    preferences: &AutoProfilePreferences,
) -> Result<()> {
    let previous = load(transaction)?;
    if !previous.deleted().eq(preferences.deleted()) {
        crate::profile_default::advance_lifecycle_revision(transaction)?;
    }
    transaction.execute("INSERT INTO app_state(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![KEY, preferences.encode()?]).map_err(|error| profile_sql_error("write", error))?;
    Ok(())
}
pub(crate) fn save(
    connection: &Connection,
    expected: &AutoProfilePreferences,
    preferences: &AutoProfilePreferences,
) -> Result<()> {
    let transaction = transaction(connection)?;
    let current = load(&transaction)?;
    if &current != expected {
        return Err(StoreError::Conflict);
    }
    write(&transaction, preferences)?;
    transaction.commit().map_err(|_| StoreError::Database)
}
pub(crate) fn edit(
    connection: &Connection,
    edit: crate::profiles::AutoProfileEdit,
) -> Result<AutoProfilePreferences> {
    let transaction = transaction(connection)?;
    let mut current = load(&transaction)?;
    edit.apply(&mut current)?;
    write(&transaction, &current)?;
    transaction
        .commit()
        .map_err(|error| profile_sql_error("commit", error))?;
    Ok(current)
}
