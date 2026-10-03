use super::*;
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn cancelled_subscription_does_not_revoke_current_lease() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 0, transport);
    let old = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let (reply, request) = oneshot::channel();
    drop(request);
    handle
        .enqueue(Command::Subscribe {
            history_rows: 0,
            history_budget: 0,
            restore_budget: MAX_RESTORE_BYTES,
            reply,
        })
        .unwrap();
    handle.inspect().await.unwrap();
    assert_eq!(old.status.borrow().end, StreamEnd::Open);
    handle.close();
    task.join().await.unwrap();
}
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn sync_timeout_is_an_ordered_fence_not_synthetic_output_bytes() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 0, transport);
    let mut view = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let input = b"\x1b[?2026hbuffered";
    handle.input(input.to_vec()).await.unwrap();
    let record = timeout(Duration::from_secs(1), view.output.recv())
        .await
        .unwrap()
        .unwrap();
    let boundary = match record {
        StreamRecord::Data {
            start,
            bytes,
            boundary,
        } => {
            assert_eq!(start, 0);
            assert_eq!(bytes, input);
            boundary
        }
        _ => panic!("data expected"),
    };
    let fence = timeout(Duration::from_secs(2), view.output.recv())
        .await
        .unwrap()
        .unwrap();
    match fence {
        StreamRecord::Fence(after) => {
            assert_eq!(after.parsed_bytes, boundary.parsed_bytes);
            assert!(after.revision > boundary.revision);
        }
        _ => panic!("fence expected"),
    }
    handle.close();
    task.join().await.unwrap();
}
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn eof_closes_output_and_preserves_final_utf8_replacement() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(size(), 0, transport);
    let mut view = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    driver.output.send(vec![b'a', 0xf0, 0x9f]).await.unwrap();
    drop(driver);
    let mut bytes = Vec::new();
    while let Some(record) = timeout(Duration::from_secs(1), view.output.recv())
        .await
        .unwrap()
    {
        if let StreamRecord::Data { bytes: part, .. } = record {
            bytes.extend(part);
        }
    }
    let state = task.join().await.unwrap();
    assert_eq!(state.lifecycle, Lifecycle::Ended);
    assert_eq!(
        view.status.borrow().end,
        StreamEnd::Session(Lifecycle::Ended)
    );
    let mut replay = TerminalProbe::new(size(), 0);
    replay.feed(&view.bootstrap.restore);
    replay.feed(&bytes);
    replay.finish_input();
    assert_eq!(replay.screen().cells[0][1].text, "�");
    assert_eq!(state.parsed_bytes, 3);
}
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn pending_resize_refuses_new_lease_and_failed_resize_keeps_old() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(size(), 0, transport);
    let old = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let caller = handle.clone();
    let resize = tokio::spawn(async move { caller.resize(TermSize::new(40, 9).unwrap()).await });
    let TransportWrite::Resize(_, ack) = driver.input.recv().await.unwrap() else {
        panic!("resize expected")
    };
    assert!(matches!(
        handle.diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES).await,
        Err(SessionRestoreError::Session(SessionError::Busy))
    ));
    ack.send(Err(SessionError::Transport)).unwrap();
    assert_eq!(resize.await.unwrap(), Err(SessionError::Transport));
    assert_eq!(old.status.borrow().end, StreamEnd::Open);
    handle.close();
    task.join().await.unwrap();
}

use tokio::time::{Duration, timeout};
#[allow(clippy::unwrap_used)]
async fn parsed(handle: &SessionHandle, n: u64) {
    timeout(Duration::from_secs(2), async {
        let mut s = handle.state.clone();
        while s.borrow().parsed_bytes < n {
            s.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}
#[allow(clippy::unwrap_used)]
fn size() -> TermSize {
    TermSize::new(32, 8).unwrap()
}
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn snapshot_and_live_have_exact_boundary_with_partial_utf8() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 128, transport);
    let prefix = b"main\x1b[?1049halt\xf0\x9f";
    handle.input(prefix.to_vec()).await.unwrap();
    parsed(&handle, prefix.len() as u64).await;
    let mut view = handle
        .diagnostic_subscribe(20, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let mut restored = TerminalProbe::new(size(), 128);
    restored.feed(&view.bootstrap.history.bytes);
    restored.feed(&view.bootstrap.restore);
    let suffix = b"\x98\x80!\x1b[?1049lZ";
    handle.input(suffix.to_vec()).await.unwrap();
    let record = timeout(Duration::from_secs(2), view.output.recv())
        .await
        .unwrap()
        .unwrap();
    match record {
        StreamRecord::Data {
            start,
            bytes,
            boundary,
        } => {
            assert_eq!(start, view.bootstrap.boundary.parsed_bytes);
            assert_eq!(bytes, suffix);
            assert_eq!(boundary.parsed_bytes, start + bytes.len() as u64);
            restored.feed(&bytes);
        }
        _ => panic!("unexpected fence"),
    }
    assert_eq!(restored.screen(), handle.inspect().await.unwrap());
    handle.close();
    task.join().await.unwrap();
    assert_eq!(
        view.status.borrow().end,
        StreamEnd::Session(Lifecycle::Closed)
    );
}
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn full_subscriber_backpressures_without_blocking_control_or_close() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 0, transport);
    let view = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    for _ in 0..16 {
        handle.input(b"x".to_vec()).await.unwrap();
    }
    parsed(&handle, 8).await;
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(handle.state.borrow().parsed_bytes, 8);
    assert_eq!(view.output.len(), 8);
    timeout(Duration::from_secs(1), handle.inspect())
        .await
        .unwrap()
        .unwrap();
    handle.close();
    timeout(Duration::from_secs(1), task.join())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        view.status.borrow().end,
        StreamEnd::Session(Lifecycle::Closed)
    );
}
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn dropping_receiver_releases_pressure_and_registry_handle_keeps_session() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 0, transport);
    let view = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    for _ in 0..16 {
        handle.input(b"x".to_vec()).await.unwrap();
    }
    parsed(&handle, 8).await;
    let mut status = view.status.clone();
    drop(view);
    parsed(&handle, 16).await;
    timeout(Duration::from_secs(1), async {
        while status.borrow().end == StreamEnd::Open {
            status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(status.borrow().end, StreamEnd::Detached);
    assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Running);
    handle.close();
    task.join().await.unwrap();
}
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn invalid_new_capture_preserves_old_lease_and_success_replaces_monotonically() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 0, transport);
    let mut old = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert!(matches!(
        handle.diagnostic_subscribe(0, 0, 0).await,
        Err(SessionRestoreError::Gap(crate::RestoreGap::OutputTooLarge))
    ));
    assert_eq!(old.status.borrow().end, StreamEnd::Open);
    let new = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert!(new.lease_id > old.lease_id);
    assert_eq!(old.status.borrow().end, StreamEnd::Replaced);
    assert!(old.output.recv().await.is_none());
    handle.close();
    task.join().await.unwrap();
    assert_eq!(
        new.status.borrow().end,
        StreamEnd::Session(Lifecycle::Closed)
    );
}
#[tokio::test]
#[allow(clippy::unwrap_used)]
async fn successful_native_resize_invalidates_old_geometry() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 0, transport);
    let old = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let next = TermSize::new(40, 9).unwrap();
    handle.resize(next).await.unwrap();
    assert_eq!(old.status.borrow().end, StreamEnd::Reconfigured);
    assert_eq!(old.status.borrow().boundary.size, next);
    let new = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert_eq!(new.bootstrap.boundary.size, next);
    handle.close();
    task.join().await.unwrap();
}
