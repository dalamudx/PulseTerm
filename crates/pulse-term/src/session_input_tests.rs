#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn input_is_validated_on_actor_turn_against_current_lease_and_cancel() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 8).unwrap(), 0, transport);
    let first = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let (cancel, signal) = watch::channel(false);
    handle
        .input_for_lease(first.lease_id, signal.clone(), "中文🌍".as_bytes().to_vec())
        .await
        .unwrap();
    assert!(
        matches!(driver.input.recv().await,Some(TransportWrite::Input(b)) if b=="中文🌍".as_bytes())
    );
    let second = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert_eq!(
        handle
            .input_for_lease(first.lease_id, signal.clone(), b"x".to_vec())
            .await,
        Err(SessionError::StaleLease)
    );
    cancel.send_replace(true);
    assert_eq!(
        handle
            .input_for_lease(second.lease_id, signal, b"y".to_vec())
            .await,
        Err(SessionError::StaleLease)
    );
    assert!(driver.input.try_recv().is_err());
    handle.close();
    task.join().await.unwrap();
}
#[tokio::test]
async fn abandoned_input_never_reaches_transport_and_bounds_fail_before_enqueue() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 8).unwrap(), 0, transport);
    let view = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let (_cancel, signal) = watch::channel(false);
    let (reply, result) = oneshot::channel();
    drop(result);
    handle
        .enqueue(Command::BoundInput {
            lease_id: view.lease_id,
            cancelled: signal.clone(),
            bytes: b"abandoned".to_vec(),
            reply,
        })
        .unwrap();
    handle.inspect().await.unwrap();
    assert!(driver.input.try_recv().is_err());
    for bytes in [Vec::new(), vec![b'x'; MAX_CHUNK + 1]] {
        assert_eq!(
            handle
                .input_for_lease(view.lease_id, signal.clone(), bytes)
                .await,
            Err(SessionError::ChunkTooLarge)
        );
    }
    drop(view);
    assert_eq!(
        handle.input_for_lease(1, signal, b"late".to_vec()).await,
        Err(SessionError::StaleLease)
    );
    handle.close();
    task.join().await.unwrap();
}
