use super::admit;
use pulse_platform::windows::PrivateDirectory;
use pulse_store::{
    StateStore, StoreError,
    profiles::{AutoProfilePreferences, AutomaticShell},
};
use std::path::PathBuf;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn directory() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("data");
    drop(PrivateDirectory::create(&path)?);
    Ok((root, path))
}
fn seed_v1(path: &std::path::Path) -> TestResult {
    let pin = PrivateDirectory::open(path)?.database()?;
    let connection = rusqlite::Connection::open(path.join("state.db"))?;
    connection.execute_batch("CREATE TABLE app_state (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL) STRICT; PRAGMA user_version=1;")?;
    connection.close().map_err(|(_, error)| error)?;
    drop(pin);
    Ok(())
}

#[tokio::test]
async fn native_worker_persists_profiles_and_releases_file_and_directory_after_shutdown()
-> TestResult {
    let (_root, directory) = directory()?;
    let path = directory.join("state.db");
    let (store, task) = StateStore::open_admitted(path.clone(), admit, ())
        .await
        .map_err(|error| format!("open fresh worker: {error}"))?;
    let checked: Result<_, StoreError> = async {
        let before = store.load_auto_profiles().await?;
        let mut next = before.clone();
        for (name, shell) in [
            ("powershell", AutomaticShell::PowerShell),
            ("cmd", AutomaticShell::Cmd),
            ("wsl", AutomaticShell::Wsl),
        ] {
            assert_eq!(
                pulse_store::profile_default::DefaultProfileSelection::parse(name),
                Some(pulse_store::profile_default::DefaultProfileSelection::Automatic(shell)),
            );
            let default = store.load_default_profile().await?;
            let saved = store
                .save_default_profile(default, Some(shell), || true)
                .await?;
            assert_eq!(saved.shell(), Some(shell));
            next.set_hidden(shell, true)?;
        }
        store.save_auto_profiles(before, next.clone()).await?;
        Ok(next)
    }
    .await;
    let moved = directory.join("moved.db");
    let pinned = std::fs::rename(&path, &moved).is_err();
    if !pinned {
        std::fs::rename(&moved, &path)?;
    }
    let closed = task.shutdown().await;
    closed?;
    let expected = checked?;
    assert!(pinned);
    std::fs::rename(&path, &moved)?;
    std::fs::rename(&moved, &path)?;
    let (reopened, task) = StateStore::open_admitted(path, admit, ()).await?;
    let restored = reopened.load_auto_profiles().await;
    let selected = reopened.load_default_profile().await;
    task.shutdown().await?;
    assert_eq!(restored?, expected);
    assert_eq!(selected?.shell(), Some(AutomaticShell::Wsl));
    let moved_directory = directory.with_file_name("moved-data");
    std::fs::rename(&directory, &moved_directory)?;
    std::fs::rename(moved_directory, directory)?;
    Ok(())
}

#[tokio::test]
async fn native_migration_creates_a_readable_private_backup_of_the_old_schema() -> TestResult {
    let (_root, directory) = directory()?;
    seed_v1(&directory).map_err(|error| format!("seed old schema: {error}"))?;
    let (store, task) = StateStore::open_admitted(directory.join("state.db"), admit, ())
        .await
        .map_err(|error| format!("open migrating worker: {error}"))?;
    let loaded = store.load_auto_profiles().await;
    task.shutdown().await?;
    assert_eq!(loaded?, AutoProfilePreferences::default());
    let backup = rusqlite::Connection::open_with_flags(
        directory.join("state.db.bak-5"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    assert_eq!(
        backup.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
        1
    );
    backup.close().map_err(|(_, error)| error)?;
    let parent = PrivateDirectory::open(&directory)?;
    parent.open_file("state.db.bak-5")?;
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!directory.join(format!("state.db.bak-5{suffix}")).exists());
    }
    Ok(())
}

#[tokio::test]
async fn existing_backup_and_its_bytes_are_not_overwritten_during_migration() -> TestResult {
    let (_root, directory) = directory()?;
    seed_v1(&directory)?;
    drop(PrivateDirectory::open(&directory)?.create_file(
        "state.db.bak-5",
        b"preserve backup",
        15,
    )?);
    let result = StateStore::open_admitted(directory.join("state.db"), admit, ()).await;
    let rejected = matches!(&result, Err(StoreError::Backup));
    if let Ok((_, task)) = result {
        task.shutdown().await?;
    }
    assert!(rejected);
    assert_eq!(
        std::fs::read(directory.join("state.db.bak-5"))?,
        b"preserve backup"
    );
    let original = rusqlite::Connection::open(directory.join("state.db"))?;
    assert_eq!(
        original.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
        1
    );
    original.close().map_err(|(_, error)| error)?;
    Ok(())
}

#[tokio::test]
async fn readonly_wal_rejects_before_sqlite_changes_its_bytes_or_permissions() -> TestResult {
    let (_root, directory) = directory()?;
    let wal = directory.join("state.db-wal");
    drop(PrivateDirectory::open(&directory)?.create_file(
        "state.db-wal",
        b"preserve sidecar",
        16,
    )?);
    let original = std::fs::metadata(&wal)?.permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&wal, readonly)?;
    let result = StateStore::open_admitted(directory.join("state.db"), admit, ()).await;
    let still_readonly = std::fs::metadata(&wal)?.permissions().readonly();
    let bytes = std::fs::read(&wal);
    std::fs::set_permissions(&wal, original)?;
    let rejected = matches!(&result, Err(StoreError::Location));
    if let Ok((_, task)) = result {
        task.shutdown().await?;
    }
    assert!(rejected && still_readonly);
    assert_eq!(bytes?, b"preserve sidecar");
    Ok(())
}

#[tokio::test]
async fn existing_hard_linked_main_file_rejects_without_touching_either_name() -> TestResult {
    let (root, directory) = directory()?;
    let path = directory.join("state.db");
    drop(PrivateDirectory::open(&directory)?.create_file("state.db", b"untouched", 9)?);
    let alias = root.path().join("outside.db");
    std::fs::hard_link(&path, &alias)?;
    let result = StateStore::open_admitted(path.clone(), admit, ()).await;
    let rejected = matches!(&result, Err(StoreError::Location));
    if let Ok((_, task)) = result {
        task.shutdown().await?;
    }
    assert!(rejected);
    assert_eq!(std::fs::read(path)?, b"untouched");
    assert_eq!(std::fs::read(alias)?, b"untouched");
    Ok(())
}
