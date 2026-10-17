use crate::{
    StateStore, StoreError,
    vault_crypto::{SealedSecret, SecretContext, VaultCipher},
    vault_database::{self, VaultEdit, VaultSnapshot},
    vault_lifecycle::{KeyStatus, VaultKeyLifecycle},
    vault_record::CredentialRecord,
    vault_unlock::{PasswordInput, ReadyUnlock, begin_validation_unlock},
    vault_wrap::{ValidationWrappedDek, create_validation_vault},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn caller_cancel_clears_candidate_immediately_but_credit_and_shutdown_wait_for_native_exit()
-> TestResult {
    let d = private()?;
    let (store, worker) = StateStore::open(d.path().join("state.db")).await?;
    let value = seed(&store).await?;
    let scope = VaultKeyLifecycle::new(value.vault_id().ok_or("fixture.id")?)?;
    let receipt = ready(&scope, &value)?;
    let quota = Arc::new(tokio::sync::Semaphore::new(1));
    let credit = quota.clone().acquire_owned().await?;
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let resume = std::sync::Mutex::new(resume_rx);
    let count = AtomicUsize::new(0);
    let gate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        let _keep_credit = &credit;
        if count.fetch_add(1, Ordering::SeqCst) == 0 {
            if entered_tx.send(()).is_err() {
                return false;
            }
            let Ok(receiver) = resume.lock() else {
                return false;
            };
            if receiver.recv().is_err() {
                return false;
            }
        }
        true
    });
    let request_store = store.clone();
    let request_scope = scope.clone();
    let request = tokio::spawn(async move {
        request_store
            .confirm_vault_unlock(value, request_scope, receipt, gate)
            .await
    });
    entered_rx.recv_timeout(std::time::Duration::from_secs(2))?;
    assert_eq!(scope.status()?, KeyStatus::Pending);
    request.abort();
    assert!(matches!(request.await,Err(error) if error.is_cancelled()));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    assert_eq!(quota.available_permits(), 0);
    worker.cancel();
    let join = tokio::spawn(async move { worker.join().await });
    tokio::task::yield_now().await;
    assert!(!join.is_finished());
    resume_tx.send(())?;
    join.await??;
    assert_eq!(quota.available_permits(), 1);
    assert_eq!(scope.status()?, KeyStatus::Locked);
    drop(store);
    Ok(())
}

fn private() -> std::io::Result<tempfile::TempDir> {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(d)
}
fn allowed() -> Arc<dyn Fn() -> bool + Send + Sync> {
    Arc::new(|| true)
}
fn envelope(id: Uuid) -> Result<ValidationWrappedDek, Box<dyn std::error::Error>> {
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    Ok(ValidationWrappedDek::decode_ciphertext(&bytes, id)?)
}
async fn seed(store: &StateStore) -> Result<VaultSnapshot, Box<dyn std::error::Error>> {
    let id = Uuid::now_v7();
    let empty = store.load_vault_ciphertext(allowed()).await?;
    Ok(store
        .edit_vault_ciphertext(empty, VaultEdit::Initialize(id, envelope(id)?), allowed())
        .await?)
}
fn ready(
    scope: &VaultKeyLifecycle,
    snapshot: &VaultSnapshot,
) -> Result<ReadyUnlock, Box<dyn std::error::Error>> {
    Ok(ReadyUnlock::fixture(
        scope,
        snapshot
            .envelope()
            .ok_or("fixture.envelope")?
            .encode_ciphertext()?,
    )?)
}
#[tokio::test]
async fn exact_current_source_confirms_but_stale_revision_and_foreign_store_do_not() -> TestResult {
    let d = private()?;
    let (store, worker) = StateStore::open(d.path().join("state.db")).await?;
    let first = seed(&store).await?;
    let id = first.vault_id().ok_or("fixture.id")?;
    let scope = VaultKeyLifecycle::new(id)?;
    store
        .confirm_vault_unlock(
            first.clone(),
            scope.clone(),
            ready(&scope, &first)?,
            allowed(),
        )
        .await?;
    assert_eq!(scope.status()?, KeyStatus::Available);
    scope.lock()?;
    let key = VaultCipher::generate()?;
    let record = CredentialRecord::seal(id, Uuid::now_v7(), "ssh_password", &key, b"fixture")?;
    store
        .edit_vault_ciphertext(first.clone(), VaultEdit::Create(record), allowed())
        .await?;
    assert!(matches!(
        store
            .confirm_vault_unlock(
                first.clone(),
                scope.clone(),
                ready(&scope, &first)?,
                allowed()
            )
            .await,
        Err(StoreError::VaultConflict)
    ));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    let (foreign, other) = StateStore::open(d.path().join("foreign.db")).await?;
    assert!(matches!(
        foreign
            .confirm_vault_unlock(
                first.clone(),
                scope.clone(),
                ready(&scope, &first)?,
                allowed()
            )
            .await,
        Err(StoreError::VaultConflict)
    ));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    drop(foreign);
    other.join().await?;
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn same_revision_ciphertext_change_and_wrong_authenticated_envelope_are_rejected()
-> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let value = seed(&store).await?;
    let id = value.vault_id().ok_or("fixture.id")?;
    let scope = VaultKeyLifecycle::new(id)?;
    let mut wrong = value
        .envelope()
        .ok_or("fixture.envelope")?
        .encode_ciphertext()?;
    wrong[109] ^= 1;
    let receipt = ReadyUnlock::fixture(&scope, wrong)?;
    assert!(matches!(
        store
            .confirm_vault_unlock(value.clone(), scope.clone(), receipt, allowed())
            .await,
        Err(StoreError::VaultLifecycle(_))
    ));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    let receipt = ready(&scope, &value)?;
    let mut changed = value
        .envelope()
        .ok_or("fixture.envelope")?
        .encode_ciphertext()?;
    changed[108] ^= 1;
    let c = rusqlite::Connection::open(path)?;
    c.execute("UPDATE vault_envelopes SET envelope=?1", [changed])?;
    assert!(matches!(
        store
            .confirm_vault_unlock(value, scope.clone(), receipt, allowed())
            .await,
        Err(StoreError::VaultConflict)
    ));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    drop(c);
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn stale_epoch_foreign_controller_and_denied_or_panicking_authorization_never_enable()
-> TestResult {
    let d = private()?;
    let (store, worker) = StateStore::open(d.path().join("state.db")).await?;
    let value = seed(&store).await?;
    let id = value.vault_id().ok_or("fixture.id")?;
    let scope = VaultKeyLifecycle::new(id)?;
    let receipt = ready(&scope, &value)?;
    scope.lock()?;
    assert!(matches!(
        store
            .confirm_vault_unlock(value.clone(), scope.clone(), receipt, allowed())
            .await,
        Err(StoreError::VaultLifecycle(_))
    ));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    let foreign = VaultKeyLifecycle::new(id)?;
    assert!(matches!(
        store
            .confirm_vault_unlock(
                value.clone(),
                foreign.clone(),
                ready(&scope, &value)?,
                allowed()
            )
            .await,
        Err(StoreError::VaultLifecycle(_))
    ));
    assert_eq!(scope.status()?, KeyStatus::Locked);
    assert_eq!(foreign.status()?, KeyStatus::Locked);
    for panic in [false, true] {
        let gate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            if panic {
                panic!("fixture.auth_panic");
            }
            false
        });
        assert!(matches!(
            store
                .confirm_vault_unlock(value.clone(), scope.clone(), ready(&scope, &value)?, gate)
                .await,
            Err(StoreError::VaultAuthorization)
        ));
        assert_eq!(scope.status()?, KeyStatus::Locked);
    }
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn native_source_write_lock_covers_epoch_enable_boundary() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let value = seed(&store).await?;
    let scope = VaultKeyLifecycle::new(value.vault_id().ok_or("fixture.id")?)?;
    let stages = Arc::new(AtomicUsize::new(0));
    let observed = stages.clone();
    let gate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        if observed.fetch_add(1, Ordering::SeqCst) == 1 {
            let Ok(c) = rusqlite::Connection::open(&path) else {
                return false;
            };
            if c.busy_timeout(std::time::Duration::ZERO).is_err() {
                return false;
            }
            if !matches!(c.execute("UPDATE vault_envelopes SET revision=revision+1",[]),Err(rusqlite::Error::SqliteFailure(error,_)) if error.code==rusqlite::ErrorCode::DatabaseBusy || error.code==rusqlite::ErrorCode::DatabaseLocked)
            {
                return false;
            }
        }
        true
    });
    store
        .confirm_vault_unlock(value.clone(), scope.clone(), ready(&scope, &value)?, gate)
        .await?;
    assert_eq!(stages.load(Ordering::SeqCst), 2);
    assert_eq!(scope.status()?, KeyStatus::Available);
    scope.lock()?;
    drop(store);
    worker.join().await?;
    Ok(())
}
#[test]
fn undelivered_activation_clears_only_its_epoch_not_newer_unlock() -> TestResult {
    let d = private()?;
    let c = crate::database::open(&d.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let id = Uuid::now_v7();
    let empty = vault_database::load(&c, owner)?;
    let value = vault_database::edit(
        &c,
        owner,
        &empty,
        VaultEdit::Initialize(id, envelope(id)?),
        || true,
    )?;
    let scope = VaultKeyLifecycle::new(id)?;
    let old =
        vault_database::confirm_unlock(&c, owner, &value, &scope, ready(&scope, &value)?, || true)?;
    assert_eq!(scope.status()?, KeyStatus::Available);
    scope.lock()?;
    let new =
        vault_database::confirm_unlock(&c, owner, &value, &scope, ready(&scope, &value)?, || true)?;
    drop(old);
    assert_eq!(scope.status()?, KeyStatus::Available);
    drop(new);
    assert_eq!(scope.status()?, KeyStatus::Locked);
    Ok(())
}
#[test]
fn real_password_authentication_source_confirmation_restores_exact_credential() -> TestResult {
    let _serial = crate::vault_kdf::TEST_SCENARIOS
        .lock()
        .map_err(|_| "fixture.lock")?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let d = private()?;
        let (store, worker) = StateStore::open(d.path().join("state.db")).await?;
        let id = Uuid::now_v7();
        let credential = Uuid::now_v7();
        let (envelope, key) = create_validation_vault(b"fixed-source-password", id)?;
        let empty = store.load_vault_ciphertext(allowed()).await?;
        let value = store
            .edit_vault_ciphertext(
                empty,
                VaultEdit::Initialize(id, envelope.clone()),
                allowed(),
            )
            .await?;
        let value = store
            .edit_vault_ciphertext(
                value,
                VaultEdit::Create(CredentialRecord::seal(
                    id,
                    credential,
                    "ssh_password",
                    &key,
                    b"fixed-source-credential",
                )?),
                allowed(),
            )
            .await?;
        let scope = VaultKeyLifecycle::new(id)?;
        let receipt = begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed-source-password".to_vec())?,
            envelope,
        )?
        .ready()
        .await?;
        assert_eq!(scope.status()?, KeyStatus::Pending);
        store
            .confirm_vault_unlock(value.clone(), scope.clone(), receipt, allowed())
            .await?;
        let (nonce, ciphertext) = value.credentials()[0].storage_parts();
        let sealed = SealedSecret {
            nonce: *nonce,
            ciphertext: ciphertext.to_vec(),
        };
        assert_eq!(
            scope
                .open_for_adapter(
                    &SecretContext::bound_to_vault(id, credential, "ssh_password")?,
                    &sealed
                )?
                .expose(),
            b"fixed-source-credential"
        );
        scope.lock()?;
        drop(store);
        worker.join().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
