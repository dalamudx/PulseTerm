#![allow(clippy::unwrap_used)]
use super::*;
#[test]
fn broken_wide_head_is_explicitly_nonexact_for_xterm() {
    let mut term = TerminalProbe::new(TermSize::new(32, 8).unwrap(), 0);
    term.feed("\x1b[3;1H中文中\x1b[3;2H\x1b[K".as_bytes());
    let frame = term.restore_screen_bounded(1024 * 1024).unwrap();
    assert!(
        frame
            .approximations
            .contains(&"wide head with erased spacer cannot be represented by xterm")
    );
}
#[tokio::test]
async fn digest_clears_risk_only_after_same_boundary_match() {
    let size = TermSize::new(32, 8).unwrap();
    let (transport, _) = EchoTransport::spawn();
    let (handle, task) = spawn_session(size, 0, transport);
    let input = b"q\x1b[2b\x1b[L";
    handle.input(input.to_vec()).await.unwrap();
    let mut state = handle.state.clone();
    while state.borrow().parsed_bytes < input.len() as u64 {
        state.changed().await.unwrap();
    }
    let check = handle.diagnostic_digest().await.unwrap();
    assert!(check.risk & crate::reconciliation::REP != 0);
    assert!(check.risk & crate::reconciliation::LINES != 0);
    assert_eq!(
        handle
            .verify_screen(check.boundary.clone(), "0000000000000000".into())
            .await
            .unwrap(),
        ScreenVerification::Mismatch
    );
    assert_eq!(handle.diagnostic_digest().await.unwrap().risk, check.risk);
    handle.set_cell_pixels(Some((8, 16))).await.unwrap();
    assert_eq!(
        handle
            .verify_screen(check.boundary, check.digest)
            .await
            .unwrap(),
        ScreenVerification::Stale
    );
    let check = handle.diagnostic_digest().await.unwrap();
    assert_ne!(check.risk, 0);
    assert_eq!(
        handle
            .verify_screen(check.boundary, check.digest)
            .await
            .unwrap(),
        ScreenVerification::Equal
    );
    assert_eq!(handle.diagnostic_digest().await.unwrap().risk, 0);
    handle.close();
    task.join().await.unwrap();
}
#[test]
fn risk_is_chunk_independent_rep_and_reset_never_mask_unverified_changes() {
    let bytes = b"q\x1b[2b\x1b[L\x1bc";
    for chunk in 1..=bytes.len() {
        let mut term = TerminalProbe::new(TermSize::new(32, 8).unwrap(), 0);
        for part in bytes.chunks(chunk) {
            term.feed(part);
        }
        assert_eq!(
            term.divergence_risk(),
            crate::reconciliation::LINES | crate::reconciliation::REP
        );
    }
}
