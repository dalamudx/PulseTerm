use super::*;
use crate::shell_markers::{MAX_SHELL_MARKERS, ShellMarker};
use tokio::time::{Duration, timeout};
type Error = Box<dyn std::error::Error>;
#[tokio::test]
async fn eof_preserves_only_undrained_complete_hints_and_no_invented_exit() -> Result<(), Error> {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 8)?, 0, transport);
    let mut stream = handle.diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES).await?;
    driver.output.send(b"\x1b]133;A\x07".to_vec()).await?;
    timeout(Duration::from_secs(1), stream.output.recv())
        .await?
        .ok_or("missing data")?;
    let (_cancel, rx) = watch::channel(false);
    assert_eq!(
        handle
            .diagnostic_shell_markers(stream.lease_id, rx.clone())
            .await?
            .hints
            .markers,
        [ShellMarker::PromptStart]
    );
    driver
        .output
        .send(b"\x1b]133;D;7\x1b\\\x1b]133;C\x1b".to_vec())
        .await?;
    drop(driver);
    while timeout(Duration::from_secs(1), stream.output.recv())
        .await?
        .is_some()
    {}
    let receipt = timeout(Duration::from_secs(1), task.join_with_receipt()).await??;
    assert_eq!(receipt.state.lifecycle, Lifecycle::Ended);
    assert_eq!(
        receipt.shell_markers.markers,
        [ShellMarker::CommandEnd {
            exit_status: Some(7)
        }]
    );
    assert_eq!(receipt.shell_markers.dropped, 0);
    assert!(matches!(
        handle.diagnostic_shell_markers(stream.lease_id, rx).await,
        Err(SessionError::Closed)
    ));
    Ok(())
}
#[tokio::test]
async fn close_retains_bounded_leftovers_without_synthetic_command_end() -> Result<(), Error> {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 8)?, 0, transport);
    let mut stream = handle.diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES).await?;
    driver
        .output
        .send(b"\x1b]133;B\x07".repeat(MAX_SHELL_MARKERS + 9))
        .await?;
    timeout(Duration::from_secs(1), stream.output.recv())
        .await?
        .ok_or("missing data")?;
    handle.close();
    let receipt = timeout(Duration::from_secs(1), task.join_with_receipt()).await??;
    drop(driver);
    assert_eq!(receipt.state.lifecycle, Lifecycle::Closed);
    assert_eq!(
        receipt.shell_markers.markers,
        vec![ShellMarker::PromptEnd; MAX_SHELL_MARKERS]
    );
    assert_eq!(receipt.shell_markers.dropped, 9);
    Ok(())
}
