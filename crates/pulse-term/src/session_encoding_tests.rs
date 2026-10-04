#![allow(clippy::unwrap_used)]
use super::*;
use tokio::time::{Duration, timeout};
fn size() -> TermSize {
    TermSize::new(80, 24).unwrap()
}
#[tokio::test]
async fn pending_gbk_source_survives_atomic_reattach_without_fake_display_bytes() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_gbk_text_session(size(), 32, transport);
    let mut old = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    driver.output.send(vec![0xd6]).await.unwrap();
    let record = timeout(Duration::from_secs(1), old.output.recv())
        .await
        .unwrap()
        .unwrap();
    let StreamRecord::Marker(b) = record else {
        panic!("pending source marker");
    };
    assert_eq!(b.parsed_bytes, 1);
    assert_eq!(b.utf8_bytes, 0);
    assert!(!b.source_is_utf8);
    let mut fresh = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    assert_eq!(fresh.bootstrap.boundary, b);
    assert_eq!(old.status.borrow().end, StreamEnd::Replaced);
    driver.output.send(vec![0xd0]).await.unwrap();
    let StreamRecord::Data {
        start,
        bytes,
        boundary,
    } = fresh.output.recv().await.unwrap()
    else {
        panic!("normalized data");
    };
    assert_eq!(start, 0);
    assert_eq!(bytes, "中".as_bytes());
    assert_eq!((boundary.parsed_bytes, boundary.utf8_bytes), (2, 3));
    drop(driver);
    let end = task.join().await.unwrap();
    assert_eq!(end.lifecycle, Lifecycle::Ended);
    assert_eq!((end.parsed_bytes, end.utf8_bytes), (2, 3));
}
#[tokio::test]
async fn maximum_source_chunk_expands_into_bounded_ordered_display_records() {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_gbk_text_session(size(), 32, transport);
    let mut stream = handle
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    driver
        .output
        .send([0xd6, 0xd0].repeat(MAX_CHUNK / 2))
        .await
        .unwrap();
    let mut rendered = Vec::new();
    for _ in 0..2 {
        let StreamRecord::Data {
            start,
            bytes,
            boundary,
        } = timeout(Duration::from_secs(1), stream.output.recv())
            .await
            .unwrap()
            .unwrap()
        else {
            panic!("data");
        };
        assert_eq!(start, rendered.len() as u64);
        assert!(bytes.len() <= MAX_CHUNK);
        rendered.extend(bytes);
        assert_eq!(boundary.parsed_bytes, MAX_CHUNK as u64);
        assert_eq!(boundary.utf8_bytes, rendered.len() as u64);
    }
    assert_eq!(rendered, "中".repeat(MAX_CHUNK / 2).as_bytes());
    handle.close();
    drop(driver);
    assert_eq!(task.join().await.unwrap().lifecycle, Lifecycle::Closed);
}
#[tokio::test]
async fn reverse_input_and_query_encoding_refuse_unrepresentable_input_without_writing() {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_gbk_text_session(size(), 32, transport);
    handle.input("中文".as_bytes().to_vec()).await.unwrap();
    let TransportWrite::Input(bytes) = driver.input.recv().await.unwrap() else {
        panic!("input");
    };
    assert_eq!(bytes, [0xd6, 0xd0, 0xce, 0xc4]);
    assert_eq!(
        handle.input("🌍".as_bytes().to_vec()).await,
        Err(SessionError::Encoding)
    );
    assert!(driver.input.try_recv().is_err());
    driver.output.send(b"\x1b[6n".to_vec()).await.unwrap();
    let TransportWrite::Reply(reply) = timeout(Duration::from_secs(1), driver.input.recv())
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("query reply");
    };
    assert_eq!(reply, b"\x1b[1;1R");
    handle.close();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn incomplete_or_malformed_gbk_is_failure_not_clean_eof_or_replacement() {
    for bytes in [vec![0xd6], vec![0xff]] {
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = spawn_gbk_text_session(size(), 0, transport);
        driver.output.send(bytes).await.unwrap();
        drop(driver);
        let (state, screen) = task.join_with_screen().await.unwrap();
        assert_eq!(state.lifecycle, Lifecycle::Failed);
        assert_eq!(state.utf8_bytes, 0);
        assert!(
            !screen
                .cells
                .iter()
                .flatten()
                .any(|c| c.text.contains('\u{fffd}'))
        );
        assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Failed);
    }
}
