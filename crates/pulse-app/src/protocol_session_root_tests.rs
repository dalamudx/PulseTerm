use super::*;
use crate::local_session_root::SessionRoot;

#[tokio::test]
async fn same_root_protocol_owner_survives_page_change_and_keeps_both_quotas_until_actual_reap() {
    let registry = SessionRegistry::new();
    let (window, original) = page_and_window(&registry);
    let started = ProtocolSessionReservation::reserve(&registry, &original)
        .unwrap()
        .begin()
        .unwrap();
    let (handle, task, driver) = actor();
    let (join, ended, release) = held_join(task, driver);
    let owner = started.commit(handle, join).unwrap();
    let id = owner.id();
    let mut root = SessionRoot::new(registry.clone(), tokio::runtime::Handle::current());
    assert_eq!(root.adopt_protocol(owner).unwrap(), id);
    assert_eq!(root.ids(), vec![id]);
    assert!(root.is_protocol(id));
    assert_eq!(root.occupied(), 1);
    let current = registry.activate_launch_page(window).unwrap();
    ended.await.unwrap();
    assert_eq!(root.ids(), vec![id]);
    let reservations = (0..63)
        .map(|_| ProtocolSessionReservation::reserve(&registry, &current).unwrap())
        .collect::<Vec<_>>();
    assert!(ProtocolSessionReservation::reserve(&registry, &current).is_err());
    let cleanup = root.close_protocol(id).unwrap();
    assert!(root.ids().is_empty());
    assert_eq!(root.collect_reaped(), 0);
    assert_eq!(root.occupied(), 1);
    assert!(registry.state(window, id).is_ok());
    let mut cleanup = Box::pin(cleanup.join());
    pending(cleanup.as_mut());
    release.send(()).unwrap();
    cleanup.await.unwrap();
    assert_eq!(root.collect_reaped(), 1);
    assert_eq!(root.occupied(), 0);
    assert!(registry.state(window, id).is_err());
    drop(reservations);
    root.shutdown().join().await.unwrap();
}

#[tokio::test]
async fn protocol_behavior_survives_collection_and_page_refresh_until_native_reap() {
    let registry = SessionRegistry::new();
    let (window, original) = page_and_window(&registry);
    let started = ProtocolSessionReservation::reserve(&registry, &original)
        .unwrap()
        .begin()
        .unwrap();
    let (handle, task, driver) = actor();
    let (join, ended, release) = held_join(task, driver);
    let mut owner = started.commit(handle, join).unwrap();
    owner.appearance.backspace_send = Some("bs".into());
    let id = owner.id();
    let mut root = SessionRoot::new(registry.clone(), tokio::runtime::Handle::current());
    root.adopt_protocol(owner).unwrap();
    root.preflight_protocol(&registry).unwrap();
    assert_eq!(
        root.profile_appearance(id).backspace_send.as_deref(),
        Some("bs")
    );
    let _current = registry.activate_launch_page(window).unwrap();
    assert_eq!(root.collect_reaped(), 0);
    assert_eq!(
        root.profile_appearance(id).backspace_send.as_deref(),
        Some("bs")
    );
    ended.await.unwrap();
    let cleanup = root.close_protocol(id).unwrap();
    assert_eq!(root.collect_reaped(), 0);
    assert_eq!(
        root.profile_appearance(id).backspace_send.as_deref(),
        Some("bs")
    );
    let mut cleanup = Box::pin(cleanup.join());
    pending(cleanup.as_mut());
    release.send(()).unwrap();
    cleanup.await.unwrap();
    assert_eq!(root.collect_reaped(), 1);
    assert!(root.profile_appearance(id).backspace_send.is_none());
    root.shutdown().join().await.unwrap();
}
#[tokio::test]
async fn foreign_registry_same_numeric_id_cannot_adopt_or_reclaim_root_session() {
    let a = SessionRegistry::new();
    let b = SessionRegistry::new();
    let aw = page(&a);
    let bw = page(&b);
    let mut root = SessionRoot::new(a.clone(), tokio::runtime::Handle::current());
    let (handle, task, driver) = actor();
    let (join, ended_a, release_a) = held_join(task, driver);
    let owner = ProtocolSessionReservation::reserve(&a, &aw)
        .unwrap()
        .begin()
        .unwrap()
        .commit(handle, join)
        .unwrap();
    let id = owner.id();
    root.adopt_protocol(owner).unwrap();
    let (handle, task, driver) = actor();
    let (join, ended_b, release_b) = held_join(task, driver);
    let foreign = ProtocolSessionReservation::reserve(&b, &bw)
        .unwrap()
        .begin()
        .unwrap()
        .commit(handle, join)
        .unwrap();
    assert_eq!(foreign.id(), id);
    assert!(matches!(
        root.adopt_protocol(foreign),
        Err(RegistryError::Forbidden)
    ));
    ended_b.await.unwrap();
    assert_eq!(root.ids(), vec![id]);
    assert_eq!(root.occupied(), 1);
    release_b.send(()).unwrap();
    b.seal_native_workers().join().await.unwrap();
    ended_a.await.unwrap();
    let cleanup = root.close_protocol(id).unwrap();
    release_a.send(()).unwrap();
    cleanup.join().await.unwrap();
    root.collect_reaped();
    root.shutdown().join().await.unwrap();
}

#[tokio::test]
async fn cancelled_root_shutdown_observer_cannot_release_native_or_allow_new_adoption() {
    let registry = SessionRegistry::new();
    let scope = page(&registry);
    let (handle, task, driver) = actor();
    let (join, ended, release) = held_join(task, driver);
    let owner = ProtocolSessionReservation::reserve(&registry, &scope)
        .unwrap()
        .begin()
        .unwrap()
        .commit(handle, join)
        .unwrap();
    let mut root = SessionRoot::new(registry.clone(), tokio::runtime::Handle::current());
    root.adopt_protocol(owner).unwrap();
    drop(root.shutdown());
    ended.await.unwrap();
    assert_eq!(root.occupied(), 1);
    assert!(root.ids().is_empty());
    assert!(ProtocolSessionReservation::reserve(&registry, &scope).is_err());
    let mut drain = Box::pin(root.shutdown().join());
    pending(drain.as_mut());
    release.send(()).unwrap();
    drain.await.unwrap();
    assert_eq!(root.collect_reaped(), 1);
    assert_eq!(root.occupied(), 0);
}

#[tokio::test]
async fn failed_protocol_completion_keeps_the_same_shared_root_slot_quarantined() {
    let registry = SessionRegistry::new();
    let scope = page(&registry);
    let (handle, task, driver) = actor();
    let owner = ProtocolSessionReservation::reserve(&registry, &scope)
        .unwrap()
        .begin()
        .unwrap()
        .commit(handle, async move {
            drop(driver);
            task.join()
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            Err(ProtocolSessionError::NativeUnknown)
        })
        .unwrap();
    let id = owner.id();
    let mut root = SessionRoot::new(registry.clone(), tokio::runtime::Handle::current());
    root.adopt_protocol(owner).unwrap();
    assert!(root.close_protocol(id).unwrap().join().await.is_err());
    assert_eq!(root.collect_reaped(), 0);
    assert_eq!(root.occupied(), 1);
    assert!(root.shutdown().join().await.is_err());
}
