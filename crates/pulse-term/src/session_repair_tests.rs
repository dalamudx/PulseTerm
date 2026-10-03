#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn synchronized_repair_keeps_utf8_tail_and_rejects_active_bsu_without_flushing() {
    let (t, d) = ChannelTransport::bounded_driver();
    let (h, task) = spawn_session(size(), 0, t);
    let mut s = h
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let (_, cancelled) = watch::channel(false);
    let prefix = b"text\xe4\xb8";
    d.output.send(prefix.to_vec()).await.unwrap();
    s.output.recv().await.unwrap();
    let state = h.state.borrow().clone();
    assert_eq!(
        h.diagnostic_repair(request(
            &s,
            state,
            "0000000000000000".into(),
            cancelled.clone()
        ))
        .await
        .unwrap(),
        RepairAdmission::Queued
    );
    let StreamRecord::Repair { bytes, hold, .. } = s.output.recv().await.unwrap() else {
        panic!("repair")
    };
    assert!(bytes.starts_with(b"\x18\x1b[?2026h"));
    assert!(bytes.ends_with(b"\x1b[?2026l\xe4\xb8"));
    let mut source = TerminalProbe::new(size(), 0);
    source.feed(prefix);
    source.feed(b"\xad!");
    let mut display = TerminalProbe::new(size(), 0);
    display.feed(b"wrong");
    display.feed(&bytes);
    display.feed(b"\xad!");
    assert_eq!(source.screen(), display.screen());
    drop(hold);
    d.output.send(b"\x1b[?2026hwaiting".to_vec()).await.unwrap();
    s.output.recv().await.unwrap();
    let state = h.state.borrow().clone();
    assert!(matches!(
        h.diagnostic_repair(request(&s, state, "0000000000000000".into(), cancelled))
            .await,
        Err(SessionRestoreError::Gap(crate::RestoreGap::SyncPending))
    ));
    assert!(s.output.try_recv().is_err());
    h.close();
    drop(d);
    task.join().await.unwrap();
}

fn size() -> TermSize {
    TermSize::new(32, 8).unwrap()
}
fn request(
    stream: &SessionStream,
    state: SessionState,
    digest: String,
    cancelled: watch::Receiver<bool>,
) -> SessionRepairRequest {
    SessionRepairRequest {
        lease_id: stream.lease_id,
        boundary: state,
        digest,
        cancelled,
    }
}
#[tokio::test]
async fn repair_atomically_rejects_stale_and_equal_and_preserves_csi_and_history() {
    let (t, d) = ChannelTransport::bounded_driver();
    let (h, task) = spawn_session(size(), 128, t);
    let mut s = h
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let mut prefix = Vec::new();
    for i in 0..20 {
        prefix.extend_from_slice(format!("history{i}\r\n").as_bytes());
    }
    prefix.extend_from_slice(b"CURRENT\x1b[31");
    d.output.send(prefix.clone()).await.unwrap();
    s.output.recv().await.unwrap();
    let digest = h.diagnostic_digest().await.unwrap();
    let (c, watch) = watch::channel(false);
    assert_eq!(
        h.diagnostic_repair(request(
            &s,
            digest.boundary.clone(),
            digest.digest.clone(),
            watch.clone()
        ))
        .await
        .unwrap(),
        RepairAdmission::Equal
    );
    assert!(s.output.try_recv().is_err());
    let mut stale = digest.boundary.clone();
    stale.revision += 1;
    assert_eq!(
        h.diagnostic_repair(request(&s, stale, "0000000000000000".into(), watch.clone()))
            .await
            .unwrap(),
        RepairAdmission::Stale
    );
    assert_eq!(
        h.diagnostic_repair(request(
            &s,
            digest.boundary.clone(),
            "0000000000000000".into(),
            watch.clone()
        ))
        .await
        .unwrap(),
        RepairAdmission::Queued
    );
    assert!(matches!(
        h.diagnostic_repair(request(
            &s,
            digest.boundary.clone(),
            "0000000000000000".into(),
            watch.clone()
        ))
        .await,
        Err(SessionRestoreError::Session(SessionError::Busy))
    ));
    let StreamRecord::Repair { bytes, hold, .. } = s.output.recv().await.unwrap() else {
        panic!("repair expected")
    };
    let mut original = TerminalProbe::new(size(), 128);
    original.feed(&prefix);
    let mut restored = TerminalProbe::new(size(), 128);
    restored.feed(&prefix);
    restored.feed(b"\x18\x1b[1;1H!");
    restored.feed(&bytes);
    original.feed(b"mZ");
    restored.feed(b"mZ");
    assert_eq!(original.screen(), restored.screen());
    assert_eq!(
        original
            .recent_history_projection(128)
            .into_iter()
            .map(|r| r.cells)
            .collect::<Vec<_>>(),
        restored
            .recent_history_projection(128)
            .into_iter()
            .map(|r| r.cells)
            .collect::<Vec<_>>()
    );
    drop(hold);
    c.send_replace(true);
    assert!(matches!(
        h.diagnostic_repair(request(
            &s,
            digest.boundary,
            "0000000000000000".into(),
            watch
        ))
        .await,
        Err(SessionRestoreError::Session(SessionError::StaleLease))
    ));
    h.close();
    drop(d);
    task.join().await.unwrap();
}
#[tokio::test]
async fn repair_rejects_nonexpressible_wrap_and_old_lease_without_partial_record() {
    let (t, d) = ChannelTransport::bounded_driver();
    let (h, task) = spawn_session(size(), 0, t);
    let mut s = h
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let (_, watch) = watch::channel(false);
    let stale = h
        .diagnostic_subscribe(0, 0, MAX_RESTORE_BYTES)
        .await
        .unwrap();
    let state = h.state.borrow().clone();
    assert!(matches!(
        h.diagnostic_repair(request(&s, state, "0000000000000000".into(), watch.clone()))
            .await,
        Err(SessionRestoreError::Session(SessionError::StaleLease))
    ));
    s = stale;
    let mut text = vec![b'x'; 32];
    text.extend_from_slice(b"\x1b[Z");
    d.output.send(text).await.unwrap();
    s.output.recv().await.unwrap();
    let state = h.state.borrow().clone();
    assert!(matches!(
        h.diagnostic_repair(request(&s, state, "0000000000000000".into(), watch))
            .await,
        Err(SessionRestoreError::Gap(
            crate::RestoreGap::NonEdgePendingWrap
        ))
    ));
    assert!(s.output.try_recv().is_err());
    assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
    h.close();
    drop(d);
    task.join().await.unwrap();
}
