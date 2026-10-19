use super::*;
#[cfg(unix)]
use crate::{
    StateStore, vault_database::VaultEdit, vault_lifecycle::VaultKeyLifecycle,
    vault_unlock::begin_validation_unlock,
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn runtime(threads: usize) -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(threads)
        .build()
}
fn envelope(id: Uuid) -> Result<ValidationWrappedDek, WrapError> {
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    ValidationWrappedDek::decode_ciphertext(&bytes, id)
}
#[cfg(unix)]
#[test]
fn actual_creation_stays_locked_persists_once_and_requires_authenticated_source_confirmation()
-> TestResult {
    let _serial = crate::vault_kdf::TEST_SCENARIOS
        .lock()
        .map_err(|_| "fixture.poison")?;
    // One resident SQL worker plus one pre-admitted KDF worker; limiter remains one.
    runtime(2)?.block_on(async {
        let id = Uuid::now_v7();
        let scope = VaultKeyLifecycle::new(id)?;
        let root = tempfile::tempdir()?;
        let dir = crate::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let ready = begin_validation_creation(
            PasswordInput::new(b"fixed-validation-create-password".to_vec())?,
            id,
            Arc::new(|| true),
        )?
        .ready()
        .await?;
        assert_ne!(
            scope.status()?,
            crate::vault_lifecycle::KeyStatus::Available
        );
        let wrapped = ready.envelope()?;
        let gate = ready.persistence_authorization();
        let saved = store
            .edit_vault_ciphertext(
                empty.clone(),
                VaultEdit::Initialize(id, wrapped.clone()),
                gate.clone(),
            )
            .await?;
        assert_ne!(
            scope.status()?,
            crate::vault_lifecycle::KeyStatus::Available
        );
        assert!(
            store
                .edit_vault_ciphertext(
                    empty,
                    VaultEdit::Initialize(Uuid::now_v7(), wrapped.clone()),
                    gate.clone()
                )
                .await
                .is_err()
        );
        drop(ready);
        assert!(!gate());
        let unlock = begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed-validation-create-password".to_vec())?,
            wrapped,
        )?
        .ready()
        .await?;
        assert_ne!(
            scope.status()?,
            crate::vault_lifecycle::KeyStatus::Available
        );
        store
            .confirm_vault_unlock(saved, scope.clone(), unlock, Arc::new(|| true))
            .await?;
        assert_eq!(
            scope.status()?,
            crate::vault_lifecycle::KeyStatus::Available
        );
        scope.lock()?;
        assert_ne!(
            scope.status()?,
            crate::vault_lifecycle::KeyStatus::Available
        );
        drop(store);
        worker.join().await?;
        Ok(())
    })
}
#[test]
fn queued_cancel_skips_creation_and_retains_quota_until_actual_closure_exit() -> TestResult {
    let _serial = crate::vault_kdf::TEST_SCENARIOS
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime(1)?.block_on(async {
        let (entered_rx, release_tx, blocker) = block();
        entered_rx.await?;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let work_calls = calls.clone();
        let request = dispatch(
            &Handle::current(),
            admit_validation_job()?,
            Arc::new(|| true),
            move |_| {
                work_calls.fetch_add(1, Ordering::SeqCst);
                envelope(Uuid::now_v7()).map_err(Into::into)
            },
        )?;
        request.cancel();
        assert!(matches!(admit_validation_job(), Err(KdfError::Busy)));
        release_tx.send(())?;
        blocker.await?;
        request.cancel_and_join().await?;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(admit_validation_job().is_ok());
        Ok(())
    })
}
#[test]
fn running_cancel_and_revoked_receipt_never_release_quota_or_publish_early() -> TestResult {
    let _serial = crate::vault_kdf::TEST_SCENARIOS
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime(1)?.block_on(async {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let request = dispatch(
            &Handle::current(),
            admit_validation_job()?,
            Arc::new(|| true),
            move |_| {
                let _sent = entered_tx.send(());
                release_rx.recv().map_err(|_| CreationError::Worker)?;
                envelope(Uuid::now_v7()).map_err(Into::into)
            },
        )?;
        entered_rx.await?;
        request.cancel();
        assert!(matches!(admit_validation_job(), Err(KdfError::Busy)));
        release_tx.send(())?;
        assert!(matches!(
            request.ready().await,
            Err(CreationError::Cancelled)
        ));
        assert!(admit_validation_job().is_ok());
        let live = Arc::new(AtomicBool::new(true));
        let authorize = live.clone();
        let ready = dispatch(
            &Handle::current(),
            admit_validation_job()?,
            Arc::new(move || authorize.load(Ordering::SeqCst)),
            move |_| envelope(Uuid::now_v7()).map_err(Into::into),
        )?
        .ready()
        .await?;
        let gate = ready.persistence_authorization();
        assert!(gate());
        live.store(false, Ordering::SeqCst);
        assert!(matches!(ready.envelope(), Err(CreationError::Cancelled)));
        assert!(!gate());
        drop(ready);
        assert!(!gate());
        Ok(())
    })
}
#[test]
fn dropped_ready_guard_revokes_only_its_persistence_gate_and_panics_are_not_results() -> TestResult
{
    let _serial = crate::vault_kdf::TEST_SCENARIOS
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime(1)?.block_on(async {
        let old = dispatch(
            &Handle::current(),
            admit_validation_job()?,
            Arc::new(|| true),
            move |_| envelope(Uuid::now_v7()).map_err(Into::into),
        )?
        .ready()
        .await?;
        let old_gate = old.persistence_authorization();
        let new = dispatch(
            &Handle::current(),
            admit_validation_job()?,
            Arc::new(|| true),
            move |_| envelope(Uuid::now_v7()).map_err(Into::into),
        )?
        .ready()
        .await?;
        let new_gate = new.persistence_authorization();
        drop(old);
        assert!(!old_gate());
        assert!(new_gate());
        drop(new);
        assert!(!new_gate());
        let bad = dispatch(
            &Handle::current(),
            admit_validation_job()?,
            Arc::new(|| true),
            move |_| panic!("fixture.panic"),
        )?;
        assert!(matches!(bad.ready().await, Err(CreationError::Worker)));
        assert!(admit_validation_job().is_ok());
        Ok(())
    })
}
type Blocker = (
    oneshot::Receiver<()>,
    std::sync::mpsc::Sender<()>,
    JoinHandle<()>,
);
fn block() -> Blocker {
    let (tx, rx) = oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let task = tokio::task::spawn_blocking(move || {
        let _sent = tx.send(());
        let _released = release_rx.recv();
    });
    (rx, release_tx, task)
}
