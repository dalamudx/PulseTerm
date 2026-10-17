#![allow(clippy::unwrap_used)]
use super::*;
fn fixture() -> (
    Arc<ProductDocuments>,
    WindowToken,
    Arc<PageLaunchScope>,
    Arc<Selections>,
) {
    let registry = pulse_app::registry::SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let documents = Arc::new(ProductDocuments::new(registry));
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let page = documents.current_native(owner).unwrap();
    let bank = Arc::new(Selections::new(documents.clone(), owner));
    (documents, owner, page, bank)
}
#[tokio::test(flavor = "current_thread")]
async fn event_delivery_is_not_completion_and_exact_reply_is_single_use() {
    let (_, _, page, bank) = fixture();
    let mut pending = bank
        .begin(page.clone(), 7, CancellationToken::new())
        .unwrap();
    assert!(matches!(
        pending.answer.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert!(bank.complete(&page, Uuid::new_v4(), 7, true).is_err());
    assert!(bank.complete(&page, pending.id, 8, true).is_err());
    assert!(matches!(
        pending.answer.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert!(bank.complete(&page, pending.id, 7, true).is_err());
    assert!(bank.admit(&page, pending.id, 8).is_err());
    bank.admit(&page, pending.id, 7).unwrap();
    assert!(bank.admit(&page, pending.id, 7).is_err());
    bank.complete(&page, pending.id, 7, true).unwrap();
    assert_eq!((&mut pending.answer).await.unwrap(), Ok(()));
    assert!(bank.complete(&page, pending.id, 7, true).is_err());
}
#[test]
fn admission_rejects_forged_foreign_cancelled_refreshed_and_finished_intents() {
    let (docs, owner, page, bank) = fixture();
    let stop = CancellationToken::new();
    let pending = bank.begin(page.clone(), 1, stop.clone()).unwrap();
    let (_, _, other, other_bank) = fixture();
    assert!(bank.admit(&page, Uuid::new_v4(), 1).is_err());
    assert!(bank.admit(&other, pending.id, 1).is_err());
    assert!(other_bank.admit(&page, pending.id, 1).is_err());
    stop.cancel();
    assert!(bank.admit(&page, pending.id, 1).is_err());
    drop(pending);
    let newer = bank
        .begin(page.clone(), 1, CancellationToken::new())
        .unwrap();
    docs.started(owner).unwrap();
    assert!(bank.admit(&page, newer.id, 1).is_err());
    let nonce = docs.finished(owner).unwrap();
    docs.confirm(owner, nonce).unwrap();
    let fresh = docs.current_native(owner).unwrap();
    assert!(bank.admit(&fresh, newer.id, 1).is_err());
    bank.retire();
    let last = bank
        .begin(fresh.clone(), 1, CancellationToken::new())
        .unwrap();
    bank.admit(&fresh, last.id, 1).unwrap();
    bank.complete(&fresh, last.id, 1, false).unwrap();
    assert!(bank.admit(&fresh, last.id, 1).is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn foreign_page_bank_and_refresh_cannot_upgrade_old_ticket() {
    let (docs, owner, page, bank) = fixture();
    let mut pending = bank
        .begin(page.clone(), 1, CancellationToken::new())
        .unwrap();
    let (_, _, foreign, foreign_bank) = fixture();
    assert!(bank.complete(&foreign, pending.id, 1, true).is_err());
    assert!(foreign_bank.complete(&page, pending.id, 1, true).is_err());
    assert!(bank.begin(foreign, 1, CancellationToken::new()).is_err());
    docs.started(owner).unwrap();
    assert!(bank.complete(&page, pending.id, 1, true).is_err());
    let nonce = docs.finished(owner).unwrap();
    docs.confirm(owner, nonce).unwrap();
    let fresh = docs.current_native(owner).unwrap();
    assert!(bank.complete(&fresh, pending.id, 1, true).is_err());
    bank.retire();
    assert!((&mut pending.answer).await.is_err());
    let newer = bank
        .begin(fresh.clone(), 1, CancellationToken::new())
        .unwrap();
    drop(pending); // Must not remove the newer page's reservation.
    bank.admit(&fresh, newer.id, 1).unwrap();
    bank.complete(&fresh, newer.id, 1, true).unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn cancelled_expired_lost_or_negative_completion_never_succeeds() {
    let (_, _, page, bank) = fixture();
    let stop = CancellationToken::new();
    let mut cancelled = bank.begin(page.clone(), 1, stop.clone()).unwrap();
    stop.cancel();
    assert!(bank.complete(&page, cancelled.id, 1, true).is_err());
    assert!((&mut cancelled.answer).await.is_err());
    let mut expired = bank
        .begin(page.clone(), 1, CancellationToken::new())
        .unwrap();
    assert!(
        bank.complete_at(
            &page,
            expired.id,
            1,
            true,
            Instant::now() + Duration::from_secs(5)
        )
        .is_err()
    );
    assert!((&mut expired.answer).await.is_err());
    let mut lost = bank
        .begin(page.clone(), 1, CancellationToken::new())
        .unwrap();
    lost.answer.close();
    assert!(bank.complete(&page, lost.id, 1, true).is_err());
    let mut rejected = bank
        .begin(page.clone(), 1, CancellationToken::new())
        .unwrap();
    bank.complete(&page, rejected.id, 1, false).unwrap();
    assert_eq!((&mut rejected.answer).await.unwrap(), Err(Error));
}
#[test]
fn closure_revocation_busy_and_guard_drop_are_bounded() {
    let (docs, owner, page, bank) = fixture();
    assert!(
        bank.begin(page.clone(), 0, CancellationToken::new())
            .is_err()
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(bank.begin(page.clone(), 1, cancelled).is_err());
    let first = bank
        .begin(page.clone(), 1, CancellationToken::new())
        .unwrap();
    assert!(
        bank.begin(page.clone(), 2, CancellationToken::new())
            .is_err()
    );
    drop(first);
    let second = bank
        .begin(page.clone(), 2, CancellationToken::new())
        .unwrap();
    bank.close();
    assert!(bank.complete(&page, second.id, 2, true).is_err());
    assert!(
        bank.begin(page.clone(), 2, CancellationToken::new())
            .is_err()
    );
    let other_bank = Arc::new(Selections::new(docs.clone(), owner));
    let third = other_bank
        .begin(page.clone(), 3, CancellationToken::new())
        .unwrap();
    docs.remove(owner).unwrap();
    assert!(other_bank.complete(&page, third.id, 3, true).is_err());
}
