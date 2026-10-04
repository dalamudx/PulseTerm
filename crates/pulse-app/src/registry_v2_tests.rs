#![allow(clippy::unwrap_used)]
use super::tests::{config, session};
use super::*;
use pulse_ipc::{attach::Part, decode};
use pulse_term::session::Lifecycle;
async fn bootstrap(
    r: &SessionRegistry,
    o: WindowToken,
    id: SessionId,
    b: &mut RegisteredAttachment,
) {
    loop {
        let w = b.wire.recv().await.unwrap();
        let f = decode(&w).unwrap();
        let p = pulse_ipc::attach_v2::decode_fragment(f.payload).unwrap();
        r.acknowledge(o, id, b.generation, f.sequence).unwrap();
        if p.part == Part::Commit {
            break;
        }
    }
}
#[tokio::test]
async fn v2_foreign_capture_ack_input_and_close_are_owner_bound() {
    let r = SessionRegistry::new();
    let o = r.open_window().unwrap();
    let foreign = r.open_window().unwrap();
    let (h, t, d) = session();
    let id = r.register(o, h).unwrap();
    assert!(matches!(
        r.attach_v2(foreign, id, config()).await,
        Err(RegistryError::Forbidden)
    ));
    let mut b = r.attach_v2(o, id, config()).await.unwrap();
    assert!(matches!(
        r.acknowledge(foreign, id, b.generation, 0),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        r.input_bound(foreign, id, b.generation, b"x".to_vec())
            .await,
        Err(RegistryError::Forbidden)
    ));
    bootstrap(&r, o, id, &mut b).await;
    assert!(r.check_binding(o, id, b.generation).is_ok());
    r.close_window(o).unwrap();
    assert!(b.completion.await.unwrap().is_err());
    assert!(matches!(
        r.state(foreign, id),
        Err(RegistryError::Forbidden)
    ));
    // Window close only revokes the view; explicit app shutdown closes the actor.
    r.shutdown().unwrap();
    assert_eq!(t.join().await.unwrap().lifecycle, Lifecycle::Closed);
    drop(d);
}
#[tokio::test]
async fn v1_to_v2_replacement_ignores_old_ack_detach_and_late_completion() {
    let r = SessionRegistry::new();
    let o = r.open_window().unwrap();
    let (h, t, d) = session();
    let id = r.register(o, h).unwrap();
    let mut old = r.attach(o, id, config()).await.unwrap();
    super::tests::bootstrap(&r, o, id, &mut old).await;
    let mut next = r.attach_v2(o, id, config()).await.unwrap();
    assert!(next.generation > old.generation);
    assert!(matches!(
        r.acknowledge(o, id, old.generation, 0),
        Err(RegistryError::Stale)
    ));
    assert!(matches!(
        r.detach(o, id, old.generation),
        Err(RegistryError::Stale)
    ));
    assert!(old.completion.await.unwrap().is_err());
    bootstrap(&r, o, id, &mut next).await;
    assert!(r.check_binding(o, id, next.generation).is_ok());
    drop(d);
    while let Some(w) = next.wire.recv().await {
        r.acknowledge(o, id, next.generation, decode(&w).unwrap().sequence)
            .unwrap();
    }
    assert_eq!(
        next.completion.await.unwrap().unwrap().boundary.lifecycle,
        Lifecycle::Ended
    );
    t.join().await.unwrap();
}
#[tokio::test]
async fn invalid_v2_limits_do_not_remove_valid_v1_binding() {
    let r = SessionRegistry::new();
    let o = r.open_window().unwrap();
    let (h, t, d) = session();
    let id = r.register(o, h).unwrap();
    let mut old = r.attach(o, id, config()).await.unwrap();
    super::tests::bootstrap(&r, o, id, &mut old).await;
    assert!(
        r.attach_v2(
            o,
            id,
            AttachmentConfig {
                fragment: 7,
                bytes: 63,
                ..config()
            }
        )
        .await
        .is_err()
    );
    assert!(r.check_binding(o, id, old.generation).is_ok());
    r.shutdown().unwrap();
    drop(d);
    t.join().await.unwrap();
}
