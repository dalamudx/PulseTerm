#![allow(clippy::unwrap_used)]
use crate::{HistorySearchRequest, TerminalProbe};
use pulse_core::TermSize;

fn whole(query: &str, regex: bool, backwards: bool) -> HistorySearchRequest {
    HistorySearchRequest {
        query: query.into(),
        match_case: true,
        regex,
        whole_word: true,
        backwards,
        alternate: false,
        cursor: None,
        revision: None,
    }
}
#[test]
fn whole_word_escapes_literals_and_combines_with_case_and_regex() {
    let mut t = TerminalProbe::new(TermSize::new(40, 4).unwrap(), 4);
    t.feed(b"bobcat cat2 cat_ dogfish\r\nfileXtxt\r\nfile.txt\r\nCAT");
    for backwards in [false, true] {
        assert!(
            t.history_search(&whole("cat", false, backwards), 7)
                .unwrap()
                .matched_row
                .is_none()
        );
        assert!(
            t.history_search(&whole("cat|dog", true, backwards), 7)
                .unwrap()
                .matched_row
                .is_none()
        );
    }
    let mut r = whole("cat", false, false);
    r.match_case = false;
    assert_eq!(t.history_search(&r, 7).unwrap().matched_row, Some(3));
    r.whole_word = false;
    assert_eq!(t.history_search(&r, 7).unwrap().matched_row, Some(0));
    assert_eq!(
        t.history_search(&whole("file.txt", false, false), 7)
            .unwrap()
            .matched_row,
        Some(2)
    );
    assert_eq!(
        t.history_search(&whole("file.txt", true, false), 7)
            .unwrap()
            .matched_row,
        Some(1)
    );
}
#[test]
fn whole_word_respects_unicode_marks_underscores_and_continuous_chinese() {
    let mut t = TerminalProbe::new(TermSize::new(50, 3).unwrap(), 4);
    t.feed("cafe\u{301} 中文测试 élan_".as_bytes());
    for query in ["cafe", "中文", "élan"] {
        assert!(
            t.history_search(&whole(query, false, false), 7)
                .unwrap()
                .matched_row
                .is_none()
        );
    }
    t.feed("\r\ncafe\u{301} 中文 élan".as_bytes());
    for query in ["cafe\u{301}", "中文", "élan"] {
        let hit = t.history_search(&whole(query, false, true), 8).unwrap();
        assert_eq!(hit.matched_row, Some(1));
        assert!(hit.whole_word && !hit.regex);
    }
}
#[test]
fn whole_word_keeps_soft_wrap_boundaries_and_alternate_scope_separate() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(4, 3).unwrap(), 4);
    t.feed(b"xxcat cat");
    for backwards in [false, true] {
        let hit = t
            .history_search(&whole("cat", false, backwards), 7)
            .unwrap();
        assert_eq!((hit.matched_row, hit.matched_end), (Some(0), Some(3)));
    }
    t.feed(b"\x1b[?1049h\x1b[Hbobcat");
    let mut r = whole("cat", true, true);
    r.alternate = true;
    assert!(t.history_search(&r, 8).unwrap().matched_row.is_none());
    t.feed(b"\r\ncat");
    assert!(t.history_search(&r, 9).unwrap().matched_row.is_some());
}
#[tokio::test]
async fn whole_word_reads_both_frozen_sources_without_input() {
    use crate::session::*;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(12, 2).unwrap(), 4, transport);
    driver
        .output
        .send(b"cat\x1b[?1049h\x1b[Hbobcat cat".to_vec())
        .await
        .unwrap();
    drop(driver.output);
    task.join_with_receipt().await.unwrap();
    assert!(
        handle
            .history_search(whole("cat", false, false))
            .await
            .unwrap()
            .matched_row
            .is_some()
    );
    let mut r = whole("cat|dog", true, true);
    r.alternate = true;
    let hit = handle.history_search(r).await.unwrap();
    assert!(hit.alternate && hit.whole_word && hit.regex && hit.matched_row.is_some());
    assert!(handle.input(b"no".to_vec()).await.is_err());
}
