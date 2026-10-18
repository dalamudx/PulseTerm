#![allow(clippy::unwrap_used)]
use super::*;

#[tokio::test]
async fn live_end_ack_keeps_readonly_history_until_detach_without_holding_a_task() {
    let f = Fixture::new();
    let capture = f.handle.diagnostic_digest().await.unwrap();
    let prepared = f.prepare().await;
    let key = ticket(&prepared.ticket).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    f.bank
        .connect(
            f.nonce,
            key,
            tauri::ipc::Channel::new(move |body| {
                tx.send(body).unwrap();
                Ok(())
            }),
        )
        .unwrap();
    f.bank
        .confirm(
            f.nonce,
            key,
            &CaptureDrawing {
                columns: capture.boundary.size.columns(),
                rows: capture.boundary.size.rows(),
                revision: capture.boundary.revision.to_string(),
                source_bytes: capture.boundary.parsed_bytes.to_string(),
                utf8_bytes: capture.boundary.utf8_bytes.to_string(),
                digest: capture.digest,
            },
        )
        .await
        .unwrap();
    loop {
        let tauri::ipc::InvokeResponseBody::Raw(wire) = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap()
        else {
            panic!("non raw")
        };
        let frame = pulse_ipc::decode(&wire).unwrap();
        let part = pulse_ipc::attach_v2::decode_fragment(frame.payload)
            .unwrap()
            .part;
        f.bank.ack(f.nonce, key, frame.sequence).unwrap();
        if part == pulse_ipc::attach::Part::Commit {
            break;
        }
    }
    f.bank.enable_input(f.nonce, key).await.unwrap();
    f._driver
        .output
        .send(
            b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\neight\r\nEOF needle".to_vec(),
        )
        .await
        .unwrap();
    drop(f._driver.output);
    let final_state = f.task.join_with_receipt().await.unwrap().state;
    assert_eq!(final_state.lifecycle, Lifecycle::Ended);
    loop {
        let tauri::ipc::InvokeResponseBody::Raw(wire) = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap()
        else {
            panic!("non raw")
        };
        let frame = pulse_ipc::decode(&wire).unwrap();
        let part = pulse_ipc::attach_v2::decode_fragment(frame.payload)
            .unwrap()
            .part;
        f.bank.ack(f.nonce, key, frame.sequence).unwrap();
        if part == pulse_ipc::attach::Part::End {
            break;
        }
    }
    let entry = f.bank.retained_entry(f.nonce, key).unwrap();
    timeout(Duration::from_secs(2), async {
        while !entry.completed.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let request = pulse_term::HistorySearchRequest {
        query: "EOF needle".into(),
        match_case: false,
        regex: false,
        whole_word: false,
        backwards: false,
        alternate: false,
        cursor: None,
        revision: None,
    };
    assert!(
        f.registry
            .history_search(&f.page, f.id, request.clone())
            .await
            .unwrap()
            .matched_row
            .is_some()
    );
    assert!(
        f.registry
            .history_page(&f.page, f.id, None, None)
            .await
            .unwrap()
            .total_rows
            > 0
    );
    assert_eq!(f.bank.credit.available_permits(), MAX_VIEWS - 1);
    assert!(
        !entry
            .input_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
    );
    assert!(f.bank.enable_input(f.nonce, key).await.is_err());
    assert!(f.bank.input(f.nonce, key, b"never".to_vec()).await.is_err());
    assert!(f.bank.resize(f.nonce, key, 48, 10).await.is_err());
    assert!(f.bank.detach(Uuid::new_v4(), key).is_err());
    assert!(
        f.registry
            .history_search(&f.page, f.id, request.clone())
            .await
            .is_ok()
    );
    f.bank.detach(f.nonce, key).unwrap();
    drop(entry);
    assert!(
        f.registry
            .history_search(&f.page, f.id, request)
            .await
            .is_err()
    );
    timeout(Duration::from_secs(2), f.bank.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.bank.credit.available_permits(), MAX_VIEWS);
}
