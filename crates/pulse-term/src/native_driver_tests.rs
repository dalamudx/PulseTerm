#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn missing_native_resize_ack_has_a_finite_deadline() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 8).unwrap(), 0, transport);
    let caller = handle.clone();
    let request = tokio::spawn(async move { caller.resize(TermSize::new(44, 12).unwrap()).await });
    let TransportWrite::Resize(_, reply) = driver.input.recv().await.unwrap() else {
        panic!("not resize")
    };
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(6), request)
            .await
            .unwrap()
            .unwrap(),
        Err(SessionError::Transport)
    );
    assert_eq!(task.join().await.unwrap().lifecycle, Lifecycle::Failed);
    assert!(reply.send(Ok(())).is_err());
}
#[tokio::test]
async fn simultaneous_native_failure_and_eof_never_report_ended() {
    for _ in 0..32 {
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (_handle, task) = spawn_session(TermSize::new(32, 8).unwrap(), 0, transport);
        driver.failed.send_replace(true);
        drop(driver);
        assert_eq!(task.join().await.unwrap().lifecycle, Lifecycle::Failed);
    }
}

#[tokio::test]
async fn native_resize_updates_only_after_ack_and_failure_keeps_state() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let size = TermSize::new(32, 8).unwrap();
    let (handle, task) = spawn_session(size, 0, transport);
    let desired = TermSize::new(44, 12).unwrap();
    for success in [false, true] {
        let caller = handle.clone();
        let request = tokio::spawn(async move { caller.resize(desired).await });
        let TransportWrite::Resize(dimensions, reply) = driver.input.recv().await.unwrap() else {
            panic!("not resize")
        };
        assert_eq!(dimensions, desired);
        assert_eq!(handle.state.borrow().size, size);
        let _ = reply.send(if success {
            Ok(())
        } else {
            Err(SessionError::Transport)
        });
        assert_eq!(
            request.await.unwrap(),
            if success {
                Ok(())
            } else {
                Err(SessionError::Transport)
            }
        );
    }
    assert_eq!(handle.state.borrow().size, desired);
    assert_eq!(handle.state.borrow().revision, 1);
    handle.close();
    task.join().await.unwrap();
    assert!(*driver.stop.borrow());
}
#[tokio::test]
async fn cancelled_pending_native_resize_is_not_accepted() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 8).unwrap(), 0, transport);
    let caller = handle.clone();
    let request = tokio::spawn(async move { caller.resize(TermSize::new(44, 12).unwrap()).await });
    let TransportWrite::Resize(_, reply) = driver.input.recv().await.unwrap() else {
        panic!("not resize")
    };
    handle.close();
    assert_eq!(task.join().await.unwrap().lifecycle, Lifecycle::Closed);
    assert_eq!(request.await.unwrap(), Err(SessionError::Closed));
    assert!(reply.send(Ok(())).is_err());
}
#[tokio::test]
async fn fatal_native_io_is_not_an_eof() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (_handle, task) = spawn_session(TermSize::new(32, 8).unwrap(), 0, transport);
    driver.failed.send_replace(true);
    assert_eq!(task.join().await.unwrap().lifecycle, Lifecycle::Failed);
}
#[tokio::test]
async fn normal_native_endpoint_eof_is_not_a_failure() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (_handle, task) = spawn_session(TermSize::new(32, 8).unwrap(), 0, transport);
    drop(driver);
    assert_eq!(task.join().await.unwrap().lifecycle, Lifecycle::Ended);
}
