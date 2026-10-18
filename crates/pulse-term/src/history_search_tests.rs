#![allow(clippy::unwrap_used)]
use super::*;
use pulse_core::TermSize;
fn query(text: &str, backwards: bool) -> HistorySearchRequest {
    HistorySearchRequest {
        query: text.into(),
        match_case: false,
        regex: false,
        whole_word: false,
        backwards,
        alternate: false,
        cursor: None,
        revision: None,
    }
}
#[test]
fn history_search_reads_cold_hot_unicode_literal_and_alternate_primary() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(48, 2).unwrap(), 4);
    for n in 0..20 {
        t.feed(format!("{n:02} 中e\u{301} [A.*] ÄBC\r\n").as_bytes());
    }
    let mut request = query("[a.*] äbc", false);
    let first = t.history_search(&request, 7).unwrap();
    assert_eq!(first.matched_row, Some(0));
    let page = first.page.unwrap();
    assert!(page.cold_rows > 0);
    assert!(page.rows[0].text.contains("中e\u{301}"));
    request.backwards = true;
    t.feed(b"\x1b[?1049halternate");
    let last = t.history_search(&request, 8).unwrap();
    assert_eq!(last.matched_row, Some(last.total_rows - 2));
    request.match_case = true;
    assert!(t.history_search(&request, 8).unwrap().matched_row.is_none());
    request.query = "中e\u{301}".into();
    assert!(t.history_search(&request, 8).unwrap().matched_row.is_some());
}
#[test]
fn history_search_continuations_are_bounded_bidirectional_and_revision_bound() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(16, 2).unwrap(), 4);
    for _ in 0..5000 {
        t.feed(b"row\r\n");
    }
    for backwards in [false, true] {
        let mut request = query("absent", backwards);
        let mut scans = 0;
        loop {
            let found = t.history_search(&request, 9).unwrap();
            let old = request
                .cursor
                .unwrap_or(if backwards { found.total_rows } else { 0 });
            assert!(found.cursor.abs_diff(old) <= SEARCH_ROWS);
            assert!(found.cursor.abs_diff(old) > 0);
            scans += 1;
            request.cursor = Some(found.cursor);
            request.revision = Some(9);
            assert!(matches!(
                t.history_search(&request, 10),
                Err(HistoryPageError::Stale)
            ));
            if found.complete {
                break;
            }
        }
        assert_eq!(scans, 3);
    }
    for text in ["", "\n", "\u{1b}", &"x".repeat(513)] {
        assert!(t.history_search(&query(text, false), 9).is_err());
    }
}
#[test]
fn history_search_byte_budget_and_next_result_do_not_skip_rows() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(512, 2).unwrap(), 4);
    for _ in 0..750 {
        t.feed(format!("{}\r\n", "中".repeat(250)).as_bytes());
    }
    let mut request = query("missing", false);
    let first = t.history_search(&request, 1).unwrap();
    assert!(!first.complete);
    assert!(first.cursor < SEARCH_ROWS);
    assert!(first.cursor * 750 <= SEARCH_BYTES);
    request.query = "中".into();
    request.cursor = Some(first.cursor);
    request.revision = Some(1);
    let hit = t.history_search(&request, 1).unwrap();
    assert_eq!(hit.matched_row, Some(first.cursor));
    assert_eq!(hit.page.unwrap().end, first.cursor + 1);
}
#[tokio::test]
async fn history_search_actor_rejects_continuation_after_real_output() {
    use crate::session::{ChannelTransport, spawn_session};
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(16, 2).unwrap(), 64, transport);
    let mut state = handle.state.clone();
    let initial = b"one\r\ntwo\r\nthree\r\n";
    driver.output.send(initial.to_vec()).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while state.borrow().parsed_bytes < initial.len() as u64 {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let mut request = query("one", false);
    let found = handle.history_search(request.clone()).await.unwrap();
    driver.output.send(b"next\r\n".to_vec()).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while state.borrow().parsed_bytes <= initial.len() as u64 {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    request.cursor = Some(found.cursor);
    request.revision = Some(found.revision.parse().unwrap());
    assert!(matches!(
        handle.history_search(request).await,
        Err(HistoryPageError::Stale)
    ));
    handle.close();
    task.join().await.unwrap();
}

#[test]
fn history_search_primary_screen_wraps_and_hard_breaks() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(8, 3).unwrap(), 4);
    t.feed("abcdefg中e\u{301}tail".as_bytes());
    assert_eq!(t.history_page(None, 1).unwrap().total_rows, 0);
    for backwards in [false, true] {
        let found = t
            .history_search(&query("fg中e\u{301}", backwards), 1)
            .unwrap();
        assert_eq!((found.matched_row, found.matched_end), (Some(0), Some(2)));
        assert_eq!(found.screen_rows, 3);
        let page = found.page.unwrap();
        assert_eq!(page.rows[0].text, "abcdefg");
        assert!(page.rows[1].text.starts_with("中e\u{301}"));
    }
    t.feed(b"\x1b[?1049halternate-only");
    assert!(
        t.history_search(&query("alternate-only", false), 2)
            .unwrap()
            .matched_row
            .is_none()
    );
    assert!(
        t.history_search(&query("fg中e\u{301}", false), 2)
            .unwrap()
            .matched_row
            .is_some()
    );
    let mut hard = TerminalProbe::new(TermSize::new(8, 3).unwrap(), 4);
    hard.feed(b"abcdefgh\r\nIJKL");
    for backwards in [false, true] {
        assert!(
            hard.history_search(&query("fghIJK", backwards), 1)
                .unwrap()
                .matched_row
                .is_none()
        );
    }
}

#[test]
fn history_search_wraps_across_cold_hot_and_screen() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(4, 2).unwrap(), 2);
    t.feed(b"abcdefghijklmnopqrstuvwx");
    let history = t.history_page(None, 1).unwrap();
    assert_eq!((history.cold_rows, history.total_rows), (2, 4));
    for (text, start, end) in [("fghijk", 1, 3), ("nopq", 3, 5)] {
        for backwards in [false, true] {
            let found = t.history_search(&query(text, backwards), 1).unwrap();
            assert_eq!(
                (found.matched_row, found.matched_end),
                (Some(start), Some(end))
            );
            assert_eq!(found.total_rows, 6);
        }
    }
    assert_eq!(t.history_page(None, 1).unwrap().total_rows, 4);
}

#[test]
fn history_search_overlaps_batch_edges_both_directions() {
    for backwards in [false, true] {
        let mut t = TerminalProbe::with_cold_history(TermSize::new(1, 2).unwrap(), 4);
        let prefix = if backwards { 10 } else { SEARCH_ROWS - 1 };
        let suffix = if backwards { SEARCH_ROWS - 1 } else { 10 };
        t.feed(format!("{}AB{}", "x".repeat(prefix), "y".repeat(suffix)).as_bytes());
        let mut request = query("AB", backwards);
        let first = t.history_search(&request, 1).unwrap();
        assert!(first.matched_row.is_none());
        assert!(!first.complete);
        request.cursor = Some(first.cursor);
        request.revision = Some(1);
        let found = t.history_search(&request, 1).unwrap();
        assert_eq!(
            (found.matched_row, found.matched_end),
            (Some(prefix), Some(prefix + 2))
        );
        assert!(found.cursor.abs_diff(first.cursor) <= SEARCH_ROWS);
    }
}

#[test]
fn history_search_long_wrap_is_bounded_and_unicode_lowercase_crosses_rows() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(1, 2).unwrap(), 4);
    let text = format!("{}Z", "a".repeat(510));
    t.feed(text.as_bytes());
    let found = t.history_search(&query(&text, false), 1).unwrap();
    assert_eq!((found.matched_row, found.matched_end), (Some(0), Some(511)));
    let page = found.page.unwrap();
    assert_eq!(page.rows.len(), 200);
    assert_eq!(page.end, 511);
    assert!(page.start > 0);
    let mut unicode = TerminalProbe::new(TermSize::new(2, 3).unwrap(), 4);
    unicode.feed("AİBC".as_bytes());
    assert!(
        unicode
            .history_search(&query("ai\u{307}bc", false), 1)
            .unwrap()
            .matched_row
            .is_some()
    );
}

#[test]
fn history_search_alternate_isolated_unicode_wrap_and_inactive_rejection() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(8, 3).unwrap(), 4);
    for _ in 0..20 {
        t.feed(b"MAIN\r\n");
    }
    let before = t.history_page(None, 1).unwrap();
    let mut request = query("XYZtail", false);
    request.alternate = true;
    assert!(matches!(
        t.history_search(&request, 1),
        Err(HistoryPageError::Unavailable)
    ));
    t.feed("\x1b[?1049h\x1b[Hab中e\u{301}XYZtail".as_bytes());
    for backwards in [false, true] {
        request.backwards = backwards;
        let found = t.history_search(&request, 2).unwrap();
        assert!(found.alternate);
        assert_eq!((found.total_rows, found.screen_rows), (3, 3));
        assert_eq!((found.matched_row, found.matched_end), (Some(0), Some(2)));
        let page = found.page.unwrap();
        assert_eq!(page.cold_rows, 0);
        assert!(!page.truncated);
        assert!(page.rows[0].text.contains("中e\u{301}"));
    }
    request.query = "MAIN".into();
    assert!(t.history_search(&request, 2).unwrap().matched_row.is_none());
    assert!(
        t.history_search(&query("MAIN", false), 2)
            .unwrap()
            .matched_row
            .is_some()
    );
    assert_eq!(
        t.history_page(None, 2).unwrap().total_rows,
        before.total_rows
    );
    request.cursor = Some(1);
    request.revision = Some(2);
    assert!(matches!(
        t.history_search(&request, 3),
        Err(HistoryPageError::Stale)
    ));
    t.feed(b"\x1b[?1049l");
    request.cursor = None;
    request.revision = None;
    assert!(matches!(
        t.history_search(&request, 4),
        Err(HistoryPageError::Unavailable)
    ));
}

#[test]
#[ignore = "manual release timing; no wall-clock assertion"]
fn history_search_100000_rows_timing() {
    let mut terminal = TerminalProbe::with_cold_history(TermSize::new(80, 3).unwrap(), 64);
    let mut input = String::new();
    for index in 0..100_010 {
        use std::fmt::Write;
        writeln!(input, "{index:06} retained history 中e\u{301}\r").unwrap();
    }
    terminal.feed(input.as_bytes());
    assert_eq!(terminal.history_page(None, 1).unwrap().total_rows, 100_000);
    for backwards in [false, true] {
        let start = std::time::Instant::now();
        let mut turns = 0;
        for _ in 0..10 {
            let mut request = query("absent-pattern", backwards);
            loop {
                let found = terminal.history_search(&request, 1).unwrap();
                assert!(found.matched_row.is_none());
                turns += 1;
                if found.complete {
                    break;
                }
                request.cursor = Some(found.cursor);
                request.revision = Some(1);
            }
        }
        eprintln!(
            "history_search_100000 backwards={backwards} repetitions=10 turns={turns} elapsed_us={}",
            start.elapsed().as_micros()
        );
    }
}
