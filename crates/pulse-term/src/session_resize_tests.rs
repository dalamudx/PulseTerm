#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn subscription_api_waits_for_actor_commit_not_just_capture_reply() {
    let (transport, _driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(40, 12).unwrap(), 0, transport);
    let view = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let (commands, mut requests) = mpsc::channel(1);
    let (cancel, _mock_stop) = watch::channel(false);
    let mock = SessionHandle {
        commands,
        cancel,
        state: handle.state.clone(),
    };
    let mut capture = Box::pin(mock.diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES));
    let request = tokio::select! {r=&mut capture=>panic!("premature capture: {r:?}"),request=requests.recv()=>request.unwrap()};
    let (sent, committed) = oneshot::channel();
    if let Command::Subscribe { reply, .. } = request {
        assert!(
            reply
                .send(Ok(InstalledStream {
                    stream: view,
                    committed
                }))
                .is_ok()
        );
    } else {
        panic!("wrong command");
    }
    tokio::select! {r=&mut capture=>panic!("capture exposed before actor commit: {r:?}"),_=tokio::time::sleep(std::time::Duration::from_millis(10))=>{}}
    sent.send(()).unwrap();
    assert_eq!(capture.await.unwrap().lease_id, 1);
    handle.close();
    task.join().await.unwrap();
}

#[tokio::test]
async fn native_ack_alone_changes_dimensions_and_invalidates_bound_lease() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let original = TermSize::new(40, 12).unwrap();
    let (handle, task) = spawn_session(original, 0, transport);
    let view = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let (_cancel, signal) = watch::channel(false);
    let next = TermSize::new(60, 16).unwrap();
    handle
        .resize_for_lease(view.lease_id, signal.clone(), original)
        .await
        .unwrap();
    assert!(driver.input.try_recv().is_err());
    assert_eq!(view.status.borrow().end, StreamEnd::Open);
    let mut resize = Box::pin(handle.resize_for_lease(view.lease_id, signal.clone(), next));
    let event = tokio::select! {r=&mut resize=>panic!("premature resize: {r:?}"),event=driver.input.recv()=>event.unwrap()};
    assert_eq!(handle.state.borrow().size, original);
    assert_eq!(view.status.borrow().end, StreamEnd::Open);
    assert_eq!(
        handle
            .resize_for_lease(view.lease_id, signal.clone(), next)
            .await,
        Err(SessionError::Busy)
    );
    assert!(matches!(&event,TransportWrite::Resize(size,_) if *size==next));
    if let TransportWrite::Resize(_, ack) = event {
        ack.send(Ok(())).unwrap();
    }
    resize.await.unwrap();
    assert_eq!(handle.state.borrow().size, next);
    assert_eq!(view.status.borrow().end, StreamEnd::Reconfigured);
    assert_eq!(
        handle.resize_for_lease(view.lease_id, signal, next).await,
        Err(SessionError::StaleLease)
    );
    handle.close();
    task.join().await.unwrap();
}
#[tokio::test]
async fn cancelled_and_abandoned_geometry_never_reaches_native_and_error_preserves_grid() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let original = TermSize::new(40, 12).unwrap();
    let (handle, task) = spawn_session(original, 0, transport);
    let view = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let (cancel, signal) = watch::channel(false);
    let next = TermSize::new(60, 16).unwrap();
    cancel.send_replace(true);
    assert_eq!(
        handle
            .resize_for_lease(view.lease_id, signal.clone(), next)
            .await,
        Err(SessionError::StaleLease)
    );
    cancel.send_replace(false);
    let (reply, result) = oneshot::channel();
    drop(result);
    handle
        .enqueue(Command::BoundResize {
            lease_id: view.lease_id,
            cancelled: signal.clone(),
            size: next,
            reply,
        })
        .unwrap();
    handle.inspect().await.unwrap();
    assert!(driver.input.try_recv().is_err());
    let mut resize = Box::pin(handle.resize_for_lease(view.lease_id, signal, next));
    let event = tokio::select! {r=&mut resize=>panic!("premature resize: {r:?}"),event=driver.input.recv()=>event.unwrap()};
    if let TransportWrite::Resize(_, ack) = event {
        ack.send(Err(SessionError::Transport)).unwrap();
    }
    assert_eq!(resize.await, Err(SessionError::Transport));
    assert_eq!(handle.state.borrow().size, original);
    assert_eq!(view.status.borrow().end, StreamEnd::Open);
    handle.close();
    task.join().await.unwrap();
}
