#![allow(clippy::unwrap_used)]
use super::*;
use crate::{TerminalProbe, history_page::PAGE_BYTES};
use pulse_core::TermSize;

fn output(count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for n in 0..count {
        bytes.extend_from_slice(format!("{n:06} 中e\u{301}\r\n").as_bytes());
    }
    bytes
}
fn rows(t: &TerminalProbe, before: Option<usize>) -> Vec<(String, bool)> {
    t.history_page(before, 1)
        .unwrap()
        .rows
        .into_iter()
        .map(|r| (r.text, r.wrapped))
        .collect()
}
#[test]
fn cold_history_retains_100000_rows_from_one_batch_with_bounded_hot_grid() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(32, 3).unwrap(), 64);
    let written = 100_010;
    t.feed(&output(written));
    let page = t.history_page(None, 1).unwrap();
    assert_eq!(page.total_rows, MAX_ROWS);
    assert_eq!(page.cold_rows, MAX_ROWS - 64);
    assert!(page.truncated);
    assert_eq!(t.term.grid().history_size(), 64);
    assert!(t.cold.memory_bytes() <= MAX_BYTES);
    let oldest = t.history_page(Some(200), 1).unwrap();
    let first = written - 2 - MAX_ROWS;
    assert_eq!(oldest.start, 0);
    assert_eq!(oldest.rows[0].text, format!("{first:06} 中e\u{301}"));
    assert_eq!(
        oldest.rows[199].text,
        format!("{:06} 中e\u{301}", first + 199)
    );
    for (offset, row) in page.rows.iter().enumerate() {
        assert_eq!(
            row.text,
            format!("{:06} 中e\u{301}", first + page.start + offset)
        );
    }
}
#[test]
fn cold_history_matches_sdk_rows_for_wrap_wide_combining_and_tabs() {
    let size = TermSize::new(8, 3).unwrap();
    let mut t = TerminalProbe::with_cold_history(size, 4);
    let mut reference = TerminalProbe::new(size, 1000);
    let bytes = "0123456789\r\n中e\u{301}🙂\r\n\tX\r\n\x1b[2J".repeat(12);
    t.feed(bytes.as_bytes());
    reference.feed(bytes.as_bytes());
    assert!(t.cold.len() > 0);
    assert_eq!(rows(&t, None), rows(&reference, None));
    assert_eq!(t.screen(), reference.screen());
    assert_eq!(t.take_replies(), reference.take_replies());
}
#[test]
fn cold_history_byte_budget_and_oversize_row_keep_a_contiguous_suffix() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(32, 3).unwrap(), 4);
    t.cold.byte_limit = 2048;
    t.feed(&output(1000));
    let page = t.history_page(None, 1).unwrap();
    assert!(page.truncated && page.total_rows < 998);
    assert!(t.cold.memory_bytes() <= 2048);
    t.feed(format!("X{}\r\n", "\u{301}".repeat(PAGE_BYTES / 2 + 1)).as_bytes());
    t.feed(b"tail0\r\ntail1\r\ntail2\r\ntail3\r\ntail4\r\ntail5\r\ntail6\r\n");
    let page = t.history_page(None, 2).unwrap();
    assert!(page.truncated);
    assert!(page.rows.iter().all(|r| r.text.starts_with("tail")));
    assert!(t.cold.memory_bytes() <= 2048);
}
#[test]
fn cold_history_handles_alternate_resize_ed2_ed3_and_reset() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(32, 3).unwrap(), 4);
    t.feed("primary-01234567890123456789\r\n".repeat(80).as_bytes());
    let prefix = rows(&t, Some(20));
    let cold_before = t.cold.len();
    let total_before = t.history_page(None, 1).unwrap().total_rows;
    t.feed(b"\x1b[?1049hALT\r\n\x1b[3J");
    assert_eq!(t.history_page(None, 1).unwrap().total_rows, total_before);
    t.resize(TermSize::new(8, 4).unwrap());
    assert!(t.term.inactive_grid().history_size() <= 4);
    assert!(t.cold.len() > cold_before);
    assert_eq!(rows(&t, Some(20)), prefix);
    assert!(t.take_replies().is_empty());
    t.feed(b"\x1b[?1049l\x1b[2J");
    assert!(t.cold.len() > 0);
    t.feed(b"\x1b[3J");
    let page = t.history_page(None, 2).unwrap();
    assert_eq!(page.total_rows, 0);
    assert!(!page.truncated);
    t.feed(&output(40));
    assert!(t.cold.len() > 0);
    t.feed(b"\x1bc");
    assert_eq!(t.history_page(None, 3).unwrap().total_rows, 0);
}
#[test]
fn cold_history_collects_synchronized_output_on_timeout_flush() {
    let size = TermSize::new(32, 3).unwrap();
    let mut t = TerminalProbe::with_cold_history(size, 4);
    let mut reference = TerminalProbe::new(size, 1000);
    let bytes = output(300);
    t.feed(b"\x1b[?2026h");
    t.feed(&bytes);
    reference.feed(&bytes);
    assert!(t.expire_sync(std::time::Instant::now() + std::time::Duration::from_secs(60)));
    assert_eq!(rows(&t, None), rows(&reference, None));
    assert_eq!(t.history_page(None, 1).unwrap().total_rows, 298);
}
#[test]
fn cold_history_respects_explicit_zero_history() {
    let mut t = TerminalProbe::with_cold_history(TermSize::new(32, 3).unwrap(), 0);
    t.feed(&output(300));
    let page = t.history_page(None, 1).unwrap();
    assert_eq!(page.total_rows, 0);
    assert!(!page.truncated);
    assert_eq!(t.cold.memory_bytes(), 0);
}
#[tokio::test]
async fn cold_history_reaches_production_actor_revision_pages() {
    use crate::session::{ChannelTransport, spawn_session};
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(32, 3).unwrap(), 8, transport);
    let mut state = handle.state.clone();
    let bytes = output(1500);
    let length = bytes.len() as u64;
    driver.output.send(bytes).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while state.borrow().parsed_bytes < length {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let newest = handle.history_page(None, None).await.unwrap();
    assert_eq!(newest.total_rows, 1498);
    assert_eq!(newest.cold_rows, 1490);
    let oldest = handle
        .history_page(Some(100), Some(newest.revision.parse().unwrap()))
        .await
        .unwrap();
    assert_eq!(oldest.start, 0);
    assert_eq!(oldest.rows[0].text, "000000 中e\u{301}");
    handle.close();
    task.join().await.unwrap();
}

#[test]
fn configured_history_capacity_bounds_hot_and_cold_and_survives_resize() {
    for (hot, limit) in [(64, 1000), (10000, 1000), (64, 3000)] {
        let mut t = TerminalProbe::with_history_limit(TermSize::new(32, 3).unwrap(), hot, limit);
        t.feed(&output(limit + 50));
        let page = t.history_page(None, 1).unwrap();
        assert_eq!(page.total_rows, limit);
        assert!(page.truncated);
        assert_eq!(t.term.grid().history_size(), hot.min(limit));
        t.feed(b"\x1b[?1049hALT\x1b[?1049l");
        assert_eq!(t.history_page(None, 2).unwrap().total_rows, limit);
        t.feed(b"\x1b[3J");
        assert_eq!(t.history_page(None, 3).unwrap().total_rows, 0);
    }
}
