use super::*;
use crate::profiles::{AutoProfileEdit, AutomaticShell as Shell};
use std::sync::atomic::{AtomicUsize, Ordering};
fn private_dir() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
#[tokio::test]
async fn default_reopens_without_changing_legacy_preferences_and_clear_keeps_revision()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    store
        .edit_auto_profiles(AutoProfileEdit::Visibility {
            shell: Shell::Fish,
            hidden: true,
        })
        .await?;
    let before = store.load_auto_profiles().await?;
    let initial = store.load_default_profile().await?;
    assert_eq!(initial.shell(), None);
    let selected = store
        .save_default_profile(initial.clone(), Some(Shell::Bash), || true)
        .await?;
    assert_eq!(selected.shell(), Some(Shell::Bash));
    assert_eq!(store.load_auto_profiles().await?, before);
    let cleared = store.save_default_profile(selected, None, || true).await?;
    assert_eq!(cleared.shell(), None);
    assert_eq!(
        cleared.record.as_ref().ok_or("missing revision")?.revision,
        2
    );
    assert_eq!(
        store
            .save_default_profile(initial, Some(Shell::Sh), || true)
            .await,
        Err(StoreError::Conflict)
    );
    let selected = store
        .save_default_profile(cleared, Some(Shell::Sh), || true)
        .await?;
    drop(store);
    task.join().await?;
    let (store, task) = StateStore::open(path).await?;
    let reopened = store.load_default_profile().await?;
    assert_eq!(reopened.shell(), Some(Shell::Sh));
    assert_eq!(reopened.preferences(), &before);
    assert_eq!(
        store.save_default_profile(selected, None, || true).await,
        Err(StoreError::Conflict)
    );
    drop(store);
    task.join().await?;
    Ok(())
}
#[tokio::test]
async fn two_workers_cas_rejects_old_default_and_changed_visibility()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let (a, ta) = StateStore::open(path.clone()).await?;
    let (b, tb) = StateStore::open(path).await?;
    let old = a.load_default_profile().await?;
    let foreign = b.load_default_profile().await?;
    assert_eq!(
        a.save_default_profile(foreign.clone(), Some(Shell::Sh), || true)
            .await,
        Err(StoreError::Conflict)
    );
    b.save_default_profile(foreign, Some(Shell::Bash), || true)
        .await?;
    assert_eq!(
        a.save_default_profile(old, Some(Shell::Sh), || true).await,
        Err(StoreError::Conflict)
    );
    let old = a.load_default_profile().await?;
    b.edit_auto_profiles(AutoProfileEdit::Visibility {
        shell: Shell::Bash,
        hidden: true,
    })
    .await?;
    assert_eq!(
        a.save_default_profile(old, None, || true).await,
        Err(StoreError::Conflict)
    );
    let hidden = a.load_default_profile().await?;
    assert_eq!(hidden.shell(), Some(Shell::Bash)); // unavailable, never silently replaced
    assert_eq!(
        a.save_default_profile(hidden.clone(), Some(Shell::Bash), || true)
            .await,
        Err(StoreError::Record)
    );
    b.edit_auto_profiles(AutoProfileEdit::Suppress(Shell::Sh))
        .await?;
    let deleted = a.load_default_profile().await?;
    assert_eq!(
        a.save_default_profile(deleted.clone(), Some(Shell::Sh), || true)
            .await,
        Err(StoreError::Record)
    );
    a.save_default_profile(deleted, None, || true).await?;
    drop(a);
    drop(b);
    ta.join().await?;
    tb.join().await?;
    Ok(())
}
#[test]
fn revoke_after_sql_write_rolls_back_and_panic_denies()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let connection = database::open(&dir.path().join("pulse.db"))?;
    let owner = Uuid::new_v4();
    let initial = capture(&connection, owner)?;
    let calls = AtomicUsize::new(0);
    assert_eq!(
        save(
            &connection,
            initial.clone(),
            Some(DefaultProfileSelection::Automatic(Shell::Bash)),
            || calls.fetch_add(1, Ordering::SeqCst) < 2
        ),
        Err(StoreError::ProfileAuthorization)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(capture(&connection, owner)?, initial);
    let (reply, mut result) = oneshot::channel();
    serve(
        &connection,
        DefaultRequest::Save(
            initial.clone(),
            Some(DefaultProfileSelection::Automatic(Shell::Bash)),
            Arc::new(|| panic!("fixture gate")),
            reply,
        ),
    );
    assert_eq!(result.try_recv()?, Err(StoreError::ProfileAuthorization));
    assert_eq!(capture(&connection, owner)?, initial);
    Ok(())
}
#[test]
fn malformed_records_and_revision_exhaustion_never_reset()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let connection = database::open(&dir.path().join("pulse.db"))?;
    let owner = Uuid::new_v4();
    for text in [
        r#"{"schemaVersion":2,"revision":1,"shell":"bash"}"#,
        r#"{"schemaVersion":1,"revision":0,"shell":"bash"}"#,
        r#"{"schemaVersion":1,"revision":1,"shell":"unknown"}"#,
        r#"{"schemaVersion":1,"revision":1,"shell":"bash","shell":"sh"}"#,
        r#"{"schemaVersion":1,"revision":1,"shell":"bash","path":"/bin/sh"}"#,
        &"x".repeat(257),
    ] {
        connection.execute(
            "INSERT OR REPLACE INTO app_state(key,value) VALUES (?1,?2)",
            rusqlite::params![KEY, text],
        )?;
        assert_eq!(capture(&connection, owner), Err(StoreError::Record));
        let after: String =
            connection.query_row("SELECT value FROM app_state WHERE key=?1", [KEY], |row| {
                row.get(0)
            })?;
        assert_eq!(after, text);
    }
    let text = format!(
        r#"{{"schemaVersion":1,"revision":{},"shell":null}}"#,
        i64::MAX
    );
    connection.execute(
        "UPDATE app_state SET value=?2 WHERE key=?1",
        rusqlite::params![KEY, text],
    )?;
    let exhausted = capture(&connection, owner)?;
    assert_eq!(
        save(
            &connection,
            exhausted.clone(),
            Some(DefaultProfileSelection::Automatic(Shell::Sh)),
            || true
        ),
        Err(StoreError::Record)
    );
    assert_eq!(capture(&connection, owner)?, exhausted);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_queued_read_keeps_reservation_until_worker_shutdown()
-> std::result::Result<(), Box<dyn std::error::Error>> {
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
    let dir = private_dir()?;
    let (store, task) = StateStore::open(dir.path().join("pulse.db")).await?;
    let snapshot = store.load_default_profile().await?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(gate.clone());
    let (entered, entry) = std::sync::mpsc::sync_channel(1);
    let calls = AtomicUsize::new(0);
    let writer = store.clone();
    let saving = tokio::spawn(async move {
        writer
            .save_default_profile(snapshot, Some(Shell::Sh), move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    if entered.send(()).is_err() {
                        return false;
                    }
                    let Ok(released) = gate.0.lock() else {
                        return false;
                    };
                    return gate
                        .1
                        .wait_timeout_while(released, Duration::from_secs(5), |released| !*released)
                        .is_ok_and(|(released, _)| *released);
                }
                true
            })
            .await
    });
    entry.recv_timeout(Duration::from_secs(3))?;
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = slots.clone().acquire_owned().await?;
    let capacity = store.requests.capacity();
    let reader = store.clone();
    let reading = tokio::spawn(async move { reader.load_default_profile_retaining(permit).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.requests.capacity() == capacity {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    reading.abort();
    let _ = reading.await;
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
    let (store, task) = StateStore::open(dir.path().join("pulse.db")).await?;
    assert_eq!(store.load_default_profile().await?.shell(), None);
    drop(store);
    task.join().await?;
    Ok(())
}

#[tokio::test]
async fn delete_restore_roundtrip_retires_previous_default_snapshot()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let (store, task) = StateStore::open(dir.path().join("pulse.db")).await?;
    let initial = store.load_default_profile().await?;
    let old = store
        .save_default_profile(initial, Some(Shell::Sh), || true)
        .await?;
    store
        .edit_auto_profiles(AutoProfileEdit::Suppress(Shell::Sh))
        .await?;
    let deleted = store.load_default_profile().await?;
    assert_eq!(deleted.shell(), Some(Shell::Sh));
    store
        .edit_auto_profiles(AutoProfileEdit::RestoreDeleted(Shell::Sh))
        .await?;
    let result = store.save_default_profile(old, None, || true).await;
    drop(store);
    task.join().await?;
    assert_eq!(result, Err(StoreError::Conflict));
    Ok(())
}
