#![allow(clippy::unwrap_used)]
use super::*;
use std::{ffi::OsString, task::Poll};
use tokio::time::{Duration, timeout};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn pages() -> (
    SessionRegistry,
    pulse_app::product_document::ProductDocuments,
    Arc<PageLaunchScope>,
    Arc<PageLaunchScope>,
) {
    let registry = SessionRegistry::new();
    let documents = pulse_app::product_document::ProductDocuments::new(registry.clone());
    let mut scopes = Vec::new();
    for _ in 0..2 {
        let owner = registry.open_window().unwrap();
        documents.install(owner).unwrap();
        documents.started(owner).unwrap();
        let nonce = documents.finished(owner).unwrap();
        documents.confirm(owner, nonce).unwrap();
        scopes.push(documents.current(owner, nonce).unwrap());
    }
    (registry, documents, scopes.remove(0), scopes.remove(0))
}
fn readers() -> (Arc<ProfileStorage>, Arc<ProfileReader>, Arc<ProfileReader>) {
    let storage = Arc::new(ProfileStorage::new());
    let a = Arc::new(ProfileReader::with_storage(storage.clone()));
    let b = Arc::new(ProfileReader::with_storage(storage.clone()));
    (storage, a, b)
}
#[tokio::test]
async fn retiring_one_window_keeps_same_real_sql_worker_and_other_page_usable() -> TestResult {
    let temp = tempfile::tempdir()?;
    let (registry, _documents, page_a, page_b) = pages();
    let (storage, a, b) = readers();
    let directory = temp.path().join("state");
    a.list(
        registry.clone(),
        page_a.clone(),
        directory.clone(),
        OsString::from("/bin"),
        None,
    )
    .await?;
    let store = storage.store.get().unwrap().store.clone();
    let before = store.load_auto_profiles().await?;
    a.close();
    timeout(Duration::from_secs(2), a.drain_window()).await??;
    assert!(storage.store.get().unwrap().task.lock().unwrap().is_some());
    assert!(
        a.list(
            registry.clone(),
            page_a,
            directory.clone(),
            OsString::from("/bin"),
            None
        )
        .await
        .is_err()
    );
    b.list(
        registry.clone(),
        page_b,
        directory,
        OsString::from("/bin"),
        None,
    )
    .await?;
    assert_eq!(store.load_auto_profiles().await?, before);
    b.drain_window().await?;
    assert!(store.load_auto_profiles().await.is_ok());
    let (one, two) = tokio::join!(storage.shutdown(), storage.shutdown());
    one?;
    two?;
    assert!(storage.store.get().unwrap().task.lock().unwrap().is_none());
    assert!(store.load_auto_profiles().await.is_err());
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn cancelled_native_holder_keeps_global_credit_without_blocking_unrelated_window_drain()
-> TestResult {
    let (storage, a, b) = readers();
    let credit = a.reserve()?;
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
    let mut waiting = Box::pin(a.drain_window());
    std::future::poll_fn(|cx| {
        assert!(matches!(waiting.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert_eq!(storage.credit.available_permits(), 0);
    assert!(b.reserve().is_err());
    timeout(Duration::from_secs(2), b.drain_window()).await??;
    assert!(!storage.is_closed());
    release.send(())?;
    timeout(Duration::from_secs(2), a.drain_window()).await??;
    let c = Arc::new(ProfileReader::with_storage(storage.clone()));
    let credit = c.reserve()?;
    assert_eq!(storage.credit.available_permits(), 0);
    drop(credit);
    c.drain_window().await?;
    storage.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn cancelled_queued_window_releases_only_its_local_slot_not_foreign_native_holder()
-> TestResult {
    let (registry, _documents, _page_a, page_b) = pages();
    let (storage, a, b) = readers();
    let held = a.reserve()?;
    let mut pending = Box::pin(b.reserve_list(&registry, &page_b));
    std::future::poll_fn(|cx| {
        assert!(matches!(pending.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    assert_eq!(b.credit.available_permits(), 0);
    b.close();
    assert!(timeout(Duration::from_secs(2), pending).await?.is_err());
    b.drain_window().await?;
    assert_eq!(b.credit.available_permits(), 1);
    assert_eq!(storage.credit.available_permits(), 0);
    assert!(!storage.is_closed());
    drop(held);
    a.drain_window().await?;
    storage.shutdown().await?;
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn cancelled_application_exit_still_waits_holder_and_joins_real_worker_once() -> TestResult {
    let temp = tempfile::tempdir()?;
    let (registry, _documents, page_a, _page_b) = pages();
    let (storage, a, b) = readers();
    a.list(
        registry.clone(),
        page_a,
        temp.path().join("state"),
        OsString::from("/bin"),
        None,
    )
    .await?;
    let store = storage.store.get().unwrap().store.clone();
    let held = a.reserve()?;
    let mut waiting = Box::pin(storage.shutdown());
    std::future::poll_fn(|cx| {
        assert!(matches!(waiting.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(storage.shutdown.get().unwrap().borrow().is_none());
    assert!(b.reserve().is_err());
    assert!(a.reserve().is_err());
    drop(held);
    let (one, two) = tokio::join!(a.shutdown(), b.shutdown());
    one?;
    two?;
    assert!(storage.store.get().unwrap().task.lock().unwrap().is_none());
    assert!(store.load_auto_profiles().await.is_err());
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn other_window_cannot_change_application_store_directory() -> TestResult {
    let temp = tempfile::tempdir()?;
    let (registry, _documents, page_a, page_b) = pages();
    let (storage, a, b) = readers();
    a.list(
        registry.clone(),
        page_a,
        temp.path().join("first"),
        OsString::from("/bin"),
        None,
    )
    .await?;
    let foreign = temp.path().join("other");
    assert_eq!(
        b.list(
            registry.clone(),
            page_b,
            foreign.clone(),
            OsString::from("/bin"),
            None
        )
        .await
        .err(),
        Some("workspace.denied")
    );
    assert!(!foreign.exists());
    a.drain_window().await?;
    b.drain_window().await?;
    storage.shutdown().await?;
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn permit_that_loses_close_race_wakes_pending_physical_shutdown() -> TestResult {
    let storage = Arc::new(ProfileStorage::new());
    let raw = storage.credit.clone().acquire_owned().await?;
    let mut waiting = Box::pin(storage.shutdown());
    std::future::poll_fn(|cx| {
        assert!(matches!(waiting.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    assert!(storage.accept(raw).is_err());
    timeout(Duration::from_secs(2), waiting).await??;
    assert_eq!(storage.credit.available_permits(), 1);
    Ok(())
}
