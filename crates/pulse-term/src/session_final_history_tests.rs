#![allow(clippy::unwrap_used)]
use super::*;
use tokio::time::{Duration, timeout};

fn query(text: &str) -> crate::HistorySearchRequest {
    crate::HistorySearchRequest {
        query: text.into(),
        match_case: false,
        regex: false,
        whole_word: false,
        backwards: false,
        alternate: false,
        cursor: None,
        revision: None,
    }
}

#[tokio::test]
async fn eof_history_retains_cold_text_and_primary_screen_without_an_actor() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 4).unwrap(), 8, transport);
    let mut bytes = Vec::new();
    for n in 0..2500 {
        bytes.extend_from_slice(format!("history-{n:04} 中e\u{301}\r\n").as_bytes());
    }
    bytes.extend_from_slice(b"TAIL");
    bytes.extend_from_slice(&[0xe4, 0xb8]);
    for chunk in bytes.chunks(4096) {
        driver.output.send(chunk.to_vec()).await.unwrap();
    }
    let mut state = handle.state.clone();
    timeout(Duration::from_secs(2), async {
        while state.borrow().parsed_bytes < bytes.len() as u64 {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let old = handle.history_page(None, None).await.unwrap();
    drop(driver.output);
    let receipt = timeout(Duration::from_secs(2), task.join_with_receipt())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state.lifecycle, Lifecycle::Ended);
    assert!(handle.commands.is_closed());
    assert!(*driver.stop.borrow());
    let page = handle.history_page(None, None).await.unwrap();
    assert!(page.total_rows > crate::MAX_RECENT_ROWS);
    assert!(page.cold_rows > 0);
    assert_eq!(page.revision, receipt.state.revision.to_string());
    assert!(matches!(
        handle
            .history_page(Some(old.end), Some(old.revision.parse().unwrap()))
            .await,
        Err(crate::HistoryPageError::Stale)
    ));
    let first = handle
        .history_page(Some(1), Some(receipt.state.revision))
        .await
        .unwrap();
    assert_eq!(first.rows[0].text, "history-0000 中e\u{301}");
    let found = handle
        .history_search(query("history-0000 中e\u{301}"))
        .await
        .unwrap();
    assert_eq!(found.matched_row, Some(0));
    let mut request = query("TAIL\u{fffd}");
    request.backwards = true;
    let screen = handle.history_search(request.clone()).await.unwrap();
    assert!(screen.matched_row.unwrap() >= page.total_rows);
    assert_eq!(
        screen.page.unwrap().rows.last().unwrap().text,
        "TAIL\u{fffd}"
    );
    assert!(handle.input(b"no".to_vec()).await.is_err());
    assert!(handle.resize(TermSize::new(40, 5).unwrap()).await.is_err());
    let repeated = handle.history_search(request).await.unwrap();
    assert_eq!(repeated.revision, page.revision);
    assert!(repeated.matched_row.is_some());
}

#[tokio::test]
async fn ended_search_preserves_wraps_and_hidden_primary_screen() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(4, 2).unwrap(), 4, transport);
    driver
        .output
        .send(b"ABCDEFGH\x1b[?1049hALT".to_vec())
        .await
        .unwrap();
    drop(driver.output);
    assert_eq!(
        task.join_with_receipt().await.unwrap().state.lifecycle,
        Lifecycle::Ended
    );
    let found = handle.history_search(query("CDEFG")).await.unwrap();
    assert_eq!(found.matched_row, Some(0));
    assert_eq!(found.matched_end, Some(2));
    assert!(
        handle
            .history_search(query("ALT"))
            .await
            .unwrap()
            .matched_row
            .is_none()
    );
    let mut alternate = query("ALT");
    alternate.alternate = true;
    let found = handle.history_search(alternate.clone()).await.unwrap();
    assert!(found.alternate);
    assert_eq!((found.total_rows, found.screen_rows), (2, 2));
    assert_eq!(found.page.unwrap().cold_rows, 0);
    alternate.query = "CDEFG".into();
    assert!(
        handle
            .history_search(alternate)
            .await
            .unwrap()
            .matched_row
            .is_none()
    );
}

#[tokio::test]
async fn explicit_close_and_transport_failure_publish_no_final_text() {
    for failed in [false, true] {
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = spawn_session(TermSize::new(20, 3).unwrap(), 8, transport);
        if failed {
            driver.failed.send_replace(true);
        } else {
            handle.close();
        }
        let receipt = timeout(Duration::from_secs(2), task.join_with_receipt())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt.state.lifecycle,
            if failed {
                Lifecycle::Failed
            } else {
                Lifecycle::Closed
            }
        );
        assert!(handle.final_history.get().is_none());
        assert!(matches!(
            handle.history_page(None, None).await,
            Err(crate::HistoryPageError::Unavailable)
        ));
        assert!(matches!(
            handle.history_search(query("text")).await,
            Err(crate::HistoryPageError::Unavailable)
        ));
    }
}

#[tokio::test]
async fn ended_without_active_alternate_never_exposes_an_inactive_grid() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(16, 2).unwrap(), 4, transport);
    driver
        .output
        .send(b"\x1b[?1049hOLD ALT\x1b[?1049lMAIN".to_vec())
        .await
        .unwrap();
    drop(driver.output);
    task.join_with_receipt().await.unwrap();
    let mut request = query("OLD ALT");
    request.alternate = true;
    assert!(matches!(
        handle.history_search(request).await,
        Err(crate::HistoryPageError::Unavailable)
    ));
    assert!(
        handle
            .history_search(query("MAIN"))
            .await
            .unwrap()
            .matched_row
            .is_some()
    );
}

#[tokio::test]
async fn configured_history_capacity_is_retained_after_eof() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session_with_history_limit(
        TermSize::new(32, 3).unwrap(),
        64,
        transport,
        crate::QueryPalette::default(),
        1000,
    );
    let mut bytes = Vec::new();
    for n in 0..1050 {
        bytes.extend_from_slice(format!("row-{n}\r\n").as_bytes());
    }
    for chunk in bytes.chunks(4096) {
        driver.output.send(chunk.to_vec()).await.unwrap();
    }
    drop(driver.output);
    let receipt = timeout(Duration::from_secs(2), task.join_with_receipt())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state.lifecycle, Lifecycle::Ended);
    let page = handle.history_page(None, None).await.unwrap();
    assert_eq!(page.total_rows, 1000);
    assert!(page.truncated);
    assert!(handle.commands.is_closed());
}
