#![allow(clippy::unwrap_used)]
use super::*;
use pulse_core::TermSize;

#[test]
fn history_page_reads_primary_rows_and_unicode_without_replay() {
    let mut terminal = TerminalProbe::new(TermSize::new(12, 2).unwrap(), 500);
    for i in 0..450 {
        terminal.feed(format!("{i:04}中e\u{301}\r\n").as_bytes());
    }
    let newest = terminal.history_page(None, 12).unwrap();
    assert_eq!(newest.rows.len(), 200);
    assert!(
        newest.rows[0]
            .text
            .starts_with(&format!("{:04}中e\u{301}", newest.start))
    );
    let previous = terminal.history_page(Some(newest.start), 12).unwrap();
    assert_eq!(previous.end, newest.start);
    assert!(
        previous.rows[0]
            .text
            .starts_with(&format!("{:04}中e\u{301}", previous.start))
    );
    terminal.feed(b"\x1b[?1049hALT");
    let alternate = terminal.history_page(None, 13).unwrap();
    assert_eq!(alternate.total_rows, newest.total_rows);
    assert_eq!(alternate.rows[0].text, newest.rows[0].text);
    assert!(matches!(
        terminal.history_page(Some(newest.total_rows + 1), 13),
        Err(HistoryPageError::Range)
    ));
}
#[test]
fn history_page_respects_byte_budget_and_empty_history() {
    let mut terminal = TerminalProbe::new(TermSize::new(512, 2).unwrap(), 500);
    assert!(terminal.history_page(None, 0).unwrap().rows.is_empty());
    for _ in 0..210 {
        terminal.feed(format!("{}\r\n", "中".repeat(256)).as_bytes());
    }
    let page = terminal.history_page(None, 1).unwrap();
    assert!(page.rows.len() < 200 && !page.rows.is_empty());
    assert!(page.rows.iter().map(|row| row.text.len()).sum::<usize>() <= 128 * 1024);
    assert_eq!(page.rows.len(), page.end - page.start);
}
#[tokio::test]
async fn history_page_actor_rejects_revision_after_output() {
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
    let page = handle.history_page(None, None).await.unwrap();
    driver.output.send(b"next\r\n".to_vec()).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while state.borrow().parsed_bytes <= initial.len() as u64 {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        handle
            .history_page(Some(page.start), Some(page.revision.parse().unwrap()))
            .await,
        Err(HistoryPageError::Stale)
    ));
    handle.close();
    task.join().await.unwrap();
}
