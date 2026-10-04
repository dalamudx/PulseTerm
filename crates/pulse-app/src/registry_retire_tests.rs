#![allow(clippy::unwrap_used)]
use super::tests::{bootstrap, config, session};
use super::*;
use pulse_term::session::Lifecycle;
use tokio::time::{Duration, timeout};
#[tokio::test]
async fn retire_revokes_all_owner_views_not_other_window_or_sessions() {
    let r = SessionRegistry::new();
    let owner = r.open_window().unwrap();
    let other = r.open_window().unwrap();
    let (h1, t1, mut d1) = session();
    let (h2, t2, d2) = session();
    let (h3, t3, d3) = session();
    let id1 = r.register(owner, h1.clone()).unwrap();
    let id2 = r.register(owner, h2).unwrap();
    let id3 = r.register(other, h3).unwrap();
    let mut v1 = r.attach(owner, id1, config()).await.unwrap();
    bootstrap(&r, owner, id1, &mut v1).await;
    let mut v2 = r.attach(owner, id2, config()).await.unwrap();
    bootstrap(&r, owner, id2, &mut v2).await;
    let mut foreign = r.attach(other, id3, config()).await.unwrap();
    bootstrap(&r, other, id3, &mut foreign).await;
    assert_eq!(r.retire_window_views(owner).unwrap(), 2);
    assert_eq!(r.retire_window_views(owner).unwrap(), 0);
    for (id, generation) in [(id1, v1.generation), (id2, v2.generation)] {
        assert_eq!(r.state(owner, id).unwrap().lifecycle, Lifecycle::Running);
        assert!(r.check_binding(owner, id, generation).is_err());
        assert!(r.acknowledge(owner, id, generation, 0).is_err());
        assert!(
            r.input_bound(owner, id, generation, b"late".to_vec())
                .await
                .is_err()
        );
    }
    assert!(d1.input.try_recv().is_err());
    r.check_binding(other, id3, foreign.generation).unwrap();
    let before = h1.state.borrow().parsed_bytes;
    d1.output.send(b"post-navigation".to_vec()).await.unwrap();
    let mut observed = h1.state.clone();
    timeout(Duration::from_secs(1), async {
        while observed.borrow().parsed_bytes == before {
            observed.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let mut next = r.attach(owner, id1, config()).await.unwrap();
    bootstrap(&r, owner, id1, &mut next).await;
    assert!(next.generation > v1.generation);
    let _ = timeout(Duration::from_secs(1), v1.completion)
        .await
        .unwrap();
    r.check_binding(owner, id1, next.generation).unwrap();
    r.close_session(owner, id1).unwrap();
    r.close_session(owner, id2).unwrap();
    r.close_session(other, id3).unwrap();
    for task in [t1, t2, t3] {
        assert_eq!(
            timeout(Duration::from_secs(1), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
    }
    drop((d1, d2, d3));
}
#[tokio::test]
async fn retire_during_capture_expires_entry_without_resetting_gate_or_rate_limits() {
    let r = SessionRegistry::new();
    let owner = r.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = r.register(owner, handle).unwrap();
    let old = SessionRegistry::entry(&r.lock().unwrap(), owner, id).unwrap();
    let guard = old.capture.clone().try_lock_owned().unwrap();
    let instant = tokio::time::Instant::now();
    *old.comparison.lock().await = Some(instant);
    *old.marker.lock().await = Some(instant);
    *old.repair.lock().await = Some(instant);
    assert_eq!(r.retire_window_views(owner).unwrap(), 0);
    let fresh = {
        let state = r.lock().unwrap();
        assert!(SessionRegistry::current_entry(&state, owner, id, &old).is_err());
        SessionRegistry::entry(&state, owner, id).unwrap()
    };
    assert!(Arc::ptr_eq(&old.capture, &fresh.capture));
    assert!(Arc::ptr_eq(&old.comparison, &fresh.comparison));
    assert!(Arc::ptr_eq(&old.marker, &fresh.marker));
    assert!(Arc::ptr_eq(&old.repair, &fresh.repair));
    assert_eq!(*fresh.marker.lock().await, Some(instant));
    assert!(matches!(
        r.attach(owner, id, config()).await,
        Err(RegistryError::Busy)
    ));
    drop(guard);
    let mut next = r.attach(owner, id, config()).await.unwrap();
    bootstrap(&r, owner, id, &mut next).await;
    r.close_session(owner, id).unwrap();
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
async fn closed_navigation_owner_cannot_retire_foreign_views() {
    let r = SessionRegistry::new();
    let owner = r.open_window().unwrap();
    let gone = r.open_window().unwrap();
    r.close_window(gone).unwrap();
    let (handle, task, driver) = session();
    let id = r.register(owner, handle).unwrap();
    let mut current = r.attach(owner, id, config()).await.unwrap();
    bootstrap(&r, owner, id, &mut current).await;
    assert!(matches!(
        r.retire_window_views(gone),
        Err(RegistryError::Forbidden)
    ));
    r.check_binding(owner, id, current.generation).unwrap();
    r.close_session(owner, id).unwrap();
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
