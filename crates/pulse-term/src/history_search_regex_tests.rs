#![allow(clippy::unwrap_used)]
use super::*;
use crate::{HistorySearchRequest, TerminalProbe};
use pulse_core::TermSize;
fn request(pattern: &str, backwards: bool) -> HistorySearchRequest {
    HistorySearchRequest {
        query: pattern.into(),
        match_case: true,
        regex: true,
        whole_word: false,
        alternate: false,
        backwards,
        cursor: None,
        revision: None,
    }
}
#[test]
fn regex_wraps_unicode_case_and_literal_compatibility() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(4, 3).unwrap(), 4);
    t.feed("AB中e\u{301}12\r\nTAIL".as_bytes());
    for backwards in [false, true] {
        let hit = t
            .history_search(&request("^AB中e\\p{M}12$", backwards), 7)
            .unwrap();
        assert_eq!((hit.matched_row, hit.matched_end), (Some(0), Some(2)));
        assert!(hit.regex);
        assert!(
            t.history_search(&request("^e\\p{M}12$", backwards), 7)
                .unwrap()
                .matched_row
                .is_none()
        );
    }
    let mut r = request("^ab中", false);
    assert!(t.history_search(&r, 7).unwrap().matched_row.is_none());
    r.match_case = false;
    assert!(t.history_search(&r, 7).unwrap().matched_row.is_some());
    r.regex = false;
    assert!(t.history_search(&r, 7).unwrap().matched_row.is_none());
}
#[test]
fn regex_invalid_unsupported_and_expansion_limits_are_typed() {
    let t = TerminalProbe::new(TermSize::new(8, 2).unwrap(), 4);
    for pattern in ["[", "(a)\\1", "(?=a)", "(?:a{1000}){1000}"] {
        assert!(
            matches!(
                t.history_search(&request(pattern, false), 7),
                Err(HistoryPageError::Pattern)
            ),
            "{pattern}"
        );
    }
}
#[test]
fn regex_zero_width_progresses_once_per_logical_line_and_rejects_partial_cursors() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(4, 3).unwrap(), 4);
    t.feed(b"ABCDEFGH\r\nlast");
    for backwards in [false, true] {
        let mut r = request("^", backwards);
        let mut hits = Vec::new();
        loop {
            let found = t.history_search(&r, 7).unwrap();
            if let Some(row) = found.matched_row {
                hits.push(row);
            }
            if found.complete {
                break;
            }
            r.cursor = Some(found.cursor);
            r.revision = Some(7);
            assert!(hits.len() <= 3);
        }
        assert_eq!(hits.len(), 2);
    }
    let mut invalid = request("^EF", false);
    invalid.cursor = Some(1);
    invalid.revision = Some(7);
    assert!(matches!(
        t.history_search(&invalid, 7),
        Err(HistoryPageError::Range)
    ));
}
#[test]
fn regex_batches_preserve_full_line_anchors_in_both_directions() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(4, 2).unwrap(), 4);
    for _ in 0..2047 {
        t.feed(b"x\r\n");
    }
    t.feed(b"ABCDEFGH!\r\n");
    for _ in 0..2047 {
        t.feed(b"x\r\n");
    }
    for backwards in [false, true] {
        let mut r = request("^ABCDEFGH!$", backwards);
        let mut turns = 0;
        loop {
            let hit = t.history_search(&r, 9).unwrap();
            turns += 1;
            if let Some(row) = hit.matched_row {
                assert_eq!(row, 2047);
                break;
            }
            assert!(!hit.complete);
            r.cursor = Some(hit.cursor);
            r.revision = Some(9);
            assert!(turns < 5);
        }
        assert!(turns > 1);
    }
}
#[test]
fn regex_oversized_logical_line_is_rejected_without_partial_match() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(1, 2).unwrap(), 4);
    t.feed(&vec![b'x'; SEARCH_ROWS + 2]);
    for backwards in [false, true] {
        assert!(matches!(
            t.history_search(&request("^x+$", backwards), 7),
            Err(HistoryPageError::TooLarge)
        ));
    }
}
#[tokio::test]
async fn regex_works_after_eof_for_both_frozen_sources() {
    use crate::session::*;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(4, 2).unwrap(), 4, transport);
    driver
        .output
        .send(b"ABCDEFGH\x1b[?1049h\x1b[HALT".to_vec())
        .await
        .unwrap();
    drop(driver.output);
    task.join_with_receipt().await.unwrap();
    assert!(
        handle
            .history_search(request("^ABCDEFGH$", false))
            .await
            .unwrap()
            .matched_row
            .is_some()
    );
    let mut r = request("^ALT$", true);
    r.alternate = true;
    let hit = handle.history_search(r).await.unwrap();
    assert!(hit.alternate && hit.regex && hit.matched_row.is_some());
    assert!(handle.input(b"no".to_vec()).await.is_err());
}
