#![allow(clippy::unwrap_used)]
use super::*;
use pulse_app::product_document::ProductDocuments;
use std::{fs, os::unix::fs::PermissionsExt, sync::mpsc, time::Duration};
use tokio::{sync::oneshot, time::timeout};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn pages() -> (
    SessionRegistry,
    ProductDocuments,
    Arc<PageLaunchScope>,
    Arc<PageLaunchScope>,
) {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
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
fn owners() -> (Arc<SettingsIo>, Arc<SettingsOwner>, Arc<SettingsOwner>) {
    let io = Arc::new(SettingsIo::new());
    (
        io.clone(),
        Arc::new(SettingsOwner::with_io(io.clone())),
        Arc::new(SettingsOwner::with_io(io)),
    )
}
#[tokio::test]
async fn another_page_save_invalidates_old_capture_and_retirement_preserves_other_writer()
-> TestResult {
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let (registry, _documents, page_a, page_b) = pages();
    let (io, a, b) = owners();
    let dir = directory.path().to_path_buf();
    let capture_a = a
        .read(registry.clone(), page_a.clone(), dir.clone())
        .await?;
    let capture_b = b
        .read(registry.clone(), page_b.clone(), dir.clone())
        .await?;
    a.save(
        registry.clone(),
        page_a,
        dir.clone(),
        capture_a.token,
        Change::FontSize(18),
    )
    .await?;
    assert_eq!(
        b.save(
            registry.clone(),
            page_b.clone(),
            dir.clone(),
            capture_b.token,
            Change::FontSize(20)
        )
        .await
        .err(),
        Some("workspace.settings_save_unconfirmed")
    );
    let fresh = b
        .read(registry.clone(), page_b.clone(), dir.clone())
        .await?;
    assert_eq!(fresh.values.font_size.value, 18);
    a.shutdown().await?;
    let changed = b
        .save(registry, page_b, dir, fresh.token, Change::FontSize(20))
        .await?;
    assert_eq!(changed.values.font_size.value, 20);
    b.shutdown().await?;
    io.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn cancelled_blocking_waiter_keeps_shared_credit_but_other_window_drains() -> TestResult {
    let (io, a, b) = owners();
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = a.clone();
    let waiter = tokio::spawn(async move {
        worker
            .io(move |_| {
                entered_tx.send(()).map_err(|_| "entered")?;
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| "release")?;
                Ok(())
            })
            .await
    });
    timeout(Duration::from_secs(2), entered_rx).await??;
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(matches!(b.reserve(), Err("workspace.busy")));
    timeout(Duration::from_secs(2), b.shutdown()).await??;
    let closing = a.shutdown();
    tokio::pin!(closing);
    assert!(
        timeout(Duration::from_millis(20), &mut closing)
            .await
            .is_err()
    );
    release_tx.send(())?;
    timeout(Duration::from_secs(2), closing).await??;
    io.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn cancelled_application_shutdown_cannot_release_foreign_native_holder() -> TestResult {
    let (io, a, b) = owners();
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = a.clone();
    let holder = tokio::spawn(async move {
        worker
            .io(move |_| {
                entered_tx.send(()).map_err(|_| "entered")?;
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| "release")?;
                Ok(())
            })
            .await
    });
    timeout(Duration::from_secs(2), entered_rx).await??;
    let mut shutdown = Box::pin(io.shutdown());
    assert!(
        timeout(Duration::from_millis(20), &mut shutdown)
            .await
            .is_err()
    );
    drop(shutdown);
    assert!(matches!(b.reserve(), Err("workspace.denied")));
    let again = io.shutdown();
    tokio::pin!(again);
    assert!(
        timeout(Duration::from_millis(20), &mut again)
            .await
            .is_err()
    );
    release_tx.send(())?;
    holder.await??;
    timeout(Duration::from_secs(2), again).await??;
    a.shutdown().await?;
    b.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn another_window_cannot_change_application_settings_directory() -> TestResult {
    let first = tempfile::tempdir()?;
    let second = tempfile::tempdir()?;
    fs::set_permissions(first.path(), fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(second.path(), fs::Permissions::from_mode(0o700))?;
    let (registry, _documents, page_a, page_b) = pages();
    let (io, a, b) = owners();
    a.read(registry.clone(), page_a, first.path().into())
        .await?;
    assert_eq!(
        b.read(registry, page_b, second.path().into()).await.err(),
        Some("workspace.denied")
    );
    assert_eq!(fs::read_dir(second.path())?.count(), 0);
    a.shutdown().await?;
    b.shutdown().await?;
    io.shutdown().await?;
    Ok(())
}
