#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn repair_remains_single_flight_until_commit_ack_and_orders_subsequent_data() {
    let (s, actor, d) = session();
    let (h, mut wire, task) = attach(&s, 9, config()).await.unwrap();
    bootstrap(&h, &mut wire, 9).await;
    let state = s.state.borrow().clone();
    assert_eq!(
        h.request_repair(state.clone(), "0000000000000000".into())
            .await
            .unwrap(),
        pulse_term::session::RepairAdmission::Queued
    );
    let mut parts = Vec::new();
    loop {
        let raw = wire.recv().await.unwrap();
        let f = pulse_ipc::decode(&raw).unwrap();
        let p = pulse_ipc::attach::decode_fragment(f.payload).unwrap();
        parts.push(p.part);
        if p.part == pulse_ipc::attach::Part::RepairCommit {
            assert!(
                h.request_repair(state.clone(), "0000000000000000".into())
                    .await
                    .is_err()
            );
            d.output.send(b"next".to_vec()).await.unwrap();
            assert!(
                timeout(Duration::from_millis(40), wire.recv())
                    .await
                    .is_err()
            );
            h.acknowledge(9, f.sequence).unwrap();
            break;
        }
        h.acknowledge(9, f.sequence).unwrap();
    }
    assert_eq!(parts[0], pulse_ipc::attach::Part::RepairBegin);
    let raw = wire.recv().await.unwrap();
    let f = pulse_ipc::decode(&raw).unwrap();
    let p = pulse_ipc::attach::decode_fragment(f.payload).unwrap();
    assert_eq!(p.part, pulse_ipc::attach::Part::Data);
    assert_eq!(p.offset, 0);
    h.acknowledge(9, f.sequence).unwrap();
    h.detach();
    assert!(task.join().await.is_err());
    s.close();
    drop(d);
    actor.join().await.unwrap();
}

use pulse_core::TermSize;
use pulse_term::session::{self, ChannelTransport, DriverEndpoint, SessionTask};
use tokio::time::{sleep, timeout};
fn session() -> (SessionHandle, SessionTask, DriverEndpoint) {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = session::spawn_session(TermSize::new(32, 8).unwrap(), 64, transport);
    (handle, task, driver)
}
fn config() -> AttachmentConfig {
    AttachmentConfig {
        fragment: 1024,
        frames: 1,
        bytes: 1100,
        ack_timeout: Duration::from_secs(2),
    }
}
async fn bootstrap(handle: &AttachmentHandle, wire: &mut mpsc::Receiver<Vec<u8>>, generation: u64) {
    loop {
        let wire = timeout(Duration::from_secs(1), wire.recv())
            .await
            .unwrap()
            .unwrap();
        let frame = pulse_ipc::decode(&wire).unwrap();
        let part = pulse_ipc::attach::decode_fragment(frame.payload)
            .unwrap()
            .part;
        handle.acknowledge(generation, frame.sequence).unwrap();
        if part == pulse_ipc::attach::Part::Commit {
            break;
        }
    }
}
#[tokio::test]
async fn marker_follows_data_and_uses_the_same_credit_without_parser_flush() {
    let (session, actor, driver) = session();
    let (handle, mut wire, task) = attach(&session, 8, config()).await.unwrap();
    bootstrap(&handle, &mut wire, 8).await;
    driver.output.send(b"x\x1b[31".to_vec()).await.unwrap();
    let data = wire.recv().await.unwrap();
    let frame = pulse_ipc::decode(&data).unwrap();
    assert_eq!(
        pulse_ipc::attach::decode_fragment(frame.payload)
            .unwrap()
            .part,
        pulse_ipc::attach::Part::Data
    );
    handle.request_marker().await.unwrap();
    assert!(
        timeout(Duration::from_millis(40), wire.recv())
            .await
            .is_err()
    );
    handle.acknowledge(8, frame.sequence).unwrap();
    let marker = wire.recv().await.unwrap();
    let frame = pulse_ipc::decode(&marker).unwrap();
    let fragment = pulse_ipc::attach::decode_fragment(frame.payload).unwrap();
    assert_eq!(fragment.part, pulse_ipc::attach::Part::Marker);
    assert_eq!(fragment.offset, 5);
    assert_eq!(
        fragment.body,
        u64::to_le_bytes(session.state.borrow().revision)
    );
    handle.acknowledge(8, frame.sequence).unwrap();
    handle.detach();
    assert!(task.join().await.is_err());
    session.close();
    drop(driver);
    actor.join().await.unwrap();
}
#[tokio::test]
async fn normal_eof_is_ordered_and_requires_final_ack() {
    let (session, actor, driver) = session();
    let (handle, mut wire, task) = attach(&session, 8, config()).await.unwrap();
    bootstrap(&handle, &mut wire, 8).await;
    driver.output.send(vec![b'a', 0xf0, 0x9f]).await.unwrap();
    drop(driver);
    let mut bytes: Vec<u8> = Vec::new();
    let mut ended = false;
    while let Some(w) = timeout(Duration::from_secs(2), wire.recv()).await.unwrap() {
        let frame = pulse_ipc::decode(&w).unwrap();
        let p = pulse_ipc::attach::decode_fragment(frame.payload).unwrap();
        match p.part {
            pulse_ipc::attach::Part::Data => {
                assert!(!ended);
                bytes.extend(p.body);
            }
            pulse_ipc::attach::Part::End => {
                ended = true;
                assert_eq!(p.offset, 3);
            }
            _ => panic!("unexpected live part"),
        };
        handle.acknowledge(8, frame.sequence).unwrap();
    }
    let receipt = task.join().await.unwrap();
    assert!(ended);
    assert_eq!(bytes, [b'a', 0xf0, 0x9f]);
    assert_eq!(receipt.boundary.parsed_bytes, 3);
    assert!(receipt.peak_credit_bytes <= config().bytes);
    assert_eq!(actor.join().await.unwrap().lifecycle, Lifecycle::Ended);
}
#[tokio::test]
async fn no_ack_freezes_actor_then_timeout_releases_without_closing() {
    let (session, actor, driver) = session();
    let (handle, mut wire, task) = attach(
        &session,
        9,
        AttachmentConfig {
            ack_timeout: Duration::from_millis(600),
            ..config()
        },
    )
    .await
    .unwrap();
    bootstrap(&handle, &mut wire, 9).await;
    let source = driver.output.clone();
    let producer = tokio::spawn(async move {
        for _ in 0..40 {
            source.send(vec![b'x'; 65536]).await.unwrap();
        }
    });
    let first = timeout(Duration::from_secs(1), wire.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        pulse_ipc::attach::decode_fragment(pulse_ipc::decode(&first).unwrap().payload)
            .unwrap()
            .part,
        pulse_ipc::attach::Part::Data
    );
    sleep(Duration::from_millis(80)).await;
    let paused = session.state.borrow().parsed_bytes;
    sleep(Duration::from_millis(80)).await;
    assert_eq!(session.state.borrow().parsed_bytes, paused);
    assert!(paused < 40 * 65536);
    session.inspect().await.unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(2), task.join()).await.unwrap(),
        Err(AttachmentError::AckTimeout)
    ));
    timeout(Duration::from_secs(2), producer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.state.borrow().lifecycle, Lifecycle::Running);
    let mut state = session.state.clone();
    timeout(Duration::from_secs(2), async {
        while state.borrow().parsed_bytes < 40 * 65536 {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    session.close();
    drop(driver);
    actor.join().await.unwrap();
}
#[tokio::test]
async fn invalid_ack_fails_closed_and_new_binding_survives() {
    let (session, actor, driver) = session();
    let (old, mut wire, task) = attach(&session, 10, config()).await.unwrap();
    let frame = wire.recv().await.unwrap();
    old.acknowledge(11, pulse_ipc::decode(&frame).unwrap().sequence)
        .unwrap();
    assert!(matches!(
        task.join().await,
        Err(AttachmentError::Protocol(AttachError::Generation))
    ));
    let (new, mut wire, task) = attach(&session, 11, config()).await.unwrap();
    bootstrap(&new, &mut wire, 11).await;
    new.detach();
    assert!(matches!(task.join().await, Err(AttachmentError::Detached)));
    assert_eq!(session.state.borrow().lifecycle, Lifecycle::Running);
    session.close();
    drop(driver);
    actor.join().await.unwrap();
}
#[tokio::test]
async fn reconfiguration_and_drop_revoke_without_fake_end() {
    let (session, actor, driver) = session();
    let (handle, mut wire, task) = attach(&session, 12, config()).await.unwrap();
    bootstrap(&handle, &mut wire, 12).await;
    session.set_cell_pixels(Some((8, 16))).await.unwrap();
    assert!(matches!(
        task.join().await,
        Err(AttachmentError::Invalidated(StreamEnd::Reconfigured))
    ));
    assert!(wire.recv().await.is_none());
    let (_, wire, task) = attach(&session, 13, config()).await.unwrap();
    drop(wire);
    assert!(matches!(task.join().await, Err(AttachmentError::Detached)));
    assert_eq!(session.state.borrow().lifecycle, Lifecycle::Running);
    session.close();
    drop(driver);
    actor.join().await.unwrap();
}
#[tokio::test]
async fn replaced_generation_rejects_old_ack_and_new_binding_progresses() {
    let (session, actor, driver) = session();
    let (old, mut old_wire, old_task) = attach(&session, 21, config()).await.unwrap();
    let old_frame = old_wire.recv().await.unwrap();
    let (new, mut wire, new_task) = attach(&session, 22, config()).await.unwrap();
    assert!(matches!(
        old_task.join().await,
        Err(AttachmentError::Invalidated(StreamEnd::Replaced))
    ));
    assert!(matches!(
        old.acknowledge(21, pulse_ipc::decode(&old_frame).unwrap().sequence),
        Err(AttachmentError::Detached)
    ));
    bootstrap(&new, &mut wire, 22).await;
    new.detach();
    assert!(matches!(
        new_task.join().await,
        Err(AttachmentError::Detached)
    ));
    session.close();
    drop(driver);
    actor.join().await.unwrap();
}
#[tokio::test]
async fn duplicate_ack_and_missing_final_ack_are_not_completion() {
    let (session, actor, driver) = session();
    let (ack, mut wire, task) = attach(&session, 23, config()).await.unwrap();
    let first = wire.recv().await.unwrap();
    let seq = pulse_ipc::decode(&first).unwrap().sequence;
    ack.acknowledge(23, seq).unwrap();
    let second = wire.recv().await.unwrap();
    ack.acknowledge(23, seq).unwrap();
    assert!(matches!(
        task.join().await,
        Err(AttachmentError::Protocol(AttachError::Flow(
            FlowError::InvalidAck
        )))
    ));
    drop(second);
    session.close();
    drop(driver);
    actor.join().await.unwrap();
    let (session, actor, driver) = self::session();
    let (ack, mut wire, task) = attach(
        &session,
        24,
        AttachmentConfig {
            ack_timeout: Duration::from_millis(150),
            ..config()
        },
    )
    .await
    .unwrap();
    bootstrap(&ack, &mut wire, 24).await;
    drop(driver);
    let end = timeout(Duration::from_secs(1), wire.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        pulse_ipc::attach::decode_fragment(pulse_ipc::decode(&end).unwrap().payload)
            .unwrap()
            .part,
        pulse_ipc::attach::Part::End
    );
    assert!(matches!(
        task.join().await,
        Err(AttachmentError::AckTimeout)
    ));
    assert_eq!(actor.join().await.unwrap().lifecycle, Lifecycle::Ended);
}
#[tokio::test]
async fn invalid_limits_preserve_current_lease() {
    let (session, actor, driver) = session();
    let old = session
        .diagnostic_subscribe(0, 0, 1024 * 1024)
        .await
        .unwrap();
    for config in [
        AttachmentConfig {
            frames: 9,
            ..config()
        },
        AttachmentConfig {
            bytes: 28,
            ..config()
        },
        AttachmentConfig {
            ack_timeout: Duration::ZERO,
            ..config()
        },
    ] {
        assert!(matches!(
            attach(&session, 1, config).await,
            Err(AttachmentError::Limits)
        ));
    }
    assert!(matches!(
        attach(&session, 0, config()).await,
        Err(AttachmentError::Protocol(AttachError::Generation))
    ));
    assert_eq!(old.status.borrow().end, StreamEnd::Open);
    session.close();
    drop(driver);
    actor.join().await.unwrap();
}
