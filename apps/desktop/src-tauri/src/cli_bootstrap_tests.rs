#![allow(clippy::unwrap_used)]
use super::*;
use std::{future::Future, task::Poll};
fn fixture() -> (ProductDocuments, WindowToken, Bank) {
    let registry = pulse_app::registry::SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let documents = ProductDocuments::new(registry);
    documents.install(owner).unwrap();
    (documents, owner, Bank::default())
}
fn focus() -> StartupAction {
    StartupAction::new("workspace.focus", &serde_json::json!({})).unwrap()
}
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_waits_for_exact_first_finished_and_confirmation_without_executing() {
    let (documents, owner, bank) = fixture();
    let action = focus();
    let id = action.request_id().to_owned();
    bank.enqueue_first(action).unwrap();
    let claim = bank.claim(&id).unwrap();
    let mut waiting = Box::pin(claim.original_scope(&documents));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    bank.started().unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    bank.finished(&documents, owner).unwrap();
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(documents.current_native(owner).is_err());
    documents.confirm(owner, nonce).unwrap();
    let scope = tokio::time::timeout(Duration::from_millis(100), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(
        &scope,
        &documents.current_native(owner).unwrap()
    ));
    assert_eq!(claim.action().method(), "workspace.focus");
    assert!(bank.claim(&id).is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_initial_navigation_never_borrows_a_replacement_document() {
    let (documents, owner, bank) = fixture();
    let action = focus();
    let id = action.request_id().to_owned();
    bank.enqueue_first(action).unwrap();
    let claim = bank.claim(&id).unwrap();
    bank.started().unwrap();
    documents.started(owner).unwrap();
    documents.finished(owner).unwrap();
    bank.finished(&documents, owner).unwrap();
    bank.started().unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    bank.finished(&documents, owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    assert!(claim.original_scope(&documents).await.is_err());
    assert!(bank.enqueue_first(focus()).is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_forwarded_original_document_revokes_on_navigation() {
    let (documents, owner, bank) = fixture();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let action = focus();
    let id = action.request_id().to_owned();
    bank.enqueue_current(action, &documents, owner).unwrap();
    let claim = bank.claim(&id).unwrap();
    documents.started(owner).unwrap();
    let next = documents.finished(owner).unwrap();
    documents.confirm(owner, next).unwrap();
    assert!(claim.original_scope(&documents).await.is_err());
}
#[test]
fn bootstrap_claim_is_once_bounded_and_duplicate_ids_cannot_reenter_after_consumption() {
    let (_, _, bank) = fixture();
    for _ in 0..MAX_CLAIMS {
        let action = focus();
        let encoded = action.encode().unwrap();
        let id = action.request_id().to_owned();
        bank.enqueue_first(action).unwrap();
        assert!(bank.claim(&id).is_ok());
        assert!(bank.claim(&id).is_err());
        assert!(
            bank.enqueue_first(StartupAction::parse(&encoded).unwrap())
                .is_err()
        );
    }
    assert!(bank.enqueue_first(focus()).is_err());
    assert!(bank.claim("unknown").is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn split_and_ssh_parcels_wait_for_original_confirmation_and_revoke_on_navigation() {
    for (method, params) in [
        (
            "workspace.split",
            serde_json::json!({"profile":"sh","direction":"row"}),
        ),
        (
            "workspace.ssh",
            serde_json::json!({"host":"018f5691-0059-7a69-a45a-7c4d6a1cfa51","identity":"018f5691-0059-7a69-a45a-7c4d6a1cfa52"}),
        ),
    ] {
        let (documents, owner, bank) = fixture();
        let action = StartupAction::new(method, &params).unwrap();
        assert!(!action.supports_cold_readonly_transport());
        let id = action.request_id().to_owned();
        bank.enqueue_first(action).unwrap();
        let claim = bank.claim(&id).unwrap();
        assert!(bank.claim(&id).is_err());
        bank.started().unwrap();
        documents.started(owner).unwrap();
        let nonce = documents.finished(owner).unwrap();
        bank.finished(&documents, owner).unwrap();
        let mut pending = Box::pin(claim.original_scope(&documents));
        std::future::poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert!(documents.current_native(owner).is_err());
        documents.confirm(owner, nonce).unwrap();
        let original = pending.await.unwrap();
        assert!(Arc::ptr_eq(
            &original,
            &documents.current_native(owner).unwrap()
        ));
        assert_eq!(claim.action().method(), method);
        assert_eq!(claim.action().params(), params.as_object().unwrap());
        bank.started().unwrap();
        documents.started(owner).unwrap();
        let next = documents.finished(owner).unwrap();
        documents.confirm(owner, next).unwrap();
        assert!(claim.original_scope(&documents).await.is_err());
        assert!(!Arc::ptr_eq(
            &original,
            &documents.current_native(owner).unwrap()
        ));
    }
}
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_close_wakes_unreceived_first_claim_and_cancel_never_confirms() {
    let (documents, _, bank) = fixture();
    let action = focus();
    let id = action.request_id().to_owned();
    bank.enqueue_first(action).unwrap();
    let claim = bank.claim(&id).unwrap();
    let mut waiting = Box::pin(claim.original_scope(&documents));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    bank.close().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .unwrap()
            .is_err()
    );
    assert!(bank.claim(&id).is_err());
    assert!(bank.enqueue_first(focus()).is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_canceled_claim_stays_consumed_and_has_no_automatic_effect() {
    let (documents, owner, bank) = fixture();
    let action = focus();
    let id = action.request_id().to_owned();
    bank.enqueue_first(action).unwrap();
    let claim = bank.claim(&id).unwrap();
    let mut waiting = Box::pin(claim.original_scope(&documents));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    drop(claim);
    assert!(bank.claim(&id).is_err());
    assert!(documents.current_native(owner).is_err());
    bank.started().unwrap();
    documents.started(owner).unwrap();
    documents.finished(owner).unwrap();
    bank.finished(&documents, owner).unwrap();
    assert!(documents.current_native(owner).is_err());
}
#[test]
fn bootstrap_current_capture_rejects_unloaded_loading_foreign_owner_and_retired_first_wait() {
    let (documents, owner, bank) = fixture();
    assert!(bank.enqueue_current(focus(), &documents, owner).is_err());
    documents.started(owner).unwrap();
    assert!(bank.enqueue_current(focus(), &documents, owner).is_err());
    let foreign = pulse_app::registry::SessionRegistry::new();
    assert!(
        bank.enqueue_current(focus(), &documents, foreign.open_window().unwrap())
            .is_err()
    );
    bank.started().unwrap();
    bank.started().unwrap();
    assert!(bank.enqueue_first(focus()).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn profiles_bank_waits_for_original_confirmation_and_claims_only_once() {
    let (documents, owner, bank) = fixture();
    let action = StartupAction::new("workspace.profiles", &serde_json::json!({})).unwrap();
    let id = action.request_id().to_owned();
    bank.enqueue_first(action).unwrap();
    let claim = bank.claim(&id).unwrap();
    bank.started().unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    bank.finished(&documents, owner).unwrap();
    let mut waiting = Box::pin(claim.original_scope(&documents));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(documents.current_native(owner).is_err());
    documents.confirm(owner, nonce).unwrap();
    let original = waiting.await.unwrap();
    assert!(Arc::ptr_eq(
        &original,
        &documents.current_native(owner).unwrap()
    ));
    assert_eq!(claim.action().method(), "workspace.profiles");
    assert!(bank.claim(&id).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn forwarded_profiles_cannot_borrow_a_refreshed_page() {
    let (documents, owner, bank) = fixture();
    documents.started(owner).unwrap();
    let first = documents.finished(owner).unwrap();
    documents.confirm(owner, first).unwrap();
    let action = StartupAction::new("workspace.profiles", &serde_json::json!({})).unwrap();
    let id = action.request_id().to_owned();
    bank.enqueue_current(action, &documents, owner).unwrap();
    let claim = bank.claim(&id).unwrap();
    documents.started(owner).unwrap();
    let next = documents.finished(owner).unwrap();
    documents.confirm(owner, next).unwrap();
    assert!(claim.original_scope(&documents).await.is_err());
    assert!(bank.claim(&id).is_err());
    assert!(documents.current_native(owner).is_ok());
}

#[test]
fn unsupported_cold_parcels_cannot_consume_bank_capacity() {
    let (documents, owner, bank) = fixture();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    for (method, params) in [
        ("workspace.new-window", serde_json::json!({})),
        ("workspace.windows", serde_json::json!({})),
        ("workspace.focus-tab", serde_json::json!({"session":"1"})),
        ("workspace.close", serde_json::json!({})),
        (
            "workspace.focus",
            serde_json::json!({"window":"workspace.other"}),
        ),
        (
            "workspace.ssh",
            serde_json::json!({"address":"localhost","username":"probe"}),
        ),
    ] {
        assert!(StartupAction::new(method, &params).is_err());
    }
    assert!(bank.entries.lock().unwrap().is_empty());
    bank.enqueue_first(focus()).unwrap();
    bank.enqueue_current(
        StartupAction::new("workspace.profiles", &serde_json::json!({})).unwrap(),
        &documents,
        owner,
    )
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn new_tab_claim_waits_for_original_page_and_never_grants_startup() {
    let (documents, owner, bank) = fixture();
    let action =
        StartupAction::new("workspace.new-tab", &serde_json::json!({"profile":"sh"})).unwrap();
    let id = action.request_id().to_owned();
    bank.enqueue_first(action).unwrap();
    let claim = bank.claim(&id).unwrap();
    assert!(bank.claim(&id).is_err());
    bank.started().unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    bank.finished(&documents, owner).unwrap();
    let mut pending = Box::pin(claim.original_scope(&documents));
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(documents.current_native(owner).is_err());
    documents.confirm(owner, nonce).unwrap();
    let original = pending.await.unwrap();
    assert!(Arc::ptr_eq(
        &original,
        &documents.current_native(owner).unwrap()
    ));
    assert_eq!(claim.action().method(), "workspace.new-tab");
    assert_eq!(
        claim.action().params().get("profile"),
        Some(&serde_json::json!("sh"))
    );
    // Capturing a scope is metadata, not Profile/native startup admission.
    bank.started().unwrap();
    documents.started(owner).unwrap();
    let next = documents.finished(owner).unwrap();
    documents.confirm(owner, next).unwrap();
    assert!(claim.original_scope(&documents).await.is_err());
    assert!(!Arc::ptr_eq(
        &original,
        &documents.current_native(owner).unwrap()
    ));
}
