#![allow(clippy::unwrap_used)]
use super::*;
use tokio::time::{Duration, timeout};
#[tokio::test]
async fn exact_eof_replays_immutable_history_palette_and_decoder_finalization_without_actor() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(20, 4).unwrap(), 32, transport);
    let palette = crate::QueryPalette {
        background: [11, 22, 33],
        ..Default::default()
    };
    handle.set_query_palette(palette.clone()).await.unwrap();
    let mut input = b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nlast".to_vec();
    input.extend_from_slice(&[0xe4, 0xb8]);
    let source = input.len() as u64;
    driver.output.send(input).await.unwrap();
    drop(driver.output);
    let receipt = timeout(Duration::from_secs(2), task.join_with_receipt())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state.lifecycle, Lifecycle::Ended);
    let mut a = handle
        .diagnostic_subscribe(crate::MAX_RECENT_ROWS, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let b = handle
        .diagnostic_subscribe(crate::MAX_RECENT_ROWS, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert_eq!(a.bootstrap.boundary, receipt.state);
    assert_eq!(a.bootstrap.boundary.parsed_bytes, source);
    assert_eq!(a.bootstrap.palette.background, palette.background);
    assert!(a.bootstrap.history.retained_rows > 0);
    assert!(
        String::from_utf8(a.bootstrap.restore.clone())
            .unwrap()
            .contains('\u{fffd}')
    );
    assert_eq!(a.bootstrap.restore, b.bootstrap.restore);
    assert_eq!(a.status.borrow().end, StreamEnd::Session(Lifecycle::Ended));
    assert!(a.output.recv().await.is_none());
    assert!(handle.input(b"no".to_vec()).await.is_err());
    assert!(handle.resize(TermSize::new(30, 5).unwrap()).await.is_err());
    a.bootstrap.restore.clear();
    a.bootstrap.palette.background = [0; 3];
    let c = handle
        .diagnostic_subscribe(crate::MAX_RECENT_ROWS, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert_eq!(c.bootstrap.restore, b.bootstrap.restore);
    assert_eq!(c.bootstrap.palette.background, palette.background);
    for (rows, hist, screen) in [
        (0, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES),
        (crate::MAX_RECENT_ROWS, 0, MAX_RESTORE_BYTES),
        (crate::MAX_RECENT_ROWS, MAX_HISTORY_BYTES, 0),
    ] {
        assert!(matches!(
            handle.diagnostic_subscribe(rows, hist, screen).await,
            Err(SessionRestoreError::Gap(crate::RestoreGap::OutputTooLarge))
        ));
    }
}
#[tokio::test]
async fn explicit_close_never_publishes_a_natural_eof_capture() {
    let (transport, _driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(20, 4).unwrap(), 0, transport);
    handle.close();
    let end = task.join_with_receipt().await.unwrap();
    assert_eq!(end.state.lifecycle, Lifecycle::Closed);
    assert!(
        handle
            .diagnostic_subscribe(2000, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn eof_unrepresentable_state_preserves_typed_restore_refusal() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(20, 4).unwrap(), 0, transport);
    driver
        .output
        .send(b"12345678901234567890\x1b[2Z".to_vec())
        .await
        .unwrap();
    drop(driver.output);
    assert_eq!(
        task.join_with_receipt().await.unwrap().state.lifecycle,
        Lifecycle::Ended
    );
    assert!(matches!(
        handle
            .diagnostic_subscribe(2000, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
            .await,
        Err(SessionRestoreError::Gap(
            crate::RestoreGap::NonEdgePendingWrap
        ))
    ));
}

#[tokio::test]
async fn clipped_wide_pending_wrap_is_typed_gap_for_live_and_eof_capture() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(24, 6).unwrap(), 64, transport);
    let input = "a\x1b[6;22H中a\x1b7\x1b[4h\x1b[6;22H\x08中"
        .as_bytes()
        .to_vec();
    let bytes = input.len() as u64;
    driver.output.send(input).await.unwrap();
    let mut state = handle.state.clone();
    timeout(Duration::from_secs(2), async {
        while state.borrow().parsed_bytes < bytes {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        handle.diagnostic_restore(MAX_RESTORE_BYTES).await,
        Err(SessionRestoreError::Gap(
            crate::RestoreGap::ClippedWideHeadPendingWrap
        ))
    ));
    assert!(matches!(
        handle
            .diagnostic_bootstrap(crate::MAX_RECENT_ROWS, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
            .await,
        Err(SessionRestoreError::Gap(
            crate::RestoreGap::ClippedWideHeadPendingWrap
        ))
    ));
    assert!(matches!(
        handle
            .diagnostic_subscribe(crate::MAX_RECENT_ROWS, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
            .await,
        Err(SessionRestoreError::Gap(
            crate::RestoreGap::ClippedWideHeadPendingWrap
        ))
    ));
    drop(driver.output);
    let receipt = timeout(Duration::from_secs(2), task.join_with_receipt())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state.lifecycle, Lifecycle::Ended);
    assert!(matches!(
        handle
            .diagnostic_subscribe(crate::MAX_RECENT_ROWS, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
            .await,
        Err(SessionRestoreError::Gap(
            crate::RestoreGap::ClippedWideHeadPendingWrap
        ))
    ));
}
