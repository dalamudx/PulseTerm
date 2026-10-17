#![allow(clippy::unwrap_used)]
use super::*;
use std::{ffi::OsString, task::Poll};
use tokio::time::{Duration, timeout};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn page() -> (
    SessionRegistry,
    pulse_app::product_document::ProductDocuments,
    pulse_app::registry::WindowToken,
    Arc<PageLaunchScope>,
) {
    let registry = SessionRegistry::new();
    let documents = pulse_app::product_document::ProductDocuments::new(registry.clone());
    let owner = registry.open_window().unwrap();
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let scope = documents.current(owner, nonce).unwrap();
    (registry, documents, owner, scope)
}

#[tokio::test]
async fn cancelled_waiter_and_blocking_wait_do_not_publish_shutdown_until_actual_closure_exit()
-> TestResult {
    let reader = Arc::new(ProfileReader::new());
    let credit = reader.reserve()?;
    let (entered, entering) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let block = tokio::spawn(blocking(credit, move || {
        let _ = entered.send(());
        released.recv().map_err(|_| "workspace.unavailable")?;
        Ok(())
    }));
    entering.await?;
    block.abort();
    assert!(block.await.is_err());
    let mut waiting = Box::pin(reader.shutdown());
    std::future::poll_fn(|cx| {
        assert!(matches!(waiting.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert_eq!(reader.credit.available_permits(), 0);
    assert!(reader.reserve().is_err());
    assert!(reader.shutdown.get().unwrap().borrow().is_none());
    release.send(())?;
    timeout(Duration::from_secs(2), reader.shutdown()).await??;
    assert_eq!(reader.credit.available_permits(), 1);
    reader.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn simultaneous_shutdown_waiters_share_real_database_join_and_no_reopen() -> TestResult {
    let temp = tempfile::tempdir()?;
    let (registry, _documents, _owner, page) = page();
    let reader = Arc::new(ProfileReader::new());
    reader
        .list(
            registry.clone(),
            page.clone(),
            temp.path().join("state"),
            OsString::from("/bin"),
            None,
        )
        .await?;
    let store = reader.store.get().unwrap().store.clone();
    let (a, b) = tokio::join!(reader.shutdown(), reader.shutdown());
    a?;
    b?;
    assert!(reader.store.get().unwrap().task.lock().unwrap().is_none());
    assert!(store.load_auto_profiles().await.is_err());
    assert!(
        reader
            .list(
                registry.clone(),
                page,
                temp.path().join("other"),
                OsString::from("/bin"),
                None
            )
            .await
            .is_err()
    );
    assert!(!temp.path().join("other").exists());
    registry.shutdown()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn close_before_queued_source_prepare_creates_no_database_and_waits_supervisor() -> TestResult
{
    let temp = tempfile::tempdir()?;
    let (registry, _documents, _owner, page) = page();
    let reader = Arc::new(ProfileReader::new());
    let mut work = Box::pin(reader.list(
        registry.clone(),
        page,
        temp.path().join("absent"),
        OsString::from("/bin"),
        None,
    ));
    tokio::select! { biased; _ = &mut work => panic!("unexpected completion"), _ = std::future::ready(()) => {} }
    reader.close();
    assert!(work.await.is_err());
    reader.shutdown().await?;
    assert!(!temp.path().join("absent").exists());
    registry.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn source_published_after_close_is_joined_before_shared_shutdown_completes() -> TestResult {
    let temp = tempfile::tempdir()?;
    let reader = Arc::new(ProfileReader::new());
    let credit = reader.reserve()?;
    let mut waiting = Box::pin(reader.shutdown());
    std::future::poll_fn(|cx| {
        assert!(matches!(waiting.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    // Model the check-before-publication race, using a real SQLite worker.
    let directory = pulse_store::state_directory::prepare(&temp.path().join("state"))?;
    let (store, task) = StateStore::open(directory.join("state.db")).await?;
    reader
        .store
        .set(StoreState {
            directory,
            store: store.clone(),
            task: Mutex::new(Some(task)),
        })
        .map_err(|_| "unexpected duplicate source")?;
    assert!(reader.shutdown.get().unwrap().borrow().is_none());
    drop(credit);
    timeout(Duration::from_secs(2), waiting).await??;
    assert!(store.load_auto_profiles().await.is_err());
    assert!(reader.store.get().unwrap().task.lock().unwrap().is_none());
    Ok(())
}

#[tokio::test]
async fn poisoned_task_ownership_is_failed_for_all_waiters_not_a_clean_exit() -> TestResult {
    let temp = tempfile::tempdir()?;
    let (registry, _documents, _owner, page) = page();
    let reader = Arc::new(ProfileReader::new());
    reader
        .list(
            registry.clone(),
            page,
            temp.path().join("state"),
            OsString::from("/bin"),
            None,
        )
        .await?;
    let store = reader.store.get().unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = store.task.lock().unwrap();
            panic!("simulated ownership failure");
        }))
        .is_err()
    );
    assert!(reader.shutdown().await.is_err());
    assert!(reader.shutdown().await.is_err());
    // Test-only recovery: do not leave the simulated failed worker alive.
    let task = store
        .task
        .lock()
        .err()
        .unwrap()
        .into_inner()
        .take()
        .unwrap();
    task.shutdown().await?;
    registry.shutdown()?;
    Ok(())
}
