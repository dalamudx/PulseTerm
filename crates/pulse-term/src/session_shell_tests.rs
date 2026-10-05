use super::*;
use crate::shell_markers::{MAX_SHELL_MARKERS, ShellMarker};
use tokio::time::{Duration, timeout};
type Error = Box<dyn std::error::Error>;
async fn fixture() -> Result<(SessionHandle, SessionTask, DriverEndpoint, SessionStream), Error> {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 8)?, 0, transport);
    let stream = handle.diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES).await?;
    Ok((handle, task, driver, stream))
}
#[tokio::test]
async fn sampled_state_is_not_a_marker_coordinate_and_drain_does_not_write_or_flush()
-> Result<(), Error> {
    let (handle, task, mut driver, mut stream) = fixture().await?;
    let (_cancel, rx) = watch::channel(false);
    driver.output.send(b"\x1b]133;A\x1b".to_vec()).await?;
    timeout(Duration::from_secs(1), stream.output.recv())
        .await?
        .ok_or("missing data")?;
    assert!(
        handle
            .diagnostic_shell_markers(stream.lease_id, rx.clone())
            .await?
            .hints
            .markers
            .is_empty()
    );
    driver
        .output
        .send(b"\\\x1b]133;D;1\x07text".to_vec())
        .await?;
    timeout(Duration::from_secs(1), stream.output.recv())
        .await?
        .ok_or("missing tail")?;
    let before = handle.state.borrow().clone();
    let sampled = handle
        .diagnostic_shell_markers(stream.lease_id, rx.clone())
        .await?;
    assert_eq!(sampled.sampled_at, before);
    assert_eq!(*handle.state.borrow(), before);
    assert_eq!(
        sampled.hints.markers,
        [
            ShellMarker::PromptStart,
            ShellMarker::CommandEnd {
                exit_status: Some(1)
            }
        ]
    );
    assert!(driver.input.try_recv().is_err());
    assert!(stream.output.try_recv().is_err());
    assert!(
        handle
            .diagnostic_shell_markers(stream.lease_id, rx)
            .await?
            .hints
            .markers
            .is_empty()
    );
    handle.close();
    drop(driver);
    task.join().await?;
    Ok(())
}
#[tokio::test]
async fn old_cancelled_abandoned_and_detached_requests_cannot_consume_current_hints()
-> Result<(), Error> {
    let (handle, task, driver, old) = fixture().await?;
    let mut current = handle.diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES).await?;
    let (cancel, rx) = watch::channel(false);
    driver.output.send(b"\x1b]133;B\x07".to_vec()).await?;
    timeout(Duration::from_secs(1), current.output.recv())
        .await?
        .ok_or("missing data")?;
    assert!(matches!(
        handle
            .diagnostic_shell_markers(old.lease_id, rx.clone())
            .await,
        Err(SessionError::StaleLease)
    ));
    let (reply, result) = oneshot::channel();
    handle.enqueue(Command::ShellMarkers {
        lease_id: current.lease_id,
        cancelled: rx.clone(),
        reply,
    })?;
    cancel.send_replace(true);
    assert!(matches!(result.await?, Err(SessionError::StaleLease)));
    cancel.send_replace(false);
    let (reply, result) = oneshot::channel();
    drop(result);
    handle.enqueue(Command::ShellMarkers {
        lease_id: current.lease_id,
        cancelled: rx.clone(),
        reply,
    })?;
    handle.inspect().await?;
    assert_eq!(
        handle
            .diagnostic_shell_markers(current.lease_id, rx.clone())
            .await?
            .hints
            .markers,
        [ShellMarker::PromptEnd]
    );
    let id = current.lease_id;
    drop(current);
    assert!(matches!(
        handle.diagnostic_shell_markers(id, rx).await,
        Err(SessionError::StaleLease)
    ));
    handle.close();
    drop(driver);
    task.join().await?;
    Ok(())
}
#[tokio::test]
async fn full_ordered_queue_still_allows_bounded_hint_drain() -> Result<(), Error> {
    let (handle, task, driver, mut stream) = fixture().await?;
    let (_cancel, rx) = watch::channel(false);
    driver.output.send(b"\x1b]133;C\x07".to_vec()).await?;
    timeout(Duration::from_secs(1), stream.output.recv())
        .await?
        .ok_or("missing data")?;
    for _ in 0..QUEUE {
        handle.diagnostic_marker(stream.lease_id).await?;
    }
    let before = handle.state.borrow().clone();
    let sampled = timeout(
        Duration::from_secs(1),
        handle.diagnostic_shell_markers(stream.lease_id, rx),
    )
    .await??;
    assert_eq!(sampled.hints.markers, [ShellMarker::CommandStart]);
    assert_eq!(*handle.state.borrow(), before);
    assert!(matches!(
        stream.output.recv().await,
        Some(StreamRecord::Marker(_))
    ));
    handle.close();
    drop(driver);
    task.join().await?;
    Ok(())
}
#[tokio::test]
async fn overflow_counts_are_returned_once_without_inventing_eof_markers() -> Result<(), Error> {
    let (handle, task, driver, mut stream) = fixture().await?;
    let (_cancel, rx) = watch::channel(false);
    driver
        .output
        .send(b"\x1b]133;A\x07".repeat(MAX_SHELL_MARKERS + 7))
        .await?;
    timeout(Duration::from_secs(1), stream.output.recv())
        .await?
        .ok_or("missing data")?;
    let sampled = handle
        .diagnostic_shell_markers(stream.lease_id, rx.clone())
        .await?;
    assert_eq!(sampled.hints.markers.len(), MAX_SHELL_MARKERS);
    assert_eq!(sampled.hints.dropped, 7);
    let next = handle.diagnostic_shell_markers(stream.lease_id, rx).await?;
    assert!(next.hints.markers.is_empty());
    assert_eq!(next.hints.dropped, 0);
    handle.close();
    drop(driver);
    task.join().await?;
    Ok(())
}
#[test]
fn failed_reply_restores_hint_order_and_overflow_count() -> Result<(), Error> {
    let size = TermSize::new(32, 8)?;
    let mut terminal = TerminalProbe::new(size, 0);
    let mut bytes = b"\x1b]133;A\x07\x1b]133;B\x07".repeat(MAX_SHELL_MARKERS / 2);
    bytes.extend(b"\x1b]133;C\x07".repeat(7));
    terminal.feed(&bytes);
    let hints = terminal.take_shell_markers();
    let expected = hints.markers.clone();
    let sampled_at = SessionState {
        lifecycle: Lifecycle::Running,
        size,
        parsed_bytes: 0,
        utf8_bytes: 0,
        source_is_utf8: true,
        revision: 0,
    };
    let (reply, result) = oneshot::channel();
    drop(result);
    deliver(
        &mut terminal,
        reply,
        Ok(SessionShellMarkers { sampled_at, hints }),
    );
    let restored = terminal.take_shell_markers();
    assert_eq!(restored.markers, expected);
    assert_eq!(restored.dropped, 7);
    Ok(())
}
#[tokio::test]
async fn draining_hints_does_not_clear_actual_reconciliation_risk() -> Result<(), Error> {
    let (handle, task, driver, mut stream) = fixture().await?;
    let (_cancel, rx) = watch::channel(false);
    driver
        .output
        .send(b"q\x1b[2b\x1b[L\x1b]133;A\x07".to_vec())
        .await?;
    timeout(Duration::from_secs(1), stream.output.recv())
        .await?
        .ok_or("missing data")?;
    let before = handle.diagnostic_digest().await?;
    assert_ne!(before.risk, 0);
    assert_eq!(
        handle
            .diagnostic_shell_markers(stream.lease_id, rx)
            .await?
            .hints
            .markers,
        [ShellMarker::PromptStart]
    );
    let after = handle.diagnostic_digest().await?;
    assert_eq!(before.risk, after.risk);
    assert_eq!(before.boundary, after.boundary);
    assert_eq!(before.digest, after.digest);
    handle.close();
    drop(driver);
    task.join().await?;
    Ok(())
}
