#![allow(clippy::unwrap_used)]
use super::*;
use std::{future::Future, task::Poll};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn binding(directory: &WindowDirectory, primary: bool) -> Arc<NativeBinding> {
    let registration = Arc::new(directory.reserve(primary).unwrap());
    let binding = Arc::new(NativeBinding::new(directory.clone(), registration).unwrap());
    binding.publish().unwrap();
    binding
}
fn document(binding: &NativeBinding) -> Arc<pulse_app::registry::PageLaunchScope> {
    let documents = binding.documents();
    documents.started(binding.owner()).unwrap();
    let nonce = documents.finished(binding.owner()).unwrap();
    documents.confirm(binding.owner(), nonce).unwrap();
    documents.current(binding.owner(), nonce).unwrap()
}
async fn pending(binding: &NativeBinding) {
    let mut waiting = Box::pin(binding.wait());
    std::future::poll_fn(|cx| {
        assert!(matches!(waiting.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    drop(waiting);
}
#[tokio::test]
async fn io_without_destroyed_keeps_capacity_after_waiter_cancellation() -> TestResult {
    let directory = WindowDirectory::new(SessionRegistry::new());
    let first = binding(&directory, true);
    let remaining = (0..31)
        .map(|_| binding(&directory, false))
        .collect::<Vec<_>>();
    first.io_finished(Ok(()))?;
    pending(&first).await;
    assert!(first.needs_close());
    assert!(directory.reserve(false).is_err());
    first.destroyed()?;
    first.wait().await?;
    assert!(!first.needs_close());
    assert!(directory.registration_owner(&first.registration).is_err());
    let replacement = binding(&directory, false);
    first.destroyed()?;
    assert!(directory.reserve(false).is_err());
    for item in remaining.iter().chain(std::iter::once(&replacement)) {
        item.destroyed()?;
        item.io_finished(Ok(()))?;
        item.wait().await?;
    }
    Ok(())
}
#[tokio::test]
async fn destroyed_revokes_exact_page_but_waits_io_and_preserves_other_window() -> TestResult {
    let registry = SessionRegistry::new();
    let directory = WindowDirectory::new(registry.clone());
    let first = binding(&directory, true);
    let second = binding(&directory, false);
    let page_one = document(&first);
    let page_two = document(&second);
    first.destroyed()?;
    assert!(registry.check_launch_page(&page_one).is_err());
    registry.check_launch_page(&page_two)?;
    pending(&first).await;
    assert!(first.publish().is_err());
    first.io_finished(Ok(()))?;
    first.wait().await?;
    registry.check_launch_page(&page_two)?;
    second.destroyed()?;
    second.io_finished(Ok(()))?;
    second.wait().await?;
    Ok(())
}
#[tokio::test]
async fn failed_io_cannot_release_or_upgrade_native_slot_after_destroyed() -> TestResult {
    let directory = WindowDirectory::new(SessionRegistry::new());
    let first = binding(&directory, true);
    let remaining = (0..31)
        .map(|_| binding(&directory, false))
        .collect::<Vec<_>>();
    first.io_finished(Err("workspace.shutdown_failed"))?;
    pending(&first).await;
    assert!(first.destroyed().is_err());
    assert!(first.wait().await.is_err());
    assert!(first.io_finished(Ok(())).is_err());
    assert!(directory.reserve(false).is_err());
    for item in remaining {
        item.destroyed()?;
        item.io_finished(Ok(()))?;
    }
    assert!(first.wait().await.is_err());
    Ok(())
}
#[tokio::test]
async fn foreign_same_label_reservation_cannot_revoke_or_publish_another_directory() -> TestResult {
    let one = WindowDirectory::new(SessionRegistry::new());
    let two_registry = SessionRegistry::new();
    let two = WindowDirectory::new(two_registry.clone());
    let existing = binding(&one, true);
    let registration = Arc::new(two.reserve(true)?);
    assert_eq!(existing.label(), registration.label());
    assert!(NativeBinding::new(one, registration.clone()).is_err());
    two.registration_owner(&registration)?;
    let original = NativeBinding::new(two, registration)?;
    original.publish()?;
    let page = document(&original);
    two_registry.check_launch_page(&page)?;
    existing.destroyed()?;
    existing.io_finished(Ok(()))?;
    original.destroyed()?;
    original.io_finished(Ok(()))?;
    Ok(())
}
#[tokio::test]
async fn production_factory_rejects_documents_from_another_application() -> TestResult {
    let application = super::super::tests::application();
    let directory = WindowDirectory::new(application.registry.clone());
    let native = binding(&directory, true);
    let profiles = Arc::new(profiles::ProfileReader::with_storage(
        application.profile_storage.clone(),
    ));
    assert!(Workspace::from_native(application.clone(), native.clone(), profiles).is_err());
    assert!(application.window_drains.native_labels().is_empty());
    native.destroyed()?;
    native.io_finished(Ok(()))?;
    application.window_drains.shutdown().await?;
    application.profile_storage.shutdown().await?;
    application.settings_io.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn builder_failure_never_fabricates_destroyed_or_releases_capacity() -> TestResult {
    let directory = WindowDirectory::new(SessionRegistry::new());
    let registration = Arc::new(directory.reserve(true)?);
    let native = Arc::new(NativeBinding::new(directory.clone(), registration)?);
    let remaining = (0..31)
        .map(|_| binding(&directory, false))
        .collect::<Vec<_>>();
    native.creation_failed()?;
    assert!(native.wait().await.is_err());
    assert!(native.needs_close());
    assert!(directory.reserve(false).is_err());
    assert!(native.io_finished(Ok(())).is_err());
    assert!(native.destroyed().is_err());
    assert!(directory.reserve(false).is_err());
    for item in remaining {
        item.destroyed()?;
        item.io_finished(Ok(()))?;
        item.wait().await?;
    }
    Ok(())
}
