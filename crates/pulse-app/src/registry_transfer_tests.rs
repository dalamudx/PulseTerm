#![allow(clippy::unwrap_used)]
use super::tests::{bootstrap, config, session};
use super::*;
use pulse_term::session::{Lifecycle, TransportWrite};
use tokio::time::{Duration, timeout};
#[tokio::test]
async fn transfer_revokes_old_generation_but_keeps_same_actor_and_live_output() {
    let r = SessionRegistry::new();
    let a = r.open_window().unwrap();
    let b = r.open_window().unwrap();
    let (handle, task, mut driver) = session();
    let id = r.register(a, handle.clone()).unwrap();
    let mut old = r.attach(a, id, config()).await.unwrap();
    bootstrap(&r, a, id, &mut old).await;
    let old_input = r.lock().unwrap().bindings.get(&id).unwrap().handle.clone();
    driver
        .output
        .send("before中文".as_bytes().to_vec())
        .await
        .unwrap();
    let mut state = handle.state.clone();
    timeout(Duration::from_secs(1), async {
        while state.borrow().parsed_bytes == 0 {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let before = r.state(a, id).unwrap();
    r.transfer(a, b, id).unwrap();
    assert!(
        old_input
            .input(b"late-before-admission".to_vec())
            .await
            .is_err()
    );
    assert!(driver.input.try_recv().is_err());
    assert!(r.state(a, id).is_err());
    assert_eq!(r.state(b, id).unwrap(), before);
    assert!(r.check_binding(b, id, old.generation).is_err());
    assert!(r.acknowledge(a, id, old.generation, 1).is_err());
    assert!(
        r.input_bound(a, id, old.generation, b"old".to_vec())
            .await
            .is_err()
    );
    assert!(r.detach(a, id, old.generation).is_err());
    r.close_window(a).unwrap();
    driver.output.send(b"after-hidden".to_vec()).await.unwrap();
    timeout(Duration::from_secs(1), async {
        while state.borrow().parsed_bytes == before.parsed_bytes {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let mut next = r.attach(b, id, config()).await.unwrap();
    assert!(next.generation > old.generation);
    bootstrap(&r, b, id, &mut next).await;
    let _ = timeout(Duration::from_secs(1), old.completion)
        .await
        .unwrap();
    r.check_binding(b, id, next.generation).unwrap();
    r.input_bound(b, id, next.generation, b"new".to_vec())
        .await
        .unwrap();
    assert!(matches!(driver.input.recv().await,Some(TransportWrite::Input(bytes))if bytes==b"new"));
    assert_eq!(r.state(b, id).unwrap().lifecycle, Lifecycle::Running);
    r.close_session(b, id).unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), task.join())
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Closed
    );
    drop(driver);
}
#[tokio::test]
async fn failed_transfer_and_noop_preserve_original_binding_and_capture() {
    let r = SessionRegistry::new();
    let a = r.open_window().unwrap();
    let b = r.open_window().unwrap();
    let gone = r.open_window().unwrap();
    r.close_window(gone).unwrap();
    let (handle, task, driver) = session();
    let id = r.register(a, handle).unwrap();
    let mut old = r.attach(a, id, config()).await.unwrap();
    bootstrap(&r, a, id, &mut old).await;
    assert!(r.transfer(b, a, id).is_err());
    assert!(r.transfer(a, gone, id).is_err());
    r.transfer(a, a, id).unwrap();
    r.check_binding(a, id, old.generation).unwrap();
    let entry = SessionRegistry::entry(&r.lock().unwrap(), a, id).unwrap();
    let guard = entry.capture.clone().try_lock_owned().unwrap();
    assert!(matches!(r.transfer(a, b, id), Err(RegistryError::Busy)));
    r.check_binding(a, id, old.generation).unwrap();
    drop(guard);
    r.transfer(a, b, id).unwrap();
    r.close_session(b, id).unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), task.join())
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Closed
    );
    drop(driver);
}
#[tokio::test]
async fn terminal_session_cannot_change_owners_after_eof() {
    let r = SessionRegistry::new();
    let a = r.open_window().unwrap();
    let b = r.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = r.register(a, handle).unwrap();
    drop(driver);
    assert_eq!(
        timeout(Duration::from_secs(1), task.join())
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Ended
    );
    assert!(matches!(
        r.transfer(a, b, id),
        Err(RegistryError::Session(SessionError::Closed))
    ));
    assert_eq!(r.state(a, id).unwrap().lifecycle, Lifecycle::Ended);
    assert!(r.state(b, id).is_err());
    r.reap_closed(id).unwrap();
}

#[tokio::test]
async fn ownership_aba_never_restores_old_entry_or_resets_limiter_locks() {
    let r = SessionRegistry::new();
    let a = r.open_window().unwrap();
    let b = r.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = r.register(a, handle).unwrap();
    let original = SessionRegistry::entry(&r.lock().unwrap(), a, id).unwrap();
    let instant = tokio::time::Instant::now();
    *original.comparison.lock().await = Some(instant);
    *original.marker.lock().await = Some(instant);
    *original.repair.lock().await = Some(instant);
    r.transfer(a, b, id).unwrap();
    r.transfer(b, a, id).unwrap();
    let current = {
        let state = r.lock().unwrap();
        assert!(SessionRegistry::current_entry(&state, a, id, &original).is_err());
        let current = SessionRegistry::entry(&state, a, id).unwrap();
        assert!(Arc::ptr_eq(&original.capture, &current.capture));
        assert!(Arc::ptr_eq(&original.comparison, &current.comparison));
        assert!(Arc::ptr_eq(&original.marker, &current.marker));
        assert!(Arc::ptr_eq(&original.repair, &current.repair));
        current
    };
    assert_eq!(*current.comparison.lock().await, Some(instant));
    r.close_session(a, id).unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), task.join())
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Closed
    );
    drop(driver);
}
