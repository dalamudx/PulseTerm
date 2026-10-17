//! Native-event model tests; no claim of a real Tauri page or product UI.
#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn native_document_tuple_requires_ready_and_never_mixes_new_nonce_with_old_scope() {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().unwrap();
    documents.install(owner).unwrap();
    assert!(documents.current_native_document(owner).is_err());
    documents.started(owner).unwrap();
    assert!(documents.current_native_document(owner).is_err());
    let nonce = documents.finished(owner).unwrap();
    assert!(documents.current_native_document(owner).is_err());
    documents.confirm(owner, nonce).unwrap();
    let (actual, old) = documents.current_native_document(owner).unwrap();
    assert_eq!(actual, nonce);
    assert!(Arc::ptr_eq(&old, &documents.current_native(owner).unwrap()));
    let other = registry.open_window().unwrap();
    assert!(documents.current_native_document(other).is_err());
    documents.started(owner).unwrap();
    assert!(documents.current_native_document(owner).is_err());
    assert!(registry.check_launch_page(&old).is_err());
    let new_nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, new_nonce).unwrap();
    let (actual, fresh) = documents.current_native_document(owner).unwrap();
    assert_eq!(actual, new_nonce);
    assert!(!Arc::ptr_eq(&old, &fresh));
    documents.remove(owner).unwrap();
    assert!(documents.current_native_document(owner).is_err());
}
#[test]
fn repeated_refresh_revokes_old_scopes_and_pending_launches() {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().unwrap();
    documents.install(owner).unwrap();
    let mut previous: Option<(
        Uuid,
        Arc<PageLaunchScope>,
        crate::registry::LocalLaunchTicket,
    )> = None;
    for _ in 0..96 {
        documents.started(owner).unwrap();
        if let Some((nonce, scope, ticket)) = previous.take() {
            assert!(matches!(
                documents.current(owner, nonce),
                Err(RegistryError::Forbidden)
            ));
            assert!(matches!(
                registry.check_launch_page(&scope),
                Err(RegistryError::Forbidden)
            ));
            assert!(matches!(
                ticket.require_current(),
                Err(RegistryError::Forbidden)
            ));
        }
        let nonce = documents.finished(owner).unwrap();
        assert_eq!(nonce.get_version(), Some(uuid::Version::Random));
        assert!(matches!(
            documents.current(owner, nonce),
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            documents.confirm(owner, Uuid::new_v4()),
            Err(RegistryError::Forbidden)
        ));
        documents.confirm(owner, nonce).unwrap();
        let scope = documents.current(owner, nonce).unwrap();
        let ticket = registry.begin_page_local_launch(&scope).unwrap();
        previous = Some((nonce, scope, ticket));
    }
    documents.remove(owner).unwrap();
    if let Some((nonce, scope, ticket)) = previous {
        assert!(matches!(
            documents.current(owner, nonce),
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            registry.check_launch_page(&scope),
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            ticket.require_current(),
            Err(RegistryError::Forbidden)
        ));
    }
}

#[test]
fn foreign_owner_cross_window_and_malformed_order_fail_closed() {
    let registry = SessionRegistry::new();
    let foreign = SessionRegistry::new();
    let controller = ProductDocuments::new(registry.clone());
    let first = registry.open_window().unwrap();
    let second = registry.open_window().unwrap();
    let third = registry.open_window().unwrap();
    let foreign_owner = foreign.open_window().unwrap();
    assert!(matches!(
        controller.install(foreign_owner),
        Err(RegistryError::Forbidden)
    ));
    controller.install(first).unwrap();
    controller.install(second).unwrap();
    controller.install(third).unwrap();
    assert!(matches!(
        controller.install(first),
        Err(RegistryError::Forbidden)
    ));
    controller.started(first).unwrap();
    let nonce = controller.finished(first).unwrap();
    controller.confirm(first, nonce).unwrap();
    assert!(matches!(
        controller.current(second, nonce),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        controller.confirm(first, nonce),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        controller.finished(first),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        controller.current(first, nonce),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        controller.started(first),
        Err(RegistryError::Forbidden)
    ));
    controller.started(second).unwrap();
    assert!(matches!(
        controller.started(second),
        Err(RegistryError::Forbidden)
    ));
    controller.started(third).unwrap();
    let third_nonce = controller.finished(third).unwrap();
    assert_ne!(third_nonce, Uuid::nil());
    assert!(matches!(
        controller.confirm(third, Uuid::nil()),
        Err(RegistryError::Forbidden)
    ));
    controller.confirm(third, third_nonce).unwrap();
    controller.remove(first).unwrap();
    controller.remove(second).unwrap();
    controller.remove(third).unwrap();
}

#[test]
fn closing_one_document_does_not_revoke_another_and_window_count_is_bounded() {
    let registry = SessionRegistry::new();
    let controller = ProductDocuments::new(registry.clone());
    let mut owners = Vec::new();
    for _ in 0..32 {
        let owner = registry.open_window().unwrap();
        controller.install(owner).unwrap();
        owners.push(owner);
    }
    assert!(matches!(registry.open_window(), Err(RegistryError::Busy)));
    let live = owners[1];
    controller.started(live).unwrap();
    let nonce = controller.finished(live).unwrap();
    controller.confirm(live, nonce).unwrap();
    let scope = controller.current(live, nonce).unwrap();
    controller.remove(owners[0]).unwrap();
    registry.close_window(owners[0]).unwrap();
    registry.check_launch_page(&scope).unwrap();
    let extra = registry.open_window().unwrap();
    controller.install(extra).unwrap();
    for owner in owners.into_iter().skip(1) {
        controller.remove(owner).unwrap();
    }
    controller.remove(extra).unwrap();
    assert!(matches!(
        registry.check_launch_page(&scope),
        Err(RegistryError::Forbidden)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn native_navigation_keeps_the_registered_actor_alive() {
    use pulse_core::TermSize;
    use pulse_term::session::{self, ChannelTransport, Lifecycle};
    use tokio::time::{Duration, timeout};
    let registry = SessionRegistry::new();
    let controller = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().unwrap();
    controller.install(owner).unwrap();
    controller.started(owner).unwrap();
    let first = controller.finished(owner).unwrap();
    controller.confirm(owner, first).unwrap();
    let old_scope = controller.current(owner, first).unwrap();
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = session::spawn_session(TermSize::new(32, 8).unwrap(), 64, transport);
    let id = registry.register(owner, handle.clone()).unwrap();
    controller.started(owner).unwrap();
    assert!(matches!(
        registry.check_launch_page(&old_scope),
        Err(RegistryError::Forbidden)
    ));
    assert_eq!(
        registry.state(owner, id).unwrap().lifecycle,
        Lifecycle::Running
    );
    let next = controller.finished(owner).unwrap();
    controller.confirm(owner, next).unwrap();
    controller.current(owner, next).unwrap();
    controller.remove(owner).unwrap();
    assert_eq!(
        registry.state(owner, id).unwrap().lifecycle,
        Lifecycle::Running
    );
    handle.close();
    drop(driver);
    timeout(Duration::from_secs(2), task.join())
        .await
        .unwrap()
        .unwrap();
    registry.reap_closed(id).unwrap();
}

#[test]
fn trusted_native_current_scope_requires_same_live_confirmed_window_generation() {
    let registry = SessionRegistry::new();
    let foreign = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().unwrap();
    let other = registry.open_window().unwrap();
    let foreign_owner = foreign.open_window().unwrap();
    documents.install(owner).unwrap();
    documents.install(other).unwrap();
    assert!(matches!(
        documents.current_native(owner),
        Err(RegistryError::Forbidden)
    ));
    documents.started(owner).unwrap();
    let old_nonce = documents.finished(owner).unwrap();
    assert!(matches!(
        documents.current_native(owner),
        Err(RegistryError::Forbidden)
    ));
    documents.confirm(owner, old_nonce).unwrap();
    let old = documents.current_native(owner).unwrap();
    assert!(Arc::ptr_eq(
        &old,
        &documents.current(owner, old_nonce).unwrap()
    ));
    assert!(matches!(
        documents.current_native(other),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        documents.current_native(foreign_owner),
        Err(RegistryError::Forbidden)
    ));
    documents.started(owner).unwrap();
    assert!(matches!(
        documents.current_native(owner),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        registry.check_launch_page(&old),
        Err(RegistryError::Forbidden)
    ));
    let next_nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, next_nonce).unwrap();
    let next = documents.current_native(owner).unwrap();
    assert!(!Arc::ptr_eq(&old, &next));
    documents.remove(owner).unwrap();
    assert!(matches!(
        documents.current_native(owner),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        registry.check_launch_page(&next),
        Err(RegistryError::Forbidden)
    ));
}
