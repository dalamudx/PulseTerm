#[cfg(feature = "workspace-ssh")]
#[tokio::test]
async fn saved_alias_exact_ids_require_target_confirmation_and_current_original_page() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let documents = ProductDocuments::new(registry.clone());
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let page = documents.current(owner, nonce).unwrap();
    let catalogue = super::super::capabilities::catalogue(true).unwrap();
    let metadata = PreparedWindowAction::prepare(
        &catalogue,
        &registry,
        page.clone(),
        "workspace.ssh",
        Surface::Cli,
        serde_json::json!({"alias":"GUI_HOST"}),
    )
    .unwrap();
    assert!(metadata.take_args(&page, "workspace.ssh").is_err());
    let host = Uuid::now_v7();
    let identity = Uuid::now_v7();
    let exact =
        super::super::ssh::resolved_alias_action(&registry, page.clone(), host, identity).unwrap();
    assert!(exact.take_args(&page, "workspace.ssh").is_err());
    let bank = Arc::new(Approvals::default());
    let exact =
        super::super::ssh::resolved_alias_action(&registry, page.clone(), host, identity).unwrap();
    let mut slot = bank
        .reserve(page.clone(), CancellationToken::new(), exact)
        .unwrap();
    bank.answer(&page, slot.id, true).unwrap();
    let action = (&mut slot.answer).await.unwrap().unwrap();
    let args = action.take_args(&page, "workspace.ssh").unwrap();
    assert_eq!(
        args,
        serde_json::json!({"host":host.to_string(),"identity":identity.to_string()})
    );
    documents.started(owner).unwrap();
    assert!(super::super::ssh::resolved_alias_action(&registry, page, host, identity).is_err());
}

use super::*;
use pulse_app::{product_document::ProductDocuments, registry::SessionRegistry};
use pulse_cap::Surface;
fn fixture() -> (
    ProductDocuments,
    pulse_app::registry::WindowToken,
    Arc<PageLaunchScope>,
    PreparedWindowAction,
) {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let documents = ProductDocuments::new(registry.clone());
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let page = documents.current(owner, nonce).unwrap();
    let catalogue = super::super::capabilities::catalogue(false).unwrap();
    let action = PreparedWindowAction::prepare(
        &catalogue,
        &registry,
        page.clone(),
        "workspace.new-tab",
        Surface::Cli,
        serde_json::json!({"profile":"sh"}),
    )
    .unwrap();
    (documents, owner, page, action)
}
#[test]
fn original_gui_confirmation_is_single_use_and_keeps_exact_profile() {
    let (_docs, _owner, page, action) = fixture();
    let bank = Arc::new(Approvals::default());
    let mut pending = bank
        .reserve(page.clone(), CancellationToken::new(), action)
        .unwrap();
    bank.answer(&page, pending.id, true).unwrap();
    assert!(bank.answer(&page, pending.id, true).is_err());
    let action = pending.answer.try_recv().unwrap().unwrap();
    assert_eq!(
        action.take_args(&page, "workspace.new-tab").unwrap(),
        serde_json::json!({"profile":"sh"})
    );
}
#[test]
fn cancellation_rejection_and_navigation_never_return_confirmed_action() {
    for mode in 0..4 {
        let (docs, owner, page, action) = fixture();
        let bank = Arc::new(Approvals::default());
        let stop = CancellationToken::new();
        let mut pending = bank.reserve(page.clone(), stop.clone(), action).unwrap();
        match mode {
            0 => {
                stop.cancel();
                assert!(bank.answer(&page, pending.id, true).is_err());
            }
            1 => {
                bank.answer(&page, pending.id, false).unwrap();
            }
            2 => {
                docs.started(owner).unwrap();
                assert!(bank.answer(&page, pending.id, true).is_ok());
            }
            _ => {
                bank.retire();
                assert!(bank.answer(&page, pending.id, true).is_err());
            }
        }
        assert!(!matches!(pending.answer.try_recv(), Ok(Ok(_))));
    }
}
#[test]
fn foreign_wrong_ticket_and_drop_cannot_confirm_or_remove_newer_reservation() {
    let (_docs, _owner, page, action) = fixture();
    let bank = Arc::new(Approvals::default());
    let pending = bank
        .reserve(page.clone(), CancellationToken::new(), action)
        .unwrap();
    let (_, _, foreign, _) = fixture();
    assert!(bank.answer(&foreign, pending.id, true).is_err());
    assert!(bank.answer(&page, Uuid::new_v4(), true).is_err());
    bank.retire();
    let (_, _, next_page, next_action) = fixture();
    let mut next = bank
        .reserve(next_page.clone(), CancellationToken::new(), next_action)
        .unwrap();
    drop(pending);
    bank.answer(&next_page, next.id, true).unwrap();
    assert!(next.answer.try_recv().unwrap().is_ok());
}

#[test]
fn finite_deadline_and_busy_slot_do_not_return_action() {
    let (_docs, _owner, page, action) = fixture();
    let bank = Arc::new(Approvals::default());
    let mut pending = bank
        .reserve(page.clone(), CancellationToken::new(), action)
        .unwrap();
    let (_, _, foreign, next) = fixture();
    assert!(
        bank.reserve(foreign, CancellationToken::new(), next)
            .is_err()
    );
    bank.0.lock().unwrap().as_mut().unwrap().until = Instant::now() - Duration::from_secs(1);
    assert!(bank.answer(&page, pending.id, true).is_err());
    assert!(!matches!(pending.answer.try_recv(), Ok(Ok(_))));
}

#[test]
fn split_confirmation_cannot_be_reused_as_new_tab_or_change_direction() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let documents = ProductDocuments::new(registry.clone());
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let page = documents.current(owner, nonce).unwrap();
    let catalogue = super::super::capabilities::catalogue(false).unwrap();
    let args = serde_json::json!({"profile":"sh","direction":"column"});
    let action = PreparedWindowAction::prepare(
        &catalogue,
        &registry,
        page.clone(),
        "workspace.split",
        Surface::Cli,
        args.clone(),
    )
    .unwrap();
    let bank = Arc::new(Approvals::default());
    let mut pending = bank
        .reserve(page.clone(), CancellationToken::new(), action)
        .unwrap();
    bank.answer(&page, pending.id, true).unwrap();
    let approved = pending.answer.try_recv().unwrap().unwrap();
    assert!(approved.take_args(&page, "workspace.new-tab").is_err());
    let action = PreparedWindowAction::prepare(
        &catalogue,
        &registry,
        page.clone(),
        "workspace.split",
        Surface::Cli,
        args.clone(),
    )
    .unwrap();
    let mut pending = bank
        .reserve(page.clone(), CancellationToken::new(), action)
        .unwrap();
    bank.answer(&page, pending.id, true).unwrap();
    assert_eq!(
        pending
            .answer
            .try_recv()
            .unwrap()
            .unwrap()
            .take_args(&page, "workspace.split")
            .unwrap(),
        args
    );
}
