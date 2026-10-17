#![allow(clippy::unwrap_used)]
use super::*;
use crate::registry::{
    page_attach_tests::{attach, bootstrap},
    tests::{config, session},
};
use pulse_term::session::{Lifecycle, TransportWrite};
#[tokio::test(flavor = "current_thread")]
async fn capture_allowance_waits_authoritative_deadline_without_relaxing_interval() {
    let r = SessionRegistry::new();
    let owner = r.open_window().unwrap();
    let page = r.activate_launch_page(owner).unwrap();
    let (h, task, _driver) = session();
    let id = r.register(owner, h.clone()).unwrap();
    let mut first = attach(&r, &page, id, true).await.unwrap();
    bootstrap(&r, &page, id, &mut first, true).await;
    let d = h.diagnostic_digest().await.unwrap();
    let rendered = RenderedScreen {
        boundary: d.boundary,
        digest: d.digest,
    };
    let at = tokio::time::Instant::now();
    assert_eq!(
        r.compare_bootstrap_page(&page, id, first.generation, rendered.clone())
            .unwrap(),
        VisibleComparison::Equal
    );
    let mut fresh = attach(&r, &page, id, true).await.unwrap();
    bootstrap(&r, &page, id, &mut fresh, true).await;
    assert!(matches!(
        r.compare_bootstrap_page(&page, id, fresh.generation, rendered.clone()),
        Err(RegistryError::Busy)
    ));
    let observed = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(
        r.compare_bootstrap_page_when_ready_observed(
            &page,
            id,
            fresh.generation,
            rendered.clone(),
            || {
                observed.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        )
        .await
        .unwrap(),
        VisibleComparison::Equal
    );
    assert!(observed.load(std::sync::atomic::Ordering::SeqCst));
    assert!(at.elapsed() >= Duration::from_millis(500));
    assert!(matches!(
        r.compare_bootstrap_page(&page, id, fresh.generation, rendered),
        Err(RegistryError::Busy)
    ));
    r.shutdown().unwrap();
    task.join().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn capture_allowance_retired_waiter_cannot_publish_or_consume_new_page_allowance() {
    let r = SessionRegistry::new();
    let owner = r.open_window().unwrap();
    let old = r.activate_launch_page(owner).unwrap();
    let (h, task, _driver) = session();
    let id = r.register(owner, h.clone()).unwrap();
    let mut first = attach(&r, &old, id, true).await.unwrap();
    bootstrap(&r, &old, id, &mut first, true).await;
    let d = h.diagnostic_digest().await.unwrap();
    let rendered = RenderedScreen {
        boundary: d.boundary,
        digest: d.digest,
    };
    r.compare_bootstrap_page(&old, id, first.generation, rendered.clone())
        .unwrap();
    let mut waiting =
        Box::pin(r.compare_bootstrap_page_when_ready(&old, id, first.generation, rendered.clone()));
    tokio::select! {biased;_=&mut waiting=>panic!("bypassed quota"),_=std::future::ready(())=>{}}
    let current = r.activate_launch_page(owner).unwrap();
    let mut fresh = attach(&r, &current, id, true).await.unwrap();
    bootstrap(&r, &current, id, &mut fresh, true).await;
    tokio::select! { biased; result=&mut waiting => assert!(result.is_err()), _=std::future::ready(()) => panic!("retired clockwait did not exit immediately") }
    drop(waiting);
    assert!(matches!(
        r.compare_bootstrap_page(&current, id, fresh.generation, rendered.clone()),
        Err(RegistryError::Busy)
    ));
    assert_eq!(
        r.compare_bootstrap_page_when_ready(&current, id, fresh.generation, rendered)
            .await
            .unwrap(),
        VisibleComparison::Equal
    );
    r.shutdown().unwrap();
    task.join().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn page_resize_waits_for_native_ack_then_recaptures_new_dimensions() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, mut driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut bound = attach(&r, &page, id, dual).await.unwrap();
        bootstrap(&r, &page, id, &mut bound, dual).await;
        let size = TermSize::new(48, 10).unwrap();
        let old_size = h.state.borrow().size;
        let mut pending = Box::pin(r.resize_page_bound(&page, id, bound.generation, size));
        tokio::select! {biased;_=&mut pending=>panic!("no native ACK"),_=std::future::ready(())=>{}}
        let native = tokio::select! {biased;r=&mut pending=>panic!("early result: {r:?}"),native=driver.input.recv()=>native};
        let Some(TransportWrite::Resize(actual, ack)) = native else {
            panic!("missing resize")
        };
        assert_eq!(actual, size);
        assert_eq!(h.state.borrow().size, old_size);
        ack.send(Ok(())).unwrap();
        timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(h.state.borrow().size, size);
        assert!(
            r.input_page_bound(&page, id, bound.generation, b"old".to_vec())
                .await
                .is_err()
        );
        let mut fresh = attach(&r, &page, id, dual).await.unwrap();
        assert_ne!(fresh.generation, bound.generation);
        bootstrap(&r, &page, id, &mut fresh, dual).await;
        assert_eq!(h.state.borrow().size, size);
        r.shutdown().unwrap();
        drop(driver);
        task.join().await.unwrap();
    }
}
#[tokio::test(flavor = "current_thread")]
async fn page_resize_navigation_after_native_admission_never_promotes_old_scope() {
    let r = SessionRegistry::new();
    let owner = r.open_window().unwrap();
    let old = r.activate_launch_page(owner).unwrap();
    let (h, task, mut driver) = session();
    let id = r.register(owner, h.clone()).unwrap();
    let mut bound = attach(&r, &old, id, true).await.unwrap();
    bootstrap(&r, &old, id, &mut bound, true).await;
    let size = TermSize::new(48, 10).unwrap();
    let mut pending = Box::pin(r.resize_page_bound(&old, id, bound.generation, size));
    let native = tokio::select! {biased;r=&mut pending=>panic!("early result: {r:?}"),native=driver.input.recv()=>native};
    let Some(TransportWrite::Resize(_, ack)) = native else {
        panic!("missing resize")
    };
    let current = r.activate_launch_page(owner).unwrap();
    ack.send(Ok(())).unwrap();
    assert!(pending.await.is_err());
    assert_eq!(h.state.borrow().size, size);
    assert!(
        r.resize_page_bound(&old, id, bound.generation, TermSize::new(60, 12).unwrap())
            .await
            .is_err()
    );
    let mut fresh = attach(&r, &current, id, true).await.unwrap();
    bootstrap(&r, &current, id, &mut fresh, true).await;
    assert_eq!(h.state.borrow().size, size);
    r.shutdown().unwrap();
    drop(driver);
    task.join().await.unwrap();
}

#[tokio::test]
async fn explicit_page_close_rejects_retired_and_foreign_page_without_closing_root_session() {
    let r = SessionRegistry::new();
    let owner = r.open_window().unwrap();
    let old = r.activate_launch_page(owner).unwrap();
    let (h, task, _driver) = session();
    let id = r.register(owner, h.clone()).unwrap();
    let current = r.activate_launch_page(owner).unwrap();
    let foreign = r.activate_launch_page(r.open_window().unwrap()).unwrap();
    assert!(r.request_close_page_session(&old, id).is_err());
    assert!(r.request_close_page_session(&foreign, id).is_err());
    assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
    r.request_close_page_session(&current, id).unwrap();
    task.join().await.unwrap();
    r.reap_closed(id).unwrap();
    r.shutdown().unwrap();
}

#[tokio::test]
async fn page_capture_confirmation_rejects_foreign_stale_and_legacy_without_consuming_quota() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, _driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let captured = h.diagnostic_digest().await.unwrap();
        let rendered = RenderedScreen {
            boundary: captured.boundary,
            digest: captured.digest,
        };
        let mut bound = attach(&r, &page, id, dual).await.unwrap();
        bootstrap(&r, &page, id, &mut bound, dual).await;
        let foreign = r.activate_launch_page(r.open_window().unwrap()).unwrap();
        assert!(
            r.compare_bootstrap_page(&foreign, id, bound.generation, rendered.clone())
                .is_err()
        );
        assert_eq!(
            r.compare_bootstrap_page(&page, id, bound.generation, rendered.clone())
                .unwrap(),
            VisibleComparison::Equal
        );
        let current = r.activate_launch_page(owner).unwrap();
        assert!(
            r.compare_bootstrap_page(&page, id, bound.generation, rendered.clone())
                .is_err()
        );
        assert!(
            r.compare_bootstrap_page(&current, id, bound.generation, rendered.clone())
                .is_err()
        );
        let legacy = r.attach_v2(owner, id, config()).await.unwrap();
        assert!(
            r.compare_bootstrap_page(&current, id, legacy.generation, rendered)
                .is_err()
        );
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        r.shutdown().unwrap();
        task.join().await.unwrap();
    }
}

use tokio::time::{Duration, timeout};
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_receipt_waits_for_validated_commit_not_ack_admission_and_revokes_on_detach() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, _driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut bound = attach(&r, &page, id, dual).await.unwrap();
        let mut waiting = Box::pin(r.wait_page_bootstrap(&page, id, bound.generation));
        tokio::select! { biased; _=&mut waiting=>panic!("ready before ACK"), _=std::future::ready(())=>{} }
        loop {
            let bytes = bound.wire.recv().await.unwrap();
            let frame = pulse_ipc::decode(&bytes).unwrap();
            let part = if dual {
                pulse_ipc::attach_v2::decode_fragment(frame.payload)
                    .unwrap()
                    .part
            } else {
                pulse_ipc::attach::decode_fragment(frame.payload)
                    .unwrap()
                    .part
            };
            r.acknowledge_page(&page, id, bound.generation, frame.sequence)
                .unwrap();
            if part == pulse_ipc::attach::Part::Commit {
                tokio::select! { biased; _=&mut waiting=>panic!("ACK enqueue is not validation"), _=std::future::ready(())=>{} }
                break;
            }
        }
        timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        r.detach_page(&page, id, bound.generation).unwrap();
        assert!(
            r.wait_page_bootstrap(&page, id, bound.generation)
                .await
                .is_err()
        );
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        r.shutdown().unwrap();
        task.join().await.unwrap();
    }
}
#[tokio::test]
async fn bootstrap_receipt_invalid_ack_or_retirement_never_enables_input() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, _driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut bound = attach(&r, &page, id, dual).await.unwrap();
        let _begin = bound.wire.recv().await.unwrap();
        r.acknowledge_page(&page, id, bound.generation, u32::MAX)
            .unwrap();
        assert!(
            timeout(
                Duration::from_secs(2),
                r.wait_page_bootstrap(&page, id, bound.generation)
            )
            .await
            .unwrap()
            .is_err()
        );
        let current = r.activate_launch_page(owner).unwrap();
        assert!(
            r.wait_page_bootstrap(&current, id, bound.generation)
                .await
                .is_err()
        );
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        r.shutdown().unwrap();
        task.join().await.unwrap();
    }
}

#[tokio::test]
async fn scope_drop_detaches_only_its_page_without_closing_session() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut bound = attach(&r, &page, id, dual).await.unwrap();
        bootstrap(&r, &page, id, &mut bound, dual).await;
        let generation = bound.generation;
        drop(page);
        assert!(r.lock().unwrap().bindings.is_empty());
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        assert!(matches!(
            r.acknowledge(owner, id, generation, 1),
            Err(RegistryError::Stale)
        ));
        let _ = timeout(Duration::from_secs(5), bound.completion)
            .await
            .unwrap();
        let current = r.activate_launch_page(owner).unwrap();
        let mut bound = attach(&r, &current, id, dual).await.unwrap();
        bootstrap(&r, &current, id, &mut bound, dual).await;
        r.detach(owner, id, bound.generation).unwrap();
        let _ = timeout(Duration::from_secs(5), bound.completion)
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
async fn queued_old_page_input_is_rejected_before_native_write_and_new_page_input_survives() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, mut driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut old = attach(&r, &page, id, dual).await.unwrap();
        bootstrap(&r, &page, id, &mut old, dual).await;
        let mut input = Box::pin(r.input_page_bound(&page, id, old.generation, b"old".to_vec()));
        tokio::select! { biased; _ = &mut input => panic!("input unexpectedly admitted"), _ = std::future::ready(()) => {} }
        let current = r.activate_launch_page(owner).unwrap();
        assert!(
            timeout(Duration::from_secs(5), &mut input)
                .await
                .unwrap()
                .is_err()
        );
        drop(input);
        assert!(driver.input.try_recv().is_err());
        let mut bound = attach(&r, &current, id, dual).await.unwrap();
        let _ = timeout(Duration::from_secs(5), old.completion)
            .await
            .unwrap();
        bootstrap(&r, &current, id, &mut bound, dual).await;
        assert!(matches!(
            r.acknowledge_page(&page, id, bound.generation, 1),
            Err(RegistryError::Forbidden)
        ));
        r.input_page_bound(
            &current,
            id,
            bound.generation,
            "new中文\r".as_bytes().to_vec(),
        )
        .await
        .unwrap();
        assert!(
            matches!(timeout(Duration::from_secs(1), driver.input.recv()).await.unwrap(), Some(TransportWrite::Input(bytes)) if bytes == "new中文\r".as_bytes())
        );
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        r.detach(owner, id, bound.generation).unwrap();
        let _ = timeout(Duration::from_secs(5), bound.completion)
            .await
            .unwrap();
        h.close();
        drop(driver);
        task.join().await.unwrap();
        r.reap_closed(id).unwrap();
        r.shutdown().unwrap();
    }
}

#[tokio::test]
async fn current_page_cannot_borrow_a_window_only_binding() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, mut driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let legacy = if dual {
            r.attach_v2(owner, id, config()).await.unwrap()
        } else {
            r.attach(owner, id, config()).await.unwrap()
        };
        assert!(r.check_binding(owner, id, legacy.generation).is_ok());
        assert!(matches!(
            r.check_page_binding(&page, id, legacy.generation),
            Err(RegistryError::Stale)
        ));
        assert!(matches!(
            r.acknowledge_page(&page, id, legacy.generation, 1),
            Err(RegistryError::Stale)
        ));
        assert!(matches!(
            r.input_page_bound(&page, id, legacy.generation, b"borrow".to_vec())
                .await,
            Err(RegistryError::Stale)
        ));
        assert!(driver.input.try_recv().is_err());
        let mut bound = attach(&r, &page, id, dual).await.unwrap();
        let _ = timeout(Duration::from_secs(5), legacy.completion)
            .await
            .unwrap();
        bootstrap(&r, &page, id, &mut bound, dual).await;
        assert!(r.check_page_binding(&page, id, bound.generation).is_ok());
        r.detach(owner, id, bound.generation).unwrap();
        let _ = timeout(Duration::from_secs(5), bound.completion)
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
async fn cancelled_capture_and_late_scope_drop_cannot_revoke_new_binding() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let page = r.activate_launch_page(owner).unwrap();
        let (h, task, driver) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut pending = Box::pin(attach(&r, &page, id, dual));
        tokio::select! { biased; _ = &mut pending => panic!("capture unexpectedly completed"), _ = std::future::ready(()) => {} }
        drop(pending);
        let current = r.activate_launch_page(owner).unwrap();
        let mut bound = timeout(Duration::from_secs(5), attach(&r, &current, id, dual))
            .await
            .unwrap()
            .unwrap();
        drop(page);
        bootstrap(&r, &current, id, &mut bound, dual).await;
        assert!(r.check_page_binding(&current, id, bound.generation).is_ok());
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        r.detach(owner, id, bound.generation).unwrap();
        let _ = timeout(Duration::from_secs(5), bound.completion)
            .await
            .unwrap();
        h.close();
        drop(driver);
        task.join().await.unwrap();
        r.reap_closed(id).unwrap();
        r.shutdown().unwrap();
    }
}
