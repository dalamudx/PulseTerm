use super::*;
use crate::profiles::{AutoProfileEdit, AutomaticShell as Shell};
use std::sync::atomic::{AtomicUsize, Ordering};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn fixture() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

#[tokio::test]
async fn lifecycle_reopen_keeps_deleted_default_and_explicit_restore_advances_source() -> TestResult
{
    let dir = fixture()?;
    let path = dir.path().join("pulse.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    let initial = store.load_default_profile().await?;
    store
        .save_default_profile(initial, Some(Shell::Sh), || true)
        .await?;
    store
        .edit_auto_profiles(AutoProfileEdit::Visibility {
            shell: Shell::Sh,
            hidden: true,
        })
        .await?;
    let before = store.load_default_profile().await?;
    let deleted = store
        .change_profile_lifecycle(
            before.clone(),
            ProfileLifecycleEdit::Delete(Shell::Sh),
            || true,
        )
        .await?;
    assert_eq!(deleted.shell(), Some(Shell::Sh));
    assert_eq!(
        deleted.preferences().deleted().collect::<Vec<_>>(),
        [Shell::Sh]
    );
    assert_eq!(deleted.preferences().hidden().count(), 0);
    assert_ne!(deleted.record, before.record);
    drop(store);
    task.join().await?;
    let (store, task) = StateStore::open(path).await?;
    let reopened = store.load_default_profile().await?;
    assert_eq!(reopened.record, deleted.record);
    assert_eq!(reopened.preferences(), deleted.preferences());
    assert_eq!(
        store
            .change_profile_lifecycle(deleted, ProfileLifecycleEdit::Restore(Shell::Sh), || true)
            .await,
        Err(StoreError::Conflict)
    );
    let restored = store
        .change_profile_lifecycle(reopened, ProfileLifecycleEdit::Restore(Shell::Sh), || true)
        .await?;
    assert_eq!(restored.shell(), Some(Shell::Sh));
    assert_eq!(restored.preferences(), &AutoProfilePreferences::default());
    assert_eq!(restored.record.as_ref().ok_or("record")?.revision, 3);
    assert_eq!(
        store
            .change_profile_lifecycle(
                restored.clone(),
                ProfileLifecycleEdit::Restore(Shell::Sh),
                || true
            )
            .await,
        Err(StoreError::Record)
    );
    assert_eq!(store.load_default_profile().await?, restored);
    drop(store);
    task.join().await?;
    Ok(())
}

#[tokio::test]
async fn lifecycle_cross_worker_and_external_default_or_visibility_changes_conflict() -> TestResult
{
    let dir = fixture()?;
    let path = dir.path().join("pulse.db");
    let (a, ta) = StateStore::open(path.clone()).await?;
    let (b, tb) = StateStore::open(path).await?;
    let initial = a.load_default_profile().await?;
    let foreign = b.load_default_profile().await?;
    assert_eq!(
        a.change_profile_lifecycle(
            foreign.clone(),
            ProfileLifecycleEdit::Delete(Shell::Sh),
            || true
        )
        .await,
        Err(StoreError::Conflict)
    );
    b.save_default_profile(foreign, Some(Shell::Sh), || true)
        .await?;
    assert_eq!(
        a.change_profile_lifecycle(initial, ProfileLifecycleEdit::Delete(Shell::Sh), || true)
            .await,
        Err(StoreError::Conflict)
    );
    let old = a.load_default_profile().await?;
    b.edit_auto_profiles(AutoProfileEdit::Visibility {
        shell: Shell::Bash,
        hidden: true,
    })
    .await?;
    assert_eq!(
        a.change_profile_lifecycle(old, ProfileLifecycleEdit::Delete(Shell::Sh), || true)
            .await,
        Err(StoreError::Conflict)
    );
    let old = a.load_default_profile().await?;
    b.edit_auto_profiles(AutoProfileEdit::Suppress(Shell::Sh))
        .await?;
    let deleted = a.load_default_profile().await?;
    assert_eq!(
        a.change_profile_lifecycle(deleted, ProfileLifecycleEdit::Delete(Shell::Sh), || true)
            .await,
        Err(StoreError::Record)
    );
    b.edit_auto_profiles(AutoProfileEdit::RestoreDeleted(Shell::Sh))
        .await?;
    assert_eq!(
        a.change_profile_lifecycle(old, ProfileLifecycleEdit::Delete(Shell::Sh), || true)
            .await,
        Err(StoreError::Conflict)
    );
    drop(a);
    drop(b);
    ta.join().await?;
    tb.join().await?;
    Ok(())
}

#[test]
fn lifecycle_denial_after_both_writes_rolls_back_record_and_tombstone() -> TestResult {
    let dir = fixture()?;
    let connection = database::open(&dir.path().join("pulse.db"))?;
    let owner = Uuid::new_v4();
    let initial = capture(&connection, owner)?;
    let before = save(
        &connection,
        initial,
        Some(DefaultProfileSelection::Automatic(Shell::Sh)),
        || true,
    )?;
    for edit in [
        ProfileLifecycleEdit::Delete(Shell::Sh),
        ProfileLifecycleEdit::Restore(Shell::Sh),
    ] {
        let current = capture(&connection, owner)?;
        let calls = AtomicUsize::new(0);
        assert_eq!(
            change_lifecycle(&connection, current.clone(), edit, || calls
                .fetch_add(1, Ordering::SeqCst)
                < 2),
            Err(StoreError::ProfileAuthorization)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(capture(&connection, owner)?, current);
        let (reply, mut result) = oneshot::channel();
        serve(
            &connection,
            DefaultRequest::Lifecycle(
                current.clone(),
                edit,
                Arc::new(|| panic!("fixture gate")),
                reply,
            ),
        );
        assert_eq!(result.try_recv()?, Err(StoreError::ProfileAuthorization));
        assert_eq!(capture(&connection, owner)?, current);
        change_lifecycle(&connection, current, edit, || true)?;
    }
    let after = capture(&connection, owner)?;
    assert_eq!(after.preferences(), before.preferences());
    assert_ne!(after.record, before.record);
    Ok(())
}

#[test]
fn lifecycle_legacy_snapshot_write_retires_old_source_and_corruption_rolls_back() -> TestResult {
    let dir = fixture()?;
    let connection = database::open(&dir.path().join("pulse.db"))?;
    let owner = Uuid::new_v4();
    let initial = capture(&connection, owner)?;
    let mut deleted = initial.preferences().clone();
    deleted.suppress(Shell::Sh);
    database::save(&connection, initial.preferences(), &deleted)?;
    database::save(&connection, &deleted, initial.preferences())?;
    assert_eq!(
        save(
            &connection,
            initial,
            Some(DefaultProfileSelection::Automatic(Shell::Sh)),
            || true
        ),
        Err(StoreError::Conflict)
    );
    for record in [
        "invalid".to_owned(),
        format!(
            r#"{{"schemaVersion":1,"revision":{},"shell":null}}"#,
            i64::MAX
        ),
    ] {
        connection.execute(
            "UPDATE app_state SET value=?2 WHERE key=?1",
            rusqlite::params![KEY, record],
        )?;
        let preferences = database::load(&connection)?;
        assert_eq!(
            database::edit(&connection, AutoProfileEdit::Suppress(Shell::Sh)),
            Err(StoreError::Record)
        );
        assert_eq!(database::load(&connection)?, preferences);
        let actual: String =
            connection.query_row("SELECT value FROM app_state WHERE key=?1", [KEY], |row| {
                row.get(0)
            })?;
        assert_eq!(actual, record);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_cancel_during_commit_gate_retains_native_credit_and_shutdown_barrier()
-> TestResult {
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;
    struct Release(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Ok(mut released) = self.0.0.lock() {
                *released = true;
                self.0.1.notify_all();
            }
        }
    }
    let dir = fixture()?;
    let path = dir.path().join("pulse.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    let snapshot = store.load_default_profile().await?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(gate.clone());
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = slots.clone().acquire_owned().await?;
    let (entered, entry) = std::sync::mpsc::sync_channel(1);
    let calls = AtomicUsize::new(0);
    let writer = store.clone();
    let saving = tokio::spawn(async move {
        writer
            .change_profile_lifecycle(
                snapshot,
                ProfileLifecycleEdit::Delete(Shell::Sh),
                move || {
                    let _retain = &permit;
                    if calls.fetch_add(1, Ordering::SeqCst) == 2 {
                        if entered.send(()).is_err() {
                            return false;
                        }
                        let Ok(released) = gate.0.lock() else {
                            return false;
                        };
                        return gate
                            .1
                            .wait_timeout_while(released, Duration::from_secs(5), |released| {
                                !*released
                            })
                            .is_ok_and(|(released, _)| *released);
                    }
                    true
                },
            )
            .await
    });
    entry.recv_timeout(Duration::from_secs(3))?;
    saving.abort();
    let _ = saving.await;
    assert_eq!(slots.available_permits(), 0);
    let shutdown = tokio::spawn(task.shutdown());
    tokio::task::yield_now().await;
    assert!(!shutdown.is_finished());
    assert_eq!(slots.available_permits(), 0);
    drop(release);
    tokio::time::timeout(Duration::from_secs(3), shutdown).await???;
    assert_eq!(slots.available_permits(), 1);
    drop(store);
    let (store, task) = StateStore::open(path).await?;
    let after = store.load_default_profile().await?;
    assert_eq!(after.record, None);
    assert_eq!(after.preferences(), &AutoProfilePreferences::default());
    drop(store);
    task.join().await?;
    Ok(())
}
