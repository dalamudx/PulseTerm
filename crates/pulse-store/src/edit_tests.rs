use super::*;
use profiles::{AutoProfileEdit as Edit, AutoProfilePreferences as Prefs, AutomaticShell as Shell};
use std::path::Path;
fn private_dir() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::{fs, os::unix::fs::PermissionsExt};
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
fn hide(shell: Shell) -> Edit {
    Edit::Visibility {
        shell,
        hidden: true,
    }
}
async fn combined(store: &StateStore) -> Result<()> {
    let (a, b) = tokio::join!(
        store.edit_auto_profiles(hide(Shell::Bash)),
        store.edit_auto_profiles(Edit::Suppress(Shell::Zsh))
    );
    assert!(a?.hidden().any(|s| s == Shell::Bash));
    assert!(b?.deleted().any(|s| s == Shell::Zsh));
    let mut expected = Prefs::default();
    expected.set_hidden(Shell::Bash, true)?;
    expected.suppress(Shell::Zsh);
    assert_eq!(store.load_auto_profiles().await?, expected);
    Ok(())
}
#[tokio::test]
async fn cloned_clients_merge_edits_and_reopen()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let (a, task) = StateStore::open(path.clone()).await?;
    let b = a.clone();
    combined(&a).await?;
    let (r1, r2) = tokio::join!(
        a.edit_auto_profiles(hide(Shell::Fish)),
        b.edit_auto_profiles(hide(Shell::Sh))
    );
    r1?;
    r2?;
    assert_eq!(a.load_auto_profiles().await?.hidden().count(), 3);
    let expected = a.load_auto_profiles().await?;
    drop(a);
    drop(b);
    task.join().await?;
    let (a, task) = StateStore::open(path).await?;
    assert_eq!(a.load_auto_profiles().await?, expected);
    drop(a);
    task.join().await?;
    Ok(())
}
#[tokio::test]
async fn independent_workers_merge_and_stale_snapshot_is_rejected()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let (a, ta) = StateStore::open(path.clone()).await?;
    let (b, tb) = StateStore::open(path).await?;
    let cached = b.load_auto_profiles().await?;
    let (r1, r2) = tokio::join!(
        a.edit_auto_profiles(hide(Shell::Bash)),
        b.edit_auto_profiles(Edit::Suppress(Shell::Zsh))
    );
    r1?;
    r2?;
    let mut replacement = cached.clone();
    replacement.set_hidden(Shell::Fish, true)?;
    assert_eq!(
        b.save_auto_profiles(cached, replacement).await,
        Err(StoreError::Conflict)
    );
    let mut current = a.load_auto_profiles().await?;
    assert_eq!(current.hidden().collect::<Vec<_>>(), vec![Shell::Bash]);
    assert_eq!(current.deleted().collect::<Vec<_>>(), vec![Shell::Zsh]);
    let expected = current.clone();
    current.set_hidden(Shell::Fish, true)?;
    a.save_auto_profiles(expected, current.clone()).await?;
    assert_eq!(b.load_auto_profiles().await?, current);
    drop(a);
    drop(b);
    ta.join().await?;
    tb.join().await?;
    Ok(())
}
#[tokio::test]
async fn deletion_requires_explicit_restore_and_failed_edit_never_changes_others()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let (a, task) = StateStore::open(dir.path().join("pulse.db")).await?;
    combined(&a).await?;
    let before = a.load_auto_profiles().await?;
    assert_eq!(
        a.edit_auto_profiles(hide(Shell::Zsh)).await,
        Err(StoreError::Record)
    );
    assert_eq!(a.load_auto_profiles().await?, before);
    let after = a
        .edit_auto_profiles(Edit::RestoreDeleted(Shell::Zsh))
        .await?;
    assert!(after.deleted().next().is_none());
    assert_eq!(after.hidden().collect::<Vec<_>>(), vec![Shell::Bash]);
    assert_eq!(
        a.edit_auto_profiles(Edit::RestoreDeleted(Shell::Zsh))
            .await?,
        after
    );
    drop(a);
    task.join().await?;
    Ok(())
}
#[test]
fn malformed_record_and_failed_transaction_do_not_reset_or_overwrite()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let c = database::open(&path)?;
    c.execute(
        "INSERT INTO app_state VALUES('profiles.automatic.unix.v1',?1)",
        ["{bad"],
    )?;
    assert_eq!(
        database::edit(&c, hide(Shell::Bash)),
        Err(StoreError::Record)
    );
    let raw: String = c.query_row("SELECT value FROM app_state", [], |r| r.get(0))?;
    assert_eq!(raw, "{bad");
    c.execute("DELETE FROM app_state", [])?;
    database::edit(&c, hide(Shell::Bash))?;
    c.execute_batch("CREATE TRIGGER deny_write BEFORE UPDATE ON app_state BEGIN SELECT RAISE(ABORT,'rejected'); END;")?;
    assert_eq!(
        database::edit(&c, Edit::Suppress(Shell::Zsh)),
        Err(StoreError::Database)
    );
    assert_eq!(
        database::load(&c)?.hidden().collect::<Vec<_>>(),
        vec![Shell::Bash]
    );
    assert!(database::load(&c)?.deleted().next().is_none());
    Ok(())
}
#[test]
fn actual_sqlite_lock_failure_preserves_state()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let c = database::open(&path)?;
    database::edit(&c, hide(Shell::Bash))?;
    let other = rusqlite::Connection::open(Path::new(&path))?;
    other.execute_batch("BEGIN IMMEDIATE;")?;
    assert_eq!(
        database::edit(&c, Edit::Suppress(Shell::Zsh)),
        Err(StoreError::Database)
    );
    other.execute_batch("ROLLBACK;")?;
    assert_eq!(
        database::load(&c)?.hidden().collect::<Vec<_>>(),
        vec![Shell::Bash]
    );
    assert!(database::load(&c)?.deleted().next().is_none());
    Ok(())
}
