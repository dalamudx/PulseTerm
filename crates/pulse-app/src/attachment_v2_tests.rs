#![allow(clippy::unwrap_used)]
use super::*;
use pulse_core::TermSize;
use pulse_ipc::{attach_v2, decode};
use pulse_term::session::{ChannelTransport, spawn_session};
use tokio::time::timeout;
fn config() -> AttachmentConfig {
    AttachmentConfig {
        fragment: 7,
        frames: 1,
        bytes: 64,
        ack_timeout: Duration::from_secs(2),
    }
}
#[tokio::test]
async fn v2_invalid_credit_preserves_old_lease_before_atomic_capture() {
    let (t, d) = ChannelTransport::bounded_driver();
    let (s, actor) = spawn_session(TermSize::new(32, 8).unwrap(), 32, t);
    let old = s.diagnostic_subscribe(0, 0, 1024 * 1024).await.unwrap();
    assert!(matches!(
        attach_v2(
            &s,
            2,
            AttachmentConfig {
                bytes: 63,
                ..config()
            }
        )
        .await,
        Err(AttachmentError::Limits)
    ));
    assert_eq!(old.status.borrow().end, StreamEnd::Open);
    s.close();
    drop(d);
    actor.join().await.unwrap();
}
#[tokio::test]
async fn v2_old_generation_ack_revokes_only_view_and_new_generation_completes() {
    let (t, d) = ChannelTransport::bounded_driver();
    let (s, actor) = spawn_session(TermSize::new(32, 8).unwrap(), 32, t);
    let (h, mut wire, binding) = attach_v2(&s, 9, config()).await.unwrap();
    let first = wire.recv().await.unwrap();
    let f = decode(&first).unwrap();
    assert!(attach_v2::decode_fragment(f.payload).is_ok());
    h.acknowledge(8, f.sequence).unwrap();
    assert!(matches!(
        binding.join().await,
        Err(AttachmentError::Protocol(AttachError::Generation))
    ));
    assert_eq!(s.state.borrow().lifecycle, Lifecycle::Running);
    let (h, mut wire, binding) = attach_v2(&s, 10, config()).await.unwrap();
    drop(d);
    timeout(Duration::from_secs(3), async {
        while let Some(w) = wire.recv().await {
            h.acknowledge(10, decode(&w).unwrap().sequence).unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(
        binding.join().await.unwrap().boundary.lifecycle,
        Lifecycle::Ended
    );
    actor.join().await.unwrap();
}
#[tokio::test]
async fn v2_no_ack_stops_actor_and_timeout_releases_without_session_close() {
    let (t, d) = ChannelTransport::bounded_driver();
    let (s, actor) = spawn_session(TermSize::new(32, 8).unwrap(), 32, t);
    let (_h, mut wire, binding) = attach_v2(
        &s,
        7,
        AttachmentConfig {
            ack_timeout: Duration::from_millis(100),
            ..config()
        },
    )
    .await
    .unwrap();
    wire.recv().await.unwrap();
    for _ in 0..12 {
        d.output.send(b"x".to_vec()).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    let before = s.state.borrow().utf8_bytes;
    assert!(before <= 8);
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(s.state.borrow().utf8_bytes, before);
    assert!(matches!(
        binding.join().await,
        Err(AttachmentError::AckTimeout)
    ));
    assert_eq!(s.state.borrow().lifecycle, Lifecycle::Running);
    s.close();
    drop(d);
    actor.join().await.unwrap();
}
