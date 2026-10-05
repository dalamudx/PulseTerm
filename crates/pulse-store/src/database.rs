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
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
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
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let m = fs::symlink_metadata(path).map_err(|_| StoreError::Location)?;
            if !m.is_file() {
                return Err(StoreError::Location);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if m.permissions().mode() & 0o077 != 0 {
                    return Err(StoreError::Location);
                }
            }
        }
        Err(_) => return Err(StoreError::Location),
    }
    Ok(())
}
pub(crate) fn open(path: &Path) -> Result<Connection> {
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
    migrate(
        &mut connection,
        path,
        &Migrations::new(vec![M::up(SCHEMA)]),
        1,
    )?;
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
        .map_err(|_| StoreError::Database)?;
    match value {
        None => Ok(AutoProfilePreferences::default()),
        Some((bytes, text)) if (0..=4096).contains(&bytes) => AutoProfilePreferences::decode(&text),
        _ => Err(StoreError::Record),
    }
}
pub(crate) fn save(connection: &Connection, preferences: &AutoProfilePreferences) -> Result<()> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|_| StoreError::Database)?;
    let _current = load(&transaction)?;
    transaction.execute("INSERT INTO app_state(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![KEY, preferences.encode()?]).map_err(|_| StoreError::Database)?;
    transaction.commit().map_err(|_| StoreError::Database)
}
