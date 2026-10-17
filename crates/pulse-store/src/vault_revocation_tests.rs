use crate::vault_lifecycle::{KeyStatus, VaultKeyLifecycle};
use crate::vault_wrap::AuthenticatedDek;
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn explicit_epoch_handle_clears_late_activation_but_never_later_epoch() -> TestResult {
    let id = Uuid::now_v7();
    let scope = VaultKeyLifecycle::new(id)?;
    let mut first = scope.begin()?;
    let old = first.revocation();
    let mut candidate = first.worker()?.finish(AuthenticatedDek::fixture(id)?)?;
    scope.confirm(&mut first, &mut candidate)?;
    assert_eq!(scope.status()?, KeyStatus::Available);
    old.cancel();
    assert_eq!(scope.status()?, KeyStatus::Locked);
    let mut second = scope.begin()?;
    let new = second.revocation();
    let mut candidate = second.worker()?.finish(AuthenticatedDek::fixture(id)?)?;
    scope.confirm(&mut second, &mut candidate)?;
    old.cancel();
    assert_eq!(scope.status()?, KeyStatus::Available);
    new.cancel();
    assert_eq!(scope.status()?, KeyStatus::Locked);
    Ok(())
}
#[test]
fn queued_candidate_and_foreign_controller_remain_separate() -> TestResult {
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let first = VaultKeyLifecycle::new(a)?;
    let second = VaultKeyLifecycle::new(b)?;
    let mut permit = first.begin()?;
    let cancel = permit.revocation();
    let worker = permit.worker()?;
    cancel.cancel();
    assert!(worker.finish(AuthenticatedDek::fixture(a)?).is_err());
    assert_eq!(first.status()?, KeyStatus::Locked);
    let mut permit = second.begin()?;
    let mut candidate = permit.worker()?.finish(AuthenticatedDek::fixture(b)?)?;
    second.confirm(&mut permit, &mut candidate)?;
    cancel.cancel();
    assert_eq!(second.status()?, KeyStatus::Available);
    Ok(())
}
