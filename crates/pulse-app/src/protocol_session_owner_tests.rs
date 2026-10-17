#![allow(clippy::unwrap_used)]
#[path = "protocol_session_root_tests.rs"]
mod root_tests;
use super::*;
use pulse_core::TermSize;
use pulse_term::session::{self, ChannelTransport, DriverEndpoint, SessionTask};
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};

fn actor() -> (SessionHandle, SessionTask, DriverEndpoint) {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = session::spawn_session(TermSize::new(32, 8).unwrap(), 64, transport);
    (handle, task, driver)
}
fn page_and_window(registry: &SessionRegistry) -> (crate::registry::WindowToken, PageLaunchScope) {
    let window = registry.open_window().unwrap();
    (window, registry.activate_launch_page(window).unwrap())
}
fn page(registry: &SessionRegistry) -> PageLaunchScope {
    page_and_window(registry).1
}
fn held_join(
    task: SessionTask,
    driver: DriverEndpoint,
) -> (
    impl Future<Output = Result<()>> + Send,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
) {
    let (ended, actor_done) = oneshot::channel();
    let (release, native_done) = oneshot::channel();
    (
        async move {
            drop(driver);
            task.join()
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            let _ = ended.send(());
            native_done
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)
        },
        actor_done,
        release,
    )
}
fn pending<F: Future>(future: std::pin::Pin<&mut F>) {
    assert!(matches!(
        future.poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

#[tokio::test]
async fn known_unregistered_setup_cleanup_retains_credit_after_waiter_drop() {
    let registry = SessionRegistry::new();
    let scope = page(&registry);
    let started = ProtocolSessionReservation::reserve(&registry, &scope)
        .unwrap()
        .begin()
        .unwrap();
    let (release, native_done) = oneshot::channel();
    drop(started.finish_unregistered(async move {
        native_done
            .await
            .map_err(|_| ProtocolSessionError::NativeUnknown)
    }));
    let tickets = (0..63)
        .map(|_| registry.begin_page_local_launch(&scope).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        registry.begin_page_local_launch(&scope),
        Err(RegistryError::Busy)
    ));
    drop(tickets);
    let mut drain = Box::pin(registry.seal_native_workers().join());
    pending(drain.as_mut());
    release.send(()).unwrap();
    drain.await.unwrap();
    assert!(registry.begin_page_local_launch(&scope).is_ok());
}

#[tokio::test]
async fn original_page_and_native_quota_survive_retirement_and_actor_eof() {
    let registry = SessionRegistry::new();
    let (window, original) = page_and_window(&registry);
    let started = ProtocolSessionReservation::reserve(&registry, &original)
        .unwrap()
        .begin()
        .unwrap();
    let (handle, task, driver) = actor();
    let (join, actor_done, release) = held_join(task, driver);
    let owner = started.commit(handle.clone(), join).unwrap();
    let id = owner.id();
    let _current_page = registry.activate_launch_page(window).unwrap();
    assert!(owner.is_managed_by(&registry));
    actor_done.await.unwrap();
    // EOF/actor join still does not establish transport native exit.
    let mut finish = Box::pin(owner.finish());
    pending(finish.as_mut());
    let mut drain = Box::pin(registry.seal_native_workers().join());
    pending(drain.as_mut());
    assert!(registry.state(window, id).is_ok());
    release.send(()).unwrap();
    finish.await.unwrap();
    drain.await.unwrap();
    assert!(registry.state(window, id).is_err());
}

#[tokio::test]
async fn rejected_old_page_commit_keeps_reservations_until_actual_join() {
    let registry = SessionRegistry::new();
    let (window, original) = page_and_window(&registry);
    let started = ProtocolSessionReservation::reserve(&registry, &original)
        .unwrap()
        .begin()
        .unwrap();
    let current = registry.activate_launch_page(window).unwrap();
    let (handle, task, driver) = actor();
    let (join, actor_done, release) = held_join(task, driver);
    assert!(matches!(
        started.commit(handle, join),
        Err(ProtocolSessionError::Registry(RegistryError::Forbidden))
    ));
    actor_done.await.unwrap();
    let tickets = (0..63)
        .map(|_| registry.begin_page_local_launch(&current).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        registry.begin_page_local_launch(&current),
        Err(RegistryError::Busy)
    ));
    let guards = (0..63)
        .map(|_| registry.native_workers.reserve().unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        registry.native_workers.reserve(),
        Err(RegistryError::Busy)
    ));
    drop(tickets);
    drop(guards);
    let mut drain = Box::pin(registry.seal_native_workers().join());
    pending(drain.as_mut());
    release.send(()).unwrap();
    drain.await.unwrap();
    assert!(registry.begin_page_local_launch(&current).is_ok());
}

#[tokio::test]
async fn dropped_finish_and_cleanup_waiters_cannot_abort_native_reclamation() {
    let registry = SessionRegistry::new();
    let scope = page(&registry);
    let started = ProtocolSessionReservation::reserve(&registry, &scope)
        .unwrap()
        .begin()
        .unwrap();
    let (handle, task, driver) = actor();
    let (join, actor_done, release) = held_join(task, driver);
    let owner = started.commit(handle, join).unwrap();
    let mut observation = owner.observe_cleanup();
    actor_done.await.unwrap();
    let mut finish = Box::pin(owner.finish());
    pending(finish.as_mut());
    drop(finish);
    assert_eq!(*observation.borrow(), CleanupState::Closing);
    release.send(()).unwrap();
    observation
        .wait_for(|s| *s == CleanupState::Reaped)
        .await
        .unwrap();
    registry.seal_native_workers().join().await.unwrap();

    let other = SessionRegistry::new();
    let scope = page(&other);
    let (handle, task, driver) = actor();
    let (join, actor_done, release) = held_join(task, driver);
    let owner = ProtocolSessionReservation::reserve(&other, &scope)
        .unwrap()
        .begin()
        .unwrap()
        .commit(handle, join)
        .unwrap();
    let mut observation = owner.observe_cleanup();
    drop(owner.cancel());
    actor_done.await.unwrap();
    release.send(()).unwrap();
    observation
        .wait_for(|s| *s == CleanupState::Reaped)
        .await
        .unwrap();
    other.seal_native_workers().join().await.unwrap();
}

#[tokio::test]
async fn same_numeric_ids_never_change_owner_registry_provenance() {
    let a = SessionRegistry::new();
    let b = SessionRegistry::new();
    let pa = page(&a);
    let (window_b, pb) = page_and_window(&b);
    let (ha, ta, da) = actor();
    let (hb, tb, db) = actor();
    let (ja, done_a, release_a) = held_join(ta, da);
    let (jb, done_b, release_b) = held_join(tb, db);
    let oa = ProtocolSessionReservation::reserve(&a, &pa)
        .unwrap()
        .begin()
        .unwrap()
        .commit(ha, ja)
        .unwrap();
    let ob = ProtocolSessionReservation::reserve(&b, &pb)
        .unwrap()
        .begin()
        .unwrap()
        .commit(hb, jb)
        .unwrap();
    assert_eq!(oa.id(), ob.id());
    assert!(!oa.is_managed_by(&b));
    assert!(!ob.is_managed_by(&a));
    assert!(ProtocolSessionReservation::reserve(&a, &pb).is_err());
    let id = ob.id();
    done_a.await.unwrap();
    done_b.await.unwrap();
    release_a.send(()).unwrap();
    oa.finish().await.unwrap();
    assert!(b.state(window_b, id).is_ok());
    release_b.send(()).unwrap();
    ob.finish().await.unwrap();
}

#[tokio::test]
async fn unknown_started_drop_and_native_error_are_quarantined_not_successful_exit() {
    let registry = SessionRegistry::new();
    let scope = page(&registry);
    drop(
        ProtocolSessionReservation::reserve(&registry, &scope)
            .unwrap()
            .begin()
            .unwrap(),
    );
    let tickets = (0..63)
        .map(|_| registry.begin_page_local_launch(&scope).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        registry.begin_page_local_launch(&scope),
        Err(RegistryError::Busy)
    ));
    drop(tickets);
    assert!(registry.seal_native_workers().join().await.is_err());

    let other = SessionRegistry::new();
    let (window, scope) = page_and_window(&other);
    let (handle, task, driver) = actor();
    let h = handle.clone();
    let owner = ProtocolSessionReservation::reserve(&other, &scope)
        .unwrap()
        .begin()
        .unwrap()
        .commit(handle, async move {
            h.close();
            drop(driver);
            task.join()
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            Err(ProtocolSessionError::NativeUnknown)
        })
        .unwrap();
    let observation = owner.observe_cleanup();
    let id = owner.id();
    assert!(owner.finish().await.is_err());
    assert_eq!(*observation.borrow(), CleanupState::Failed);
    assert!(other.state(window, id).is_ok());
    assert!(other.seal_native_workers().join().await.is_err());
}

#[tokio::test]
async fn unused_reservation_and_retired_pre_native_begin_do_not_quarantine() {
    let registry = SessionRegistry::new();
    let (window, scope) = page_and_window(&registry);
    drop(ProtocolSessionReservation::reserve(&registry, &scope).unwrap());
    let reservation = ProtocolSessionReservation::reserve(&registry, &scope).unwrap();
    let _current_page = registry.activate_launch_page(window).unwrap();
    assert!(reservation.begin().is_err());
    registry.seal_native_workers().join().await.unwrap();
}
