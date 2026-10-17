#![allow(clippy::unwrap_used)]
use super::*;
use std::{future::Future, task::Poll};
fn fixture() -> (
    ProductDocuments,
    WindowToken,
    Arc<PageLaunchScope>,
    Presentation,
) {
    let registry = pulse_app::registry::SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let documents = ProductDocuments::new(registry);
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let original = documents.current_native(owner).unwrap();
    (documents, owner, original, Presentation::default())
}
#[tokio::test(flavor = "current_thread")]
async fn confirmed_document_does_not_mean_presentation_ready() {
    let (documents, owner, original, presentation) = fixture();
    let cancel = CancellationToken::new();
    let mut waiting = Box::pin(presentation.wait(&documents, owner, &original, &cancel));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(Arc::ptr_eq(
        &original,
        &documents.current_native(owner).unwrap()
    ));
    presentation
        .mark(&documents, owner, original.clone())
        .unwrap();
    waiting.await.unwrap();
    presentation
        .mark(&documents, owner, original.clone())
        .unwrap();
    presentation
        .wait(&documents, owner, &original, &cancel)
        .await
        .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn navigation_wakes_old_wait_and_late_marker_cannot_replace_new_page() {
    let (documents, owner, original, presentation) = fixture();
    let cancel = CancellationToken::new();
    let mut waiting = Box::pin(presentation.wait(&documents, owner, &original, &cancel));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    documents.started(owner).unwrap();
    presentation.navigation().unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let new = documents.current_native(owner).unwrap();
    presentation.mark(&documents, owner, new.clone()).unwrap();
    assert!(
        presentation
            .mark(&documents, owner, original.clone())
            .is_err()
    );
    assert!(waiting.await.is_err());
    presentation
        .wait(&documents, owner, &new, &cancel)
        .await
        .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn cancellation_is_waiter_local_and_close_blocks_late_ready() {
    let (documents, owner, original, presentation) = fixture();
    let cancel = CancellationToken::new();
    let mut waiting = Box::pin(presentation.wait(&documents, owner, &original, &cancel));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    cancel.cancel();
    assert!(waiting.await.is_err());
    presentation
        .mark(&documents, owner, original.clone())
        .unwrap();
    presentation
        .wait(&documents, owner, &original, &CancellationToken::new())
        .await
        .unwrap();
    presentation.navigation().unwrap();
    let stop = CancellationToken::new();
    let mut next = Box::pin(presentation.wait(&documents, owner, &original, &stop));
    std::future::poll_fn(|cx| {
        assert!(next.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    presentation.close().unwrap();
    assert!(next.await.is_err());
    assert!(presentation.mark(&documents, owner, original).is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn same_numbered_foreign_window_cannot_publish_or_use_marker() {
    let (documents, owner, original, presentation) = fixture();
    let (foreign_documents, foreign_owner, foreign, _) = fixture();
    presentation
        .mark(&documents, owner, original.clone())
        .unwrap();
    assert!(
        presentation
            .mark(&documents, owner, foreign.clone())
            .is_err()
    );
    assert!(
        presentation
            .mark(&documents, foreign_owner, original.clone())
            .is_err()
    );
    assert!(
        presentation
            .wait(
                &foreign_documents,
                foreign_owner,
                &original,
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    presentation
        .wait(&documents, owner, &original, &CancellationToken::new())
        .await
        .unwrap();
}
