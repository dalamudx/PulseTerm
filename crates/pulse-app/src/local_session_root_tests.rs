#![allow(clippy::unwrap_used)]
use super::*;
use crate::{local_launch::start_local_registered, registry::WindowToken};
use pulse_core::TermSize;
use pulse_pty::PtyCommand;
use pulse_term::session::Lifecycle;
use tokio::{
    sync::oneshot,
    time::{Duration, timeout},
};
async fn launch(
    registry: &SessionRegistry,
    window: WindowToken,
    body: &str,
) -> RegisteredLocalSession {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), body.into()];
    timeout(
        Duration::from_secs(5),
        start_local_registered(registry, window, command, TermSize::new(32, 8).unwrap(), 16)
            .unwrap()
            .wait(),
    )
    .await
    .unwrap()
    .unwrap()
}
async fn running(ready: &RegisteredLocalSession) {
    let mut state = ready.handle().state.clone();
    timeout(Duration::from_secs(5), async {
        while state.borrow().parsed_bytes == 0 {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(state.borrow().lifecycle, Lifecycle::Running);
}
async fn reaped(observer: &mut watch::Receiver<CleanupState>) {
    timeout(
        Duration::from_secs(5),
        observer.wait_for(|state| *state == CleanupState::Reaped),
    )
    .await
    .unwrap()
    .unwrap();
}
#[tokio::test]
async fn root_survives_navigation_and_window_close_and_finishes_after_native_proof() {
    let r = SessionRegistry::new();
    let window = r.open_window().unwrap();
    let page = r.activate_launch_page(window).unwrap();
    let ready = launch(&r, window, "printf ready; read -r line").await;
    running(&ready).await;
    let handle = ready.handle().clone();
    let mut root = LocalSessionRoot::new(r.clone(), Handle::current());
    let id = root.adopt(ready).unwrap();
    drop(page);
    r.close_window(window).unwrap();
    assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Running);
    assert_eq!(root.ids(), vec![id]);
    let owner = root.take_for_finish(id).unwrap();
    assert!(root.ids().is_empty());
    assert_eq!(root.occupied(), 1);
    assert_eq!(root.collect_reaped(), 0);
    timeout(Duration::from_secs(5), owner.cancel().join())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(root.collect_reaped(), 1);
    assert_eq!(root.occupied(), 0);
    assert_eq!(r.test_registered_session_count().unwrap(), 0);
    r.shutdown().unwrap();
}
#[tokio::test]
async fn dropped_close_waiter_and_root_drop_do_not_abort_native_cleanup() {
    let r = SessionRegistry::new();
    let window = r.open_window().unwrap();
    let ready = launch(&r, window, "printf ready; read -r line").await;
    running(&ready).await;
    let mut root = LocalSessionRoot::new(r.clone(), Handle::current());
    let id = root.adopt(ready).unwrap();
    let cleanup = root.close(id).unwrap();
    let mut observer = root.retiring.get(&id).unwrap().clone();
    drop(cleanup);
    drop(root);
    reaped(&mut observer).await;
    assert_eq!(r.test_registered_session_count().unwrap(), 0);
    r.shutdown().unwrap();
}
#[tokio::test]
async fn foreign_registry_same_id_rejection_cleans_only_the_foreign_session() {
    let a = SessionRegistry::new();
    let b = SessionRegistry::new();
    let aw = a.open_window().unwrap();
    let bw = b.open_window().unwrap();
    let ar = launch(&a, aw, "printf A; read -r line").await;
    running(&ar).await;
    let br = launch(&b, bw, "printf B; read -r line").await;
    running(&br).await;
    assert_eq!(ar.id(), br.id());
    let b_owned = LocalSessionOwner::adopt(Handle::current(), br);
    let mut b_observer = b_owned.observe_cleanup();
    let mut root = LocalSessionRoot::new(a.clone(), Handle::current());
    let id = root.adopt(ar).unwrap();
    assert!(matches!(
        root.adopt_owned(b_owned),
        Err(RegistryError::Forbidden)
    ));
    reaped(&mut b_observer).await;
    assert_eq!(b.test_registered_session_count().unwrap(), 0);
    assert_eq!(root.ids(), vec![id]);
    assert_eq!(root.occupied(), 1);
    let owner = root.take_for_finish(id).unwrap();
    assert_eq!(owner.handle().state.borrow().lifecycle, Lifecycle::Running);
    timeout(Duration::from_secs(5), owner.cancel().join())
        .await
        .unwrap()
        .unwrap();
    root.collect_reaped();
    a.shutdown().unwrap();
    b.shutdown().unwrap();
}
#[tokio::test]
async fn gated_native_receipt_retains_slot_after_actor_closed_and_rejects_double_finish() {
    let r = SessionRegistry::new();
    let window = r.open_window().unwrap();
    let ready = launch(&r, window, "printf ready; read -r line").await;
    running(&ready).await;
    let RegisteredLocalSession {
        id,
        handle,
        completion: actual,
        registry,
    } = ready;
    let (deliver, completion) = oneshot::channel();
    let mut root = LocalSessionRoot::new(r.clone(), Handle::current());
    root.adopt(RegisteredLocalSession {
        id,
        handle,
        completion,
        registry,
    })
    .unwrap();
    let cleanup = root.close(id).unwrap();
    assert!(matches!(root.close(id), Err(RegistryError::Forbidden)));
    let receipt = timeout(Duration::from_secs(5), actual)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(root.collect_reaped(), 0);
    assert_eq!(root.occupied(), 1);
    assert_eq!(r.test_registered_session_count().unwrap(), 1);
    deliver.send(Ok(receipt)).unwrap();
    timeout(Duration::from_secs(5), cleanup.join())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(root.collect_reaped(), 1);
    assert_eq!(root.occupied(), 0);
    r.shutdown().unwrap();
}
#[tokio::test]
async fn missing_native_proof_retains_failed_capacity_and_shutdown_rejects_new_owners() {
    let r = SessionRegistry::new();
    let window = r.open_window().unwrap();
    let ready = launch(&r, window, "printf ready; read -r line").await;
    running(&ready).await;
    let RegisteredLocalSession {
        id,
        handle,
        completion: actual,
        registry,
    } = ready;
    let (deliver, completion) = oneshot::channel();
    let mut root = LocalSessionRoot::new(r.clone(), Handle::current());
    root.adopt(RegisteredLocalSession {
        id,
        handle,
        completion,
        registry,
    })
    .unwrap();
    let cleanup = root.close(id).unwrap();
    let _proof = timeout(Duration::from_secs(5), actual)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    drop(deliver);
    assert!(cleanup.join().await.is_err());
    assert_eq!(root.collect_reaped(), 0);
    assert_eq!(root.occupied(), 1);
    assert_eq!(
        *root.retiring.get(&id).unwrap().borrow(),
        CleanupState::Failed
    );
    assert!(root.close_all().is_empty());
    let ready = launch(&r, window, "exit 0").await;
    assert!(matches!(root.adopt(ready), Err(RegistryError::Forbidden)));
    assert_eq!(root.collect_reaped(), 0);
    assert_eq!(root.occupied(), 1);
    r.reap_closed(id).unwrap(); // Actual proof is independently held by this fixture.
    r.shutdown().unwrap();
}
#[tokio::test]
async fn shutdown_closes_all_root_owners_and_waiters_prove_native_completion() {
    let r = SessionRegistry::new();
    let window = r.open_window().unwrap();
    let mut root = LocalSessionRoot::new(r.clone(), Handle::current());
    for _ in 0..2 {
        let ready = launch(&r, window, "printf ready; read -r line").await;
        running(&ready).await;
        root.adopt(ready).unwrap();
    }
    let waiters = root.close_all();
    assert_eq!(waiters.len(), 2);
    assert!(root.ids().is_empty());
    assert_eq!(root.occupied(), 2);
    assert!(root.close_all().is_empty());
    for waiter in waiters {
        timeout(Duration::from_secs(5), waiter.join())
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(root.collect_reaped(), 2);
    assert_eq!(root.occupied(), 0);
    assert_eq!(r.test_registered_session_count().unwrap(), 0);
    r.shutdown().unwrap();
}

#[tokio::test]
async fn dropping_root_with_owned_child_closes_and_reaps_while_observer_survives() {
    let r = SessionRegistry::new();
    let window = r.open_window().unwrap();
    let ready = launch(&r, window, "printf ready; read -r line").await;
    running(&ready).await;
    let owner = LocalSessionOwner::adopt(Handle::current(), ready);
    let mut observer = owner.observe_cleanup();
    let mut root = LocalSessionRoot::new(r.clone(), Handle::current());
    root.adopt_owned(owner).unwrap();
    drop(root);
    reaped(&mut observer).await;
    assert_eq!(r.test_registered_session_count().unwrap(), 0);
    r.shutdown().unwrap();
}

#[tokio::test]
async fn unknown_closed_watch_slots_stay_full_and_busy_rejection_reaps_new_child() {
    let r = SessionRegistry::new();
    let window = r.open_window().unwrap();
    let mut root = LocalSessionRoot::new(r.clone(), Handle::current());
    // Synthetic metadata/closed watches, NOT a 64-PTY capacity/performance probe.
    // Mint unique backend ids through real registry admission, then explicitly retire
    // these fake transports. No completed native proof is delivered to the root watches.
    for _ in 0..MAX_ROOT_LOCAL_SESSIONS {
        let (transport, _driver) = pulse_term::session::ChannelTransport::bounded_driver();
        let (handle, task) =
            pulse_term::session::spawn_session(TermSize::new(32, 8).unwrap(), 64, transport);
        let id = r.register(window, handle.clone()).unwrap();
        handle.close();
        timeout(Duration::from_secs(5), task.join())
            .await
            .unwrap()
            .unwrap();
        r.reap_closed(id).unwrap();
        let (producer, observer) = watch::channel(CleanupState::Closing);
        drop(producer);
        root.retiring.insert(id, observer);
    }
    assert_eq!(root.occupied(), MAX_ROOT_LOCAL_SESSIONS);
    assert_eq!(root.collect_reaped(), 0);
    let ready = launch(&r, window, "printf ready; read -r line").await;
    running(&ready).await;
    let owner = LocalSessionOwner::adopt(Handle::current(), ready);
    let mut observer = owner.observe_cleanup();
    assert!(matches!(root.adopt_owned(owner), Err(RegistryError::Busy)));
    reaped(&mut observer).await;
    assert_eq!(r.test_registered_session_count().unwrap(), 0);
    assert_eq!(root.occupied(), MAX_ROOT_LOCAL_SESSIONS);
    assert_eq!(root.collect_reaped(), 0);
    r.shutdown().unwrap();
}
