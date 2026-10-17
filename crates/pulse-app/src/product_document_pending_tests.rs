//! Native metadata checkpoint tests, not real WebView or action admission.
#![allow(clippy::unwrap_used)]
use super::*;
use std::{future::Future, task::Poll, time::Duration};
fn fixture() -> (ProductDocuments, WindowToken) {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let documents = ProductDocuments::new(registry);
    documents.install(owner).unwrap();
    (documents, owner)
}
#[test]
fn pending_document_requires_finished_and_never_confirms_itself() {
    let (documents, owner) = fixture();
    assert!(documents.capture_pending_native(owner).is_err());
    documents.started(owner).unwrap();
    assert!(documents.capture_pending_native(owner).is_err());
    let nonce = documents.finished(owner).unwrap();
    let captured = documents.capture_pending_native(owner).unwrap();
    assert!(
        documents
            .resolve_pending_native(&captured)
            .unwrap()
            .is_none()
    );
    assert!(documents.current_native(owner).is_err());
    assert!(documents.confirm(owner, Uuid::new_v4()).is_err());
    assert!(
        documents
            .resolve_pending_native(&captured)
            .unwrap()
            .is_none()
    );
    documents.confirm(owner, nonce).unwrap();
    let resolved = documents
        .resolve_pending_native(&captured)
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(
        &resolved,
        &documents.current_native(owner).unwrap()
    ));
    let ready_capture = documents.capture_pending_native(owner).unwrap();
    assert!(Arc::ptr_eq(
        &resolved,
        &documents
            .resolve_pending_native(&ready_capture)
            .unwrap()
            .unwrap()
    ));
}
#[test]
fn pending_document_cannot_upgrade_even_if_a_new_document_reuses_nonce_bytes() {
    let (documents, owner) = fixture();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    let old = documents.capture_pending_native(owner).unwrap();
    documents.started(owner).unwrap();
    assert!(documents.resolve_pending_native(&old).is_err());
    documents.finished(owner).unwrap();
    // Simulate a nonce collision only inside this private test. A fresh checkpoint
    // must still prevent the old pending intent from borrowing the new scope.
    if let Some(Phase::Awaiting { nonce: next, .. }) = documents.lock().unwrap().get_mut(&owner) {
        *next = nonce;
    } else {
        panic!("test did not reach Awaiting");
    }
    let next = documents.capture_pending_native(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    assert!(documents.resolve_pending_native(&old).is_err());
    assert!(documents.resolve_pending_native(&next).unwrap().is_some());
}
#[test]
fn pending_document_is_bound_to_controller_and_removal_lifetime() {
    let (documents, owner) = fixture();
    documents.started(owner).unwrap();
    documents.finished(owner).unwrap();
    let captured = documents.capture_pending_native(owner).unwrap();
    let other = ProductDocuments::new(documents.registry.clone());
    other.install(owner).unwrap();
    other.started(owner).unwrap();
    other.finished(owner).unwrap();
    assert!(other.resolve_pending_native(&captured).is_err());
    documents.remove(owner).unwrap();
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    documents.finished(owner).unwrap();
    assert!(documents.resolve_pending_native(&captured).is_err());
    let foreign = SessionRegistry::new();
    assert!(
        documents
            .capture_pending_native(foreign.open_window().unwrap())
            .is_err()
    );
}
#[test]
fn pending_document_invalid_event_revokes_capture_without_borrowing_other_window() {
    let (documents, owner) = fixture();
    documents.started(owner).unwrap();
    documents.finished(owner).unwrap();
    let captured = documents.capture_pending_native(owner).unwrap();
    let other = documents.registry.open_window().unwrap();
    documents.install(other).unwrap();
    documents.started(other).unwrap();
    let nonce = documents.finished(other).unwrap();
    documents.confirm(other, nonce).unwrap();
    assert!(
        documents
            .resolve_pending_native(&captured)
            .unwrap()
            .is_none()
    );
    assert!(documents.finished(owner).is_err());
    assert!(documents.resolve_pending_native(&captured).is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn pending_document_wait_registered_before_confirm_returns_original_scope() {
    let (documents, owner) = fixture();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    let captured = documents.capture_pending_native(owner).unwrap();
    let mut waiting = Box::pin(documents.wait_pending_native(&captured, Duration::from_secs(1)));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    documents.confirm(owner, nonce).unwrap();
    let resolved = tokio::time::timeout(Duration::from_millis(100), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(
        &resolved,
        &documents.current_native(owner).unwrap()
    ));
}
#[tokio::test(flavor = "current_thread")]
async fn pending_document_wait_wakes_on_navigation_remove_and_invalid_finished() {
    for event in 0..3 {
        let (documents, owner) = fixture();
        documents.started(owner).unwrap();
        documents.finished(owner).unwrap();
        let captured = documents.capture_pending_native(owner).unwrap();
        let mut waiting =
            Box::pin(documents.wait_pending_native(&captured, Duration::from_secs(1)));
        std::future::poll_fn(|cx| {
            assert!(waiting.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        match event {
            0 => documents.started(owner).unwrap(),
            1 => documents.remove(owner).unwrap(),
            _ => {
                assert!(documents.finished(owner).is_err());
            }
        }
        assert!(matches!(
            tokio::time::timeout(Duration::from_millis(100), waiting)
                .await
                .unwrap(),
            Err(RegistryError::Forbidden)
        ));
    }
}
#[tokio::test(flavor = "current_thread")]
async fn pending_document_wait_rejects_unbounded_budget_and_times_out_without_activation() {
    let (documents, owner) = fixture();
    documents.started(owner).unwrap();
    documents.finished(owner).unwrap();
    let captured = documents.capture_pending_native(owner).unwrap();
    for budget in [
        Duration::ZERO,
        Duration::from_secs(9),
        Duration::from_millis(5),
    ] {
        assert!(matches!(
            documents.wait_pending_native(&captured, budget).await,
            Err(RegistryError::Busy)
        ));
    }
    assert!(documents.current_native(owner).is_err());
    assert!(
        documents
            .resolve_pending_native(&captured)
            .unwrap()
            .is_none()
    );
}
#[tokio::test(flavor = "current_thread")]
async fn pending_document_wait_cancellation_does_not_confirm_or_consume_document() {
    let (documents, owner) = fixture();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    let captured = documents.capture_pending_native(owner).unwrap();
    let mut waiting = Box::pin(documents.wait_pending_native(&captured, Duration::from_secs(1)));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(documents.current_native(owner).is_err());
    documents.confirm(owner, nonce).unwrap();
    assert!(
        documents
            .wait_pending_native(&captured, Duration::from_secs(1))
            .await
            .is_ok()
    );
}
