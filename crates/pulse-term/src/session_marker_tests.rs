#![allow(clippy::unwrap_used)]
use super::*;
use tokio::time::{Duration, timeout};
fn size() -> TermSize {
    TermSize::new(32, 8).unwrap()
}
#[tokio::test]
async fn marker_preserves_partial_parser_and_utf8_with_same_boundary() {
    for (input, tail) in [
        (b"\x1b[31".as_slice(), b"mZ".as_slice()),
        (b"x\xe4\xb8".as_slice(), b"\xadZ".as_slice()),
    ] {
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = spawn_session(size(), 0, transport);
        let mut stream = handle
            .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
            .await
            .unwrap();
        driver.output.send(input.to_vec()).await.unwrap();
        let data = timeout(Duration::from_secs(1), stream.output.recv())
            .await
            .unwrap()
            .unwrap();
        let StreamRecord::Data {
            boundary, bytes, ..
        } = data
        else {
            panic!("data first");
        };
        handle.diagnostic_marker(stream.lease_id).await.unwrap();
        let marker = stream.output.recv().await.unwrap();
        let StreamRecord::Marker(at) = marker else {
            panic!("marker expected");
        };
        assert_eq!(at, boundary);
        assert_eq!(*handle.state.borrow(), boundary);
        driver.output.send(tail.to_vec()).await.unwrap();
        let StreamRecord::Data { bytes: tail, .. } = stream.output.recv().await.unwrap() else {
            panic!("data expected");
        };
        let mut replay = TerminalProbe::new(size(), 0);
        replay.feed(&stream.bootstrap.restore);
        replay.feed(&bytes);
        replay.feed(&tail);
        assert_eq!(handle.inspect().await.unwrap(), replay.screen());
        handle.close();
        drop(driver);
        task.join().await.unwrap();
    }
}
#[tokio::test]
async fn full_marker_queue_returns_busy_and_controls_stay_responsive() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(size(), 0, transport);
    let mut stream = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    for _ in 0..QUEUE {
        handle.diagnostic_marker(stream.lease_id).await.unwrap();
    }
    assert_eq!(
        handle.diagnostic_marker(stream.lease_id).await,
        Err(SessionError::Busy)
    );
    timeout(Duration::from_secs(1), handle.inspect())
        .await
        .unwrap()
        .unwrap();
    for _ in 0..QUEUE {
        assert!(matches!(
            stream.output.recv().await,
            Some(StreamRecord::Marker(_))
        ));
    }
    handle.diagnostic_marker(stream.lease_id).await.unwrap();
    handle.close();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn old_cancelled_and_detached_marker_requests_cannot_enter_new_lease() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(size(), 0, transport);
    let old = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let mut new = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert_eq!(
        handle.diagnostic_marker(old.lease_id).await,
        Err(SessionError::StaleLease)
    );
    let (reply, result) = oneshot::channel();
    drop(result);
    handle
        .enqueue(Command::Marker(new.lease_id, reply))
        .unwrap();
    handle.inspect().await.unwrap();
    assert!(matches!(
        new.output.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    let serial = new.lease_id;
    drop(new);
    assert_eq!(
        handle.diagnostic_marker(serial).await,
        Err(SessionError::StaleLease)
    );
    handle.close();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn marker_does_not_terminate_synchronized_updates() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(size(), 0, transport);
    let mut stream = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    driver
        .output
        .send(b"\x1b[?2026hheld".to_vec())
        .await
        .unwrap();
    assert!(matches!(
        stream.output.recv().await,
        Some(StreamRecord::Data { .. })
    ));
    assert_eq!(
        handle.diagnostic_marker(stream.lease_id).await,
        Err(SessionError::Busy)
    );
    assert!(matches!(
        stream.output.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    handle.close();
    drop(driver);
    task.join().await.unwrap();
}
