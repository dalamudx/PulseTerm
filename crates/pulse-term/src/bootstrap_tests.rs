#![allow(clippy::unwrap_used)]
use super::*;
fn size() -> TermSize {
    TermSize::new(32, 8).unwrap()
}
async fn parsed(handle: &SessionHandle, bytes: u64) {
    let mut state = handle.state.clone();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while state.borrow().parsed_bytes < bytes {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn bootstrap_captures_recent_history_and_screen_at_one_boundary() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 64, transport);
    let prefix = b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\neight\r\nnine\r\nten\r\neleven\r\ntwelve\r\n\x1b[?1049hALT\x1b[31";
    handle.input(prefix.to_vec()).await.unwrap();
    parsed(&handle, prefix.len() as u64).await;
    let capture = handle
        .diagnostic_bootstrap(3, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert_eq!(capture.boundary.parsed_bytes, prefix.len() as u64);
    assert_eq!(capture.history.retained_rows, 3);
    assert!(capture.history.total_rows > 3);
    assert!(!capture.restore.windows(2).any(|w| w == b"\x1bc"));
    let mut view = TerminalProbe::new(size(), 64);
    view.feed(&capture.history.bytes);
    view.feed(&capture.restore);
    assert_eq!(view.screen(), handle.inspect().await.unwrap());
    let suffix = b"m!\x1b[?1049lZ";
    view.feed(suffix);
    handle.input(suffix.to_vec()).await.unwrap();
    parsed(&handle, (prefix.len() + suffix.len()) as u64).await;
    assert_eq!(view.screen(), handle.inspect().await.unwrap());
    handle.close();
    task.join().await.unwrap();
}
#[tokio::test]
async fn bootstrap_failure_returns_no_partial_and_keeps_actor_running() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 64, transport);
    let prefix = b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n9\r\n";
    handle.input(prefix.to_vec()).await.unwrap();
    parsed(&handle, prefix.len() as u64).await;
    let before = handle.state.borrow().clone();
    assert!(matches!(
        handle.diagnostic_bootstrap(2, 0, MAX_RESTORE_BYTES).await,
        Err(SessionRestoreError::Gap(crate::RestoreGap::OutputTooLarge))
    ));
    assert!(matches!(
        handle.diagnostic_bootstrap(2, MAX_HISTORY_BYTES, 0).await,
        Err(SessionRestoreError::Gap(crate::RestoreGap::OutputTooLarge))
    ));
    assert_eq!(before, *handle.state.borrow());
    assert!(
        handle
            .diagnostic_bootstrap(usize::MAX, usize::MAX, usize::MAX)
            .await
            .is_ok()
    );
    handle.close();
    task.join().await.unwrap();
}
