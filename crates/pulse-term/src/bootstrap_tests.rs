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

#[test]
fn restore_carries_base_without_turning_it_into_remote_overrides() {
    let mut source = TerminalProbe::new(size(), 64);
    let palette = crate::QueryPalette {
        foreground: [1, 2, 3],
        background: [4, 5, 6],
        cursor: [7, 8, 9],
        ansi: std::array::from_fn(|i| [i as u8, 16 + i as u8, 32 + i as u8]),
    };
    source.set_query_palette(palette.clone());
    source.feed(b"one\r\ntwo\x1b]4;1;#abcdef\x07\x1b]10;#102030\x07");
    let frame = source.restore_sequence().unwrap();
    assert_eq!(frame.palette, palette);
    let mut restored = TerminalProbe::new(size(), 64);
    restored.set_query_palette(frame.palette.clone());
    restored.feed(&frame.bytes);
    assert_eq!(restored.restore_fingerprint(), source.restore_fingerprint());
    let mut wrong_base = TerminalProbe::new(size(), 64);
    wrong_base.feed(&frame.bytes);
    assert_ne!(
        wrong_base.restore_fingerprint(),
        source.restore_fingerprint()
    );
    for continuation in [b"\x1b]4;1;?\x07\x1b]10;?\x07".as_slice(), b"\x1b]104\x07\x1b]110\x07\x1b]111\x07\x1b]112\x07\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07".as_slice(),b"\x1bc\x1b]10;?\x07".as_slice()] {
      source.feed(continuation);restored.feed(continuation);
      assert_eq!(source.take_replies(),restored.take_replies());
      assert_eq!(source.restore_fingerprint(),restored.restore_fingerprint());
    }
}

#[tokio::test]
async fn captures_retain_palette_boundary_after_actor_reconfiguration() {
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size(), 64, transport);
    let a = crate::QueryPalette {
        foreground: [1, 2, 3],
        ..crate::QueryPalette::default()
    };
    handle.set_query_palette(a.clone()).await.unwrap();
    let before = handle.state.borrow().clone();
    let old = handle
        .diagnostic_subscribe(2, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let diagnostic = handle.diagnostic_restore(MAX_RESTORE_BYTES).await.unwrap();
    assert_eq!(old.bootstrap.palette, a);
    assert_eq!(old.bootstrap.boundary, before);
    assert_eq!(diagnostic.palette, a);
    assert_eq!(diagnostic.boundary, before);
    let b = crate::QueryPalette {
        background: [4, 5, 6],
        ..a.clone()
    };
    handle.set_query_palette(b.clone()).await.unwrap();
    assert_eq!(old.status.borrow().end, StreamEnd::Reconfigured);
    assert_eq!(old.bootstrap.palette, a);
    let fresh = handle
        .diagnostic_bootstrap(2, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert_eq!(fresh.palette, b);
    assert_eq!(fresh.boundary.revision, before.revision + 1);
    assert_eq!(fresh.boundary.parsed_bytes, before.parsed_bytes);
    handle.close();
    task.join().await.unwrap();
}
