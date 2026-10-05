use super::*;
use crate::{
    vault_crypto::{CryptoError, SecretContext},
    vault_kdf::TEST_SCENARIOS,
    vault_lifecycle::KeyStatus,
    vault_wrap::create_validation_vault,
};
use std::{
    sync::{
        Arc, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::runtime::{Builder, Runtime};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn scenario() -> Result<MutexGuard<'static, ()>, KdfError> {
    TEST_SCENARIOS.lock().map_err(|_| KdfError::Busy)
}
fn id() -> Uuid {
    Uuid::from_bytes([1; 16])
}
fn runtime() -> std::io::Result<Runtime> {
    Builder::new_current_thread()
        .enable_time()
        .max_blocking_threads(1)
        .build()
}
fn proof() -> Result<AuthenticatedDek, UnlockError> {
    Ok(AuthenticatedDek::fixture(id()).map_err(WrapError::Crypto)?)
}
fn quota() -> Result<(KdfJob, oneshot::Receiver<()>), KdfError> {
    let mut job = admit_validation_job()?;
    let (tx, rx) = oneshot::channel();
    job.released = Some(tx);
    Ok((job, rx))
}
fn observe<T>(rt: &Runtime, rx: oneshot::Receiver<T>) -> Result<T, Box<dyn std::error::Error>> {
    Ok(rt.block_on(async { tokio::time::timeout(Duration::from_secs(5), rx).await })??)
}
type GatedJob = (
    ValidationUnlockRequest,
    oneshot::Receiver<()>,
    std::sync::mpsc::SyncSender<()>,
    oneshot::Receiver<()>,
);
fn gated(rt: &Runtime, scope: &VaultKeyLifecycle) -> Result<GatedJob, UnlockError> {
    let (job, freed) = quota()?;
    let (start, started) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let request = dispatch(rt.handle(), scope, job, move |_| {
        let _sent = start.send(());
        wait.recv_timeout(Duration::from_secs(5))
            .map_err(|_| UnlockError::Worker)?;
        proof()
    })?;
    Ok((request, started, release, freed))
}
#[test]
fn joined_candidate_waits_for_correct_scope_confirmation() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let scope = VaultKeyLifecycle::new(id())?;
    let request = dispatch(rt.handle(), &scope, admit_validation_job()?, |_| proof())?;
    let mut ready = rt.block_on(request.ready())?;
    assert_eq!(scope.status()?, KeyStatus::Pending);
    let foreign = VaultKeyLifecycle::new(id())?;
    assert!(matches!(
        ready.confirm(&foreign),
        Err(UnlockError::Lifecycle(LifecycleError::Stale))
    ));
    ready.confirm(&scope)?;
    assert_eq!(scope.status()?, KeyStatus::Available);
    assert!(ready.confirm(&scope).is_err());
    Ok(())
}
#[test]
fn running_cancel_keeps_quota_until_actual_join_then_releases() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let scope = VaultKeyLifecycle::new(id())?;
    let (request, started, release, freed) = gated(&rt, &scope)?;
    observe(&rt, started)?;
    let canceled = request.cancel();
    assert_eq!(scope.status()?, KeyStatus::Locked);
    assert!(matches!(admit_validation_job(), Err(KdfError::Busy)));
    assert!(matches!(
        crate::vault_kdf::derive_validation_kek(b"valid", &[0; 16]),
        Err(KdfError::Busy)
    ));
    release.send(())?;
    rt.block_on(canceled.join())?;
    observe(&rt, freed)?;
    assert_eq!(scope.status()?, KeyStatus::Locked);
    let _free = admit_validation_job()?;
    Ok(())
}
#[test]
fn consuming_ready_timeout_cancels_but_does_not_release_running_quota() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let scope = VaultKeyLifecycle::new(id())?;
    let (request, started, release, freed) = gated(&rt, &scope)?;
    observe(&rt, started)?;
    let timed = rt
        .block_on(async { tokio::time::timeout(Duration::from_millis(5), request.ready()).await });
    assert!(timed.is_err());
    assert_eq!(scope.status()?, KeyStatus::Locked);
    assert!(matches!(admit_validation_job(), Err(KdfError::Busy)));
    release.send(())?;
    observe(&rt, freed)?;
    assert_eq!(scope.status()?, KeyStatus::Locked);
    Ok(())
}
#[test]
fn queued_cancel_skips_computation_and_holds_quota_until_closure_drops() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let scope = VaultKeyLifecycle::new(id())?;
    let (start, started) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let blocker = rt.spawn_blocking(move || {
        let _sent = start.send(());
        wait.recv_timeout(Duration::from_secs(5))
    });
    observe(&rt, started)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let (job, freed) = quota()?;
    let request = dispatch(rt.handle(), &scope, job, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        proof()
    })?;
    let canceled = request.cancel();
    assert!(matches!(admit_validation_job(), Err(KdfError::Busy)));
    release.send(())?;
    rt.block_on(blocker)??;
    rt.block_on(canceled.join())?;
    observe(&rt, freed)?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(scope.status()?, KeyStatus::Locked);
    Ok(())
}
#[test]
fn lock_during_work_rejects_late_key_and_does_not_touch_new_epoch() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let scope = VaultKeyLifecycle::new(id())?;
    let (request, started, release, freed) = gated(&rt, &scope)?;
    observe(&rt, started)?;
    scope.lock()?;
    let current = scope.begin()?;
    release.send(())?;
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(UnlockError::Lifecycle(LifecycleError::Stale))
    ));
    observe(&rt, freed)?;
    assert_eq!(scope.status()?, KeyStatus::Pending);
    drop(current);
    Ok(())
}
#[test]
fn delivered_but_unobserved_receipt_drop_removes_key_and_pending_state() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let scope = VaultKeyLifecycle::new(id())?;
    let (job, freed) = quota()?;
    let request = dispatch(rt.handle(), &scope, job, |_| proof())?;
    observe(&rt, freed)?;
    assert_eq!(scope.status()?, KeyStatus::Pending);
    rt.block_on(request.cancel().join())?;
    assert_eq!(scope.status()?, KeyStatus::Locked);
    Ok(())
}
#[test]
fn worker_panic_is_not_an_unlock_or_a_leaked_quota() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let scope = VaultKeyLifecycle::new(id())?;
    let (job, freed) = quota()?;
    let request = dispatch(rt.handle(), &scope, job, |_| panic!("fixed worker failure"))?;
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(UnlockError::Worker)
    ));
    observe(&rt, freed)?;
    assert_eq!(scope.status()?, KeyStatus::Locked);
    let _free = admit_validation_job()?;
    Ok(())
}
#[test]
fn invalid_password_runtime_and_foreign_envelope_do_not_take_quota_or_permit() -> TestResult {
    let _scenario = scenario()?;
    assert!(PasswordInput::new(Vec::new()).is_err());
    assert!(PasswordInput::new(vec![b'x'; 4097]).is_err());
    let scope = VaultKeyLifecycle::new(id())?;
    assert!(matches!(
        begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixture".to_vec())?,
            ValidationWrappedDek::malformed_fixture(id())
        ),
        Err(UnlockError::Runtime)
    ));
    let rt = runtime()?;
    let _entered = rt.enter();
    let _held = admit_validation_job()?;
    let wrong = ValidationWrappedDek::malformed_fixture(Uuid::from_bytes([2; 16]));
    assert!(matches!(
        begin_validation_unlock(&scope, PasswordInput::new(b"fixture".to_vec())?, wrong),
        Err(UnlockError::Wrap(WrapError::Envelope))
    ));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    Ok(())
}
#[test]
fn busy_global_admission_does_not_consume_foreign_or_current_scope_permit() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let active = VaultKeyLifecycle::new(id())?;
    let (request, started, release, freed) = gated(&rt, &active)?;
    observe(&rt, started)?;
    let untouched = VaultKeyLifecycle::new(id())?;
    {
        let _entered = rt.enter();
        for scope in [&untouched, &active] {
            assert!(matches!(
                begin_validation_unlock(
                    scope,
                    PasswordInput::new(b"fixture".to_vec())?,
                    ValidationWrappedDek::malformed_fixture(id())
                ),
                Err(UnlockError::Kdf(KdfError::Busy))
            ));
        }
    }
    assert_eq!(untouched.status()?, KeyStatus::Locked);
    assert_eq!(active.status()?, KeyStatus::Pending);
    release.send(())?;
    let mut ready = rt.block_on(request.ready())?;
    observe(&rt, freed)?;
    ready.confirm(&active)?;
    assert_eq!(active.status()?, KeyStatus::Available);
    Ok(())
}
#[test]
fn real_master_authentication_background_join_confirm_and_wrong_password() -> TestResult {
    let _scenario = scenario()?;
    let rt = runtime()?;
    let scope = VaultKeyLifecycle::new(id())?;
    let ctx = SecretContext::new(Uuid::from_bytes([2; 16]), "password")?;
    let (envelope, mut original) = create_validation_vault(b"fixed validation password", id())?;
    let credential = original.seal(&ctx, b"fixed fixture credential")?;
    original.lock();
    let request = {
        let _entered = rt.enter();
        begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed validation password".to_vec())?,
            envelope.clone(),
        )?
    };
    let mut ready = rt.block_on(request.ready())?;
    assert_eq!(scope.status()?, KeyStatus::Pending);
    ready.confirm(&scope)?;
    assert_eq!(
        scope.open_for_adapter(&ctx, &credential)?.expose(),
        b"fixed fixture credential"
    );
    scope.lock()?;
    let wrong = {
        let _entered = rt.enter();
        begin_validation_unlock(&scope, PasswordInput::new(b"incorrect".to_vec())?, envelope)?
    };
    assert!(matches!(
        rt.block_on(wrong.ready()),
        Err(UnlockError::Wrap(WrapError::Crypto(
            CryptoError::Authentication
        )))
    ));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    Ok(())
}
