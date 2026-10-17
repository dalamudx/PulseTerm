use super::super::{ProductDocuments, confirm_current_document, current_document};
use super::*;
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn quick_target_book_replacement_cancel_navigation_and_unregistered_save_fail_closed() {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().unwrap();
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    let local = tauri::Url::parse("tauri://localhost/workspace.html").unwrap();
    confirm_current_document(&documents, owner, &local, &nonce.to_string()).unwrap();
    let (_, page) = current_document(&documents, owner, &local, &nonce.to_string()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let ready = pulse_store::state_directory::prepare(directory.path()).unwrap();
    let (store, task) = pulse_store::StateStore::open(ready.join("state.db"))
        .await
        .unwrap();
    let book = QuickTargets::new();
    let endpoint =
        pulse_core::ssh_endpoint::SshEndpoint::new("临时主机", "127.0.0.1", 22, "user").unwrap();
    let first = store.temporary_ssh_target(endpoint.clone(), Arc::new(|| true));
    let first_id = first.id();
    let source = store
        .load_temporary_ssh_target_source(first.clone(), Arc::new(|| true))
        .await
        .unwrap();
    let first_live = source.target_liveness();
    let token = book.publish(&registry, page.clone(), first).unwrap();
    assert!(book.claim_save(&registry, &page, token).is_err());
    assert!(book.cancel(&registry, &page, Uuid::new_v4()).is_err());
    assert!(book.resolve(&registry, &page, first_id).unwrap().is_some());
    let second = store.temporary_ssh_target(endpoint.clone(), Arc::new(|| true));
    let second_id = second.id();
    let next = book
        .publish(&registry, page.clone(), second.clone())
        .unwrap();
    assert!(!first_live());
    assert!(book.cancel(&registry, &page, token).is_err());
    book.cancel(&registry, &page, next).unwrap();
    assert!(!second.is_current());
    assert!(book.resolve(&registry, &page, second_id).unwrap().is_none());
    let third = store.temporary_ssh_target(endpoint, Arc::new(|| true));
    let third_id = third.id();
    let third_token = book
        .publish(&registry, page.clone(), third.clone())
        .unwrap();
    documents.started(owner).unwrap();
    let next_nonce = documents.finished(owner).unwrap();
    confirm_current_document(&documents, owner, &local, &next_nonce.to_string()).unwrap();
    let (_, new_page) =
        current_document(&documents, owner, &local, &next_nonce.to_string()).unwrap();
    assert!(book.resolve(&registry, &page, third_id).is_err());
    assert!(book.resolve(&registry, &new_page, third_id).is_err());
    assert!(book.claim_save(&registry, &new_page, third_token).is_err());
    book.retire();
    assert!(!third.is_current());
    assert!(store.load_ssh_hosts().await.unwrap().records().is_empty());
    drop(source);
    drop(store);
    task.shutdown().await.unwrap();
}
