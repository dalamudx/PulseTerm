use super::*;
use profiles::{AutoProfileEdit as Edit, AutoProfilePreferences as Prefs, AutomaticShell as Shell};
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
async fn shutdown_wakes_idle_worker_while_clients_remain_and_preserves_commit()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    let clone = store.clone();
    let committed = store
        .edit_auto_profiles(Edit::Suppress(Shell::Bash))
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), task.shutdown()).await??;
    assert_eq!(store.load_auto_profiles().await, Err(StoreError::Closed));
    assert_eq!(
        clone
            .edit_auto_profiles(Edit::RestoreDeleted(Shell::Bash))
            .await,
        Err(StoreError::Closed)
    );
    let (fresh, task) = StateStore::open(path).await?;
    assert_eq!(fresh.load_auto_profiles().await?, committed);
    task.shutdown().await?;
    drop(fresh);
    drop(store);
    drop(clone);
    Ok(())
}
#[tokio::test]
async fn dropping_owner_closes_idle_worker_without_dropping_client()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let (store, task) = StateStore::open(dir.path().join("pulse.db")).await?;
    drop(task);
    assert_eq!(store.load_auto_profiles().await, Err(StoreError::Closed));
    tokio::time::timeout(std::time::Duration::from_secs(2), store.requests.closed()).await?;
    Ok(())
}
#[tokio::test]
async fn full_queue_cancellation_wakes_without_credit_and_never_executes_pending_edits()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("pulse.db");
    let (requests, mut inbox) = mpsc::channel(REQUEST_LIMIT);
    let cancellation = CancellationToken::new();
    let store = StateStore {
        requests,
        cancellation: cancellation.clone(),
        host_owner: uuid::Uuid::new_v4(),
        #[cfg(feature = "ssh-trust")]
        trust_owner: uuid::Uuid::new_v4(),
    };
    let mut results = Vec::new();
    for _ in 0..REQUEST_LIMIT {
        let (reply, result) = oneshot::channel();
        store.admit(Request::Edit(Edit::Suppress(Shell::Bash), reply))?;
        results.push(result);
    }
    assert_eq!(inbox.len(), REQUEST_LIMIT);
    let (reply, _result) = oneshot::channel();
    assert_eq!(store.admit(Request::Load(reply)), Err(StoreError::Busy));
    let worker_cancel = cancellation.clone();
    let runtime = tokio::runtime::Handle::current();
    let worker_path = path.clone();
    // Prepare the actual schema before measuring cancellation of the full request queue.
    // Database initialization/migration latency is not the cancellation wakeup under test.
    let connection = tokio::task::spawn_blocking(move || database::open(&worker_path)).await??;
    let (start, wait) = std::sync::mpsc::channel();
    let task = tokio::task::spawn_blocking(move || {
        wait.recv().map_err(|_| StoreError::Closed)?;
        serve(connection, &mut inbox, &worker_cancel, &runtime, None)
    });
    let task = StateStoreTask {
        task: Some(task),
        cancellation,
    };
    task.cancel();
    assert_eq!(store.load_auto_profiles().await, Err(StoreError::Closed));
    start.send(())?;
    tokio::time::timeout(std::time::Duration::from_secs(2), task.join()).await??;
    for result in results {
        assert!(result.await.is_err());
    }
    let (fresh, task) = StateStore::open(path).await?;
    assert_eq!(fresh.load_auto_profiles().await?, Prefs::default());
    task.shutdown().await?;
    Ok(())
}
#[test]
fn lost_reply_after_actual_commit_does_not_roll_back_durable_edit()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let connection = database::open(&dir.path().join("pulse.db"))?;
    let (reply, result) = oneshot::channel();
    let committed = database::edit(&connection, Edit::Suppress(Shell::Bash))?;
    drop(result);
    assert!(reply.send(committed.clone()).is_err());
    assert_eq!(database::load(&connection)?, committed);
    Ok(())
}
