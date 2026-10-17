#![allow(clippy::unwrap_used)]
use super::{
    tests::{config, session},
    *,
};
use crate::product_document::ProductDocuments;
use pulse_term::session::Lifecycle;
use tokio::time::{Duration, timeout};

fn ready(documents: &ProductDocuments, owner: WindowToken) -> Arc<PageLaunchScope> {
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    documents.current(owner, nonce).unwrap()
}
pub(super) async fn attach(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    id: SessionId,
    dual: bool,
) -> Result<RegisteredAttachment, RegistryError> {
    if dual {
        registry.attach_page_v2(page, id, config()).await
    } else {
        registry.attach_page(page, id, config()).await
    }
}
pub(super) async fn bootstrap(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    id: SessionId,
    binding: &mut RegisteredAttachment,
    dual: bool,
) {
    loop {
        let wire = timeout(Duration::from_secs(1), binding.wire.recv())
            .await
            .unwrap()
            .unwrap();
        let frame = pulse_ipc::decode(&wire).unwrap();
        let part = if dual {
            pulse_ipc::attach_v2::decode_fragment(frame.payload)
                .unwrap()
                .part
        } else {
            pulse_ipc::attach::decode_fragment(frame.payload)
                .unwrap()
                .part
        };
        registry
            .acknowledge_page(page, id, binding.generation, frame.sequence)
            .unwrap();
        if part == pulse_ipc::attach::Part::Commit {
            break;
        }
    }
}

#[tokio::test]
async fn stale_foreign_and_other_window_scopes_do_not_spend_generation_or_replace_view() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let docs = ProductDocuments::new(r.clone());
        docs.install(owner).unwrap();
        let old = ready(&docs, owner);
        let current = ready(&docs, owner);
        let (h, task, driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut binding = attach(&r, &current, id, dual).await.unwrap();
        bootstrap(&r, &current, id, &mut binding, dual).await;
        let foreign = SessionRegistry::new();
        let foreign_owner = foreign.open_window().unwrap();
        let foreign_page = foreign.activate_launch_page(foreign_owner).unwrap();
        let other = r.open_window().unwrap();
        let other_page = r.activate_launch_page(other).unwrap();
        let generation = r.lock().unwrap().next_generation;
        for page in [old.as_ref(), &foreign_page, &other_page] {
            assert!(matches!(
                attach(&r, page, id, dual).await,
                Err(RegistryError::Forbidden)
            ));
            assert_eq!(r.lock().unwrap().next_generation, generation);
            assert!(r.check_binding(owner, id, binding.generation).is_ok());
        }
        r.detach(owner, id, binding.generation).unwrap();
        let _ = timeout(Duration::from_secs(5), binding.completion)
            .await
            .unwrap();
        h.close();
        drop(driver);
        task.join().await.unwrap();
        r.reap_closed(id).unwrap();
        r.shutdown().unwrap();
        foreign.shutdown().unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn captured_old_document_cannot_install_after_refresh_and_new_page_can_recover() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let docs = ProductDocuments::new(r.clone());
        docs.install(owner).unwrap();
        let old = ready(&docs, owner);
        let (h, task, driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut pending = Box::pin(attach(&r, &old, id, dual));
        // Actor task cannot run inside this first current-thread poll. Its capture
        // request is enqueued before the metadata transaction is allowed to continue.
        tokio::select! { biased; _ = &mut pending => panic!("capture unexpectedly completed"), _ = std::future::ready(()) => {} }
        assert_eq!(r.lock().unwrap().next_generation, 2);
        let current = ready(&docs, owner);
        assert!(matches!(
            timeout(Duration::from_secs(5), &mut pending).await.unwrap(),
            Err(RegistryError::Forbidden)
        ));
        drop(pending);
        assert!(r.lock().unwrap().bindings.is_empty());
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        let mut binding = attach(&r, &current, id, dual).await.unwrap();
        bootstrap(&r, &current, id, &mut binding, dual).await;
        assert!(r.check_binding(owner, id, binding.generation).is_ok());
        r.detach(owner, id, binding.generation).unwrap();
        let _ = timeout(Duration::from_secs(5), binding.completion)
            .await
            .unwrap();
        h.close();
        drop(driver);
        task.join().await.unwrap();
        r.reap_closed(id).unwrap();
        r.shutdown().unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn closed_window_during_capture_rejects_view_without_closing_session() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut pending = Box::pin(attach(&r, &page, id, dual));
        tokio::select! { biased; _ = &mut pending => panic!("capture unexpectedly completed"), _ = std::future::ready(()) => {} }
        r.close_window(owner).unwrap();
        assert!(matches!(
            timeout(Duration::from_secs(5), &mut pending).await.unwrap(),
            Err(RegistryError::Forbidden)
        ));
        drop(pending);
        assert!(r.lock().unwrap().bindings.is_empty());
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        h.close();
        drop(driver);
        task.join().await.unwrap();
        r.reap_closed(id).unwrap();
        r.shutdown().unwrap();
    }
}

#[tokio::test]
async fn same_page_rebind_rejects_old_ack_and_old_supervisor_cannot_remove_new_view() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let first = attach(&r, &page, id, dual).await.unwrap();
        let mut current = attach(&r, &page, id, dual).await.unwrap();
        assert!(matches!(
            r.acknowledge(owner, id, first.generation, 1),
            Err(RegistryError::Stale)
        ));
        let _ = timeout(Duration::from_secs(5), first.completion)
            .await
            .unwrap();
        bootstrap(&r, &page, id, &mut current, dual).await;
        assert!(r.check_binding(owner, id, current.generation).is_ok());
        r.detach(owner, id, current.generation).unwrap();
        let _ = timeout(Duration::from_secs(5), current.completion)
            .await
            .unwrap();
        h.close();
        drop(driver);
        task.join().await.unwrap();
        r.reap_closed(id).unwrap();
        r.shutdown().unwrap();
    }
}
