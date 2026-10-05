use super::*;
use profiles::{AutoProfilePreferences as Prefs, AutomaticShell as Shell};
use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};
use std::{fs, path::Path};
fn private_dir() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
fn value() -> Result<Prefs> {
    let mut p = Prefs::default();
    p.set_hidden(Shell::Bash, true)?;
    p.suppress(Shell::Zsh);
    Ok(p)
}
fn version(c: &Connection) -> rusqlite::Result<u32> {
    c.pragma_query_value(None, "user_version", |r| r.get(0))
}
#[test]
fn strict_preferences_reject_duplicates_conflicts_future_unknown_and_oversize()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    for input in [
        r#"{"schemaVersion":1,"hidden":["bash","bash"],"deleted":[]}"#,
        r#"{"schemaVersion":1,"hidden":["bash"],"deleted":["bash"]}"#,
        r#"{"schemaVersion":2,"hidden":[],"deleted":[]}"#,
        r#"{"schemaVersion":1,"hidden":["unknown"],"deleted":[]}"#,
        r#"{"schemaVersion":1,"hidden":[],"deleted":[],"secret":"value"}"#,
        r#"{"schemaVersion":1,"hidden":[],"deleted":[],"schemaVersion":1}"#,
    ] {
        assert_eq!(Prefs::decode(input), Err(StoreError::Record));
    }
    assert_eq!(Prefs::decode(&" ".repeat(4097)), Err(StoreError::Record));
    let p = value()?;
    assert_eq!(Prefs::decode(&p.encode()?)?, p);
    let mut deleted = p;
    assert_eq!(
        deleted.set_hidden(Shell::Zsh, true),
        Err(StoreError::Record)
    );
    assert!(format!("{deleted:?}").contains("Zsh"));
    Ok(())
}
#[tokio::test]
async fn actual_file_reopens_preferences_and_closes_all_worker_handles()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    assert_eq!(store.load_auto_profiles().await?, Prefs::default());
    store.save_auto_profiles(value()?).await?;
    let clone = store.clone();
    drop(store);
    assert_eq!(clone.load_auto_profiles().await?, value()?);
    drop(clone);
    task.join().await?;
    let (store, task) = StateStore::open(path.clone()).await?;
    assert_eq!(store.load_auto_profiles().await?, value()?);
    drop(store);
    task.join().await?;
    let c = database::open(&path)?;
    assert_eq!(version(&c)?, 1);
    assert_eq!(
        c.pragma_query_value::<String, _>(None, "journal_mode", |r| r.get(0))?,
        "wal"
    );
    assert_eq!(
        c.pragma_query_value::<u32, _>(None, "foreign_keys", |r| r.get(0))?,
        1
    );
    assert_eq!(
        c.pragma_query_value::<u32, _>(None, "synchronous", |r| r.get(0))?,
        2
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    }
    Ok(())
}
#[test]
fn bounded_queue_refuses_overload_closed_and_does_not_admit_failed_request() -> Result<()> {
    let (requests, mut receiver) = mpsc::channel(REQUEST_LIMIT);
    let store = StateStore { requests };
    let mut results = Vec::new();
    for _ in 0..REQUEST_LIMIT {
        let (reply, result) = oneshot::channel();
        store.admit(Request::Load(reply))?;
        results.push(result);
    }
    let (reply, _result) = oneshot::channel();
    assert_eq!(store.admit(Request::Load(reply)), Err(StoreError::Busy));
    assert_eq!(receiver.len(), REQUEST_LIMIT);
    let _ = receiver.try_recv();
    drop(receiver);
    let (reply, _result) = oneshot::channel();
    assert_eq!(store.admit(Request::Load(reply)), Err(StoreError::Closed));
    Ok(())
}
#[test]
fn corrupt_record_is_not_silently_reset_or_overwritten()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let c = database::open(&path)?;
    c.execute(
        "INSERT INTO app_state VALUES ('profiles.automatic.unix.v1',?1)",
        ["{bad record"],
    )?;
    assert_eq!(database::load(&c), Err(StoreError::Record));
    let raw: String = c.query_row("SELECT value FROM app_state", [], |r| r.get(0))?;
    assert_eq!(raw, "{bad record");
    assert_eq!(database::save(&c, &value()?), Err(StoreError::Record));
    let after: String = c.query_row("SELECT value FROM app_state", [], |r| r.get(0))?;
    assert_eq!(after, raw);
    c.execute("UPDATE app_state SET value=?1", [" ".repeat(4097)])?;
    assert_eq!(database::load(&c), Err(StoreError::Record));
    Ok(())
}
#[test]
fn future_or_unversioned_database_is_not_migrated()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    for future in [false, true] {
        let path = dir
            .path()
            .join(if future { "future.db" } else { "legacy.db" });
        let c = database::open(&path)?;
        c.execute_batch(
            "CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('keep');",
        )?;
        c.pragma_update(None, "user_version", if future { 2 } else { 0 })?;
        drop(c);
        assert!(matches!(database::open(&path), Err(StoreError::Schema)));
        let c = Connection::open(&path)?;
        assert_eq!(version(&c)?, if future { 2 } else { 0 });
        let keep: String = c.query_row("SELECT value FROM sentinel", [], |r| r.get(0))?;
        assert_eq!(keep, "keep");
        assert!(!Path::new(&format!("{}.bak-1", path.display())).exists());
    }
    Ok(())
}
fn upgrade() -> Migrations<'static> {
    Migrations::new(vec![
        M::up("CREATE TABLE app_state(key TEXT PRIMARY KEY NOT NULL,value TEXT NOT NULL) STRICT;"),
        M::up("CREATE TABLE migrated(value INTEGER);"),
    ])
}
#[test]
fn upgrade_copies_committed_wal_state_before_schema_change()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let mut c = database::open(&path)?;
    database::save(&c, &value()?)?;
    database::migrate(&mut c, &path, &upgrade(), 2)?;
    assert_eq!(version(&c)?, 2);
    let backup = Connection::open(path.with_file_name("pulse.db.bak-2"))?;
    assert_eq!(version(&backup)?, 1);
    assert_eq!(database::load(&backup)?, value()?);
    assert_eq!(database::load(&c)?, value()?);
    Ok(())
}
#[test]
fn backup_collision_refuses_upgrade_and_preserves_existing_file()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let mut c = database::open(&path)?;
    let backup = dir.path().join("pulse.db.bak-2");
    fs::write(&backup, b"do-not-overwrite")?;
    assert_eq!(
        database::migrate(&mut c, &path, &upgrade(), 2),
        Err(StoreError::Backup)
    );
    assert_eq!(version(&c)?, 1);
    assert_eq!(fs::read(&backup)?, b"do-not-overwrite");
    Ok(())
}
#[test]
fn failed_upgrade_rolls_back_and_keeps_preupgrade_backup()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let mut c = database::open(&path)?;
    database::save(&c, &value()?)?;
    let steps = Migrations::new(vec![
        M::up("CREATE TABLE app_state(key TEXT PRIMARY KEY NOT NULL,value TEXT NOT NULL) STRICT;"),
        M::up("CREATE TABLE rolled_back(value INTEGER); INSERT INTO missing VALUES(1);"),
    ]);
    assert_eq!(
        database::migrate(&mut c, &path, &steps, 2),
        Err(StoreError::Database)
    );
    assert_eq!(version(&c)?, 1);
    assert_eq!(database::load(&c)?, value()?);
    let count: u32 = c.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='rolled_back'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 0);
    let backup = Connection::open(dir.path().join("pulse.db.bak-2"))?;
    assert_eq!(version(&backup)?, 1);
    assert_eq!(database::load(&backup)?, value()?);
    Ok(())
}
#[cfg(unix)]
#[test]
fn public_mode_symlink_and_directory_database_are_rejected()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    drop(database::open(&path)?);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    assert!(matches!(database::open(&path), Err(StoreError::Location)));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let link = dir.path().join("link.db");
    symlink(&path, &link)?;
    assert!(matches!(database::open(&link), Err(StoreError::Location)));
    let sub = dir.path().join("directory.db");
    fs::create_dir(&sub)?;
    assert!(matches!(database::open(&sub), Err(StoreError::Location)));
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755))?;
    assert!(matches!(database::open(&path), Err(StoreError::Location)));
    Ok(())
}
