#![allow(clippy::unwrap_used)]
use super::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
fn pending<F: Future>(future: Pin<&mut F>) {
    assert!(matches!(
        future.poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}
fn fixture(basis: ResizeBasis) -> (SessionHandle, SessionTask, DriverEndpoint) {
    let (transport, driver) = ChannelTransport::bounded_driver_with_resize_basis(basis);
    let (handle, task) = spawn_session(TermSize::new(32, 8).unwrap(), 64, transport);
    (handle, task, driver)
}
async fn finish(handle: SessionHandle, task: SessionTask, driver: DriverEndpoint) {
    handle.close();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn local_and_ssh_receipts_wait_for_driver_then_commit_the_model() {
    for basis in [
        ResizeBasis::LocalNativeConfirmed,
        ResizeBasis::SshWindowRequestAdmitted,
    ] {
        let (handle, task, mut driver) = fixture(basis);
        let before = handle.state.borrow().size;
        let target = TermSize::new(60, 12).unwrap();
        let mut request = Box::pin(handle.resize_with_receipt(target));
        pending(request.as_mut());
        let TransportWrite::Resize(size, ack) = driver.input.recv().await.unwrap() else {
            panic!("expected resize")
        };
        assert_eq!(size, target);
        assert_eq!(handle.state.borrow().size, before);
        pending(request.as_mut());
        ack.send(Ok(())).unwrap();
        assert_eq!(
            request.await.unwrap(),
            ResizeReceipt {
                size: target,
                basis: Some(basis)
            }
        );
        assert_eq!(handle.state.borrow().size, target);
        finish(handle, task, driver).await;
    }
}
#[tokio::test]
async fn ssh_layout_invalidates_old_lease_without_granting_input() {
    let (handle, task, mut driver) = fixture(ResizeBasis::SshWindowRequestAdmitted);
    let stream = handle.diagnostic_subscribe(0, 1024, 8192).await.unwrap();
    let target = TermSize::new(60, 12).unwrap();
    let (_live, cancelled) = watch::channel(false);
    let mut request =
        Box::pin(handle.resize_for_lease_with_receipt(stream.lease_id, cancelled.clone(), target));
    pending(request.as_mut());
    let TransportWrite::Resize(_, ack) = driver.input.recv().await.unwrap() else {
        panic!("expected resize")
    };
    ack.send(Ok(())).unwrap();
    assert_eq!(
        request.await.unwrap().basis,
        Some(ResizeBasis::SshWindowRequestAdmitted)
    );
    assert!(
        handle
            .input_for_lease(stream.lease_id, cancelled.clone(), b"old".to_vec())
            .await
            .is_err()
    );
    assert!(driver.input.try_recv().is_err());
    drop(stream);
    finish(handle, task, driver).await;
}
#[tokio::test]
async fn unchanged_bound_layout_is_not_a_new_transport_receipt() {
    let (handle, task, mut driver) = fixture(ResizeBasis::SshWindowRequestAdmitted);
    let stream = handle.diagnostic_subscribe(0, 1024, 8192).await.unwrap();
    let size = handle.state.borrow().size;
    let (_live, cancelled) = watch::channel(false);
    assert_eq!(
        handle
            .resize_for_lease_with_receipt(stream.lease_id, cancelled.clone(), size)
            .await
            .unwrap(),
        ResizeReceipt { size, basis: None }
    );
    assert!(driver.input.try_recv().is_err());
    drop(stream);
    finish(handle, task, driver).await;
}
#[tokio::test]
async fn failed_ssh_admission_never_changes_grid_or_returns_success() {
    let (handle, task, mut driver) = fixture(ResizeBasis::SshWindowRequestAdmitted);
    let before = handle.state.borrow().size;
    let mut request = Box::pin(handle.resize_with_receipt(TermSize::new(60, 12).unwrap()));
    pending(request.as_mut());
    let TransportWrite::Resize(_, ack) = driver.input.recv().await.unwrap() else {
        panic!("expected resize")
    };
    ack.send(Err(SessionError::Transport)).unwrap();
    assert_eq!(request.await, Err(SessionError::Transport));
    assert_eq!(handle.state.borrow().size, before);
    finish(handle, task, driver).await;
}
#[tokio::test]
async fn lost_waiter_does_not_undo_admitted_layout_or_fabricate_a_receipt() {
    let (handle, task, mut driver) = fixture(ResizeBasis::SshWindowRequestAdmitted);
    let target = TermSize::new(60, 12).unwrap();
    let mut request = Box::pin(handle.resize_with_receipt(target));
    pending(request.as_mut());
    let TransportWrite::Resize(_, ack) = driver.input.recv().await.unwrap() else {
        panic!("expected resize")
    };
    drop(request);
    let mut state = handle.state.clone();
    state.borrow_and_update();
    ack.send(Ok(())).unwrap();
    state.changed().await.unwrap();
    assert_eq!(state.borrow().size, target);
    finish(handle, task, driver).await;
}
#[tokio::test]
async fn retired_lease_cannot_dispatch_ssh_window_request() {
    let (handle, task, mut driver) = fixture(ResizeBasis::SshWindowRequestAdmitted);
    let stream = handle.diagnostic_subscribe(0, 1024, 8192).await.unwrap();
    let (_closed, cancelled) = watch::channel(true);
    assert_eq!(
        handle
            .resize_for_lease_with_receipt(
                stream.lease_id,
                cancelled,
                TermSize::new(60, 12).unwrap()
            )
            .await,
        Err(SessionError::StaleLease)
    );
    assert!(driver.input.try_recv().is_err());
    drop(stream);
    finish(handle, task, driver).await;
}
