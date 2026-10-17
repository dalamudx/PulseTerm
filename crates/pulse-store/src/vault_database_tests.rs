use crate::{
    StateStore, StoreError,
    vault_crypto::VaultCipher,
    vault_database::VaultEdit,
    vault_record::CredentialRecord,
    vault_wrap::{ValidationWrappedDek, create_validation_vault, recover_validation_dek},
};
use std::sync::Arc;
use uuid::Uuid;
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
fn shape_envelope(id: Uuid) -> Result<ValidationWrappedDek, crate::vault_wrap::WrapError> {
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    ValidationWrappedDek::decode_ciphertext(&bytes, id)
}
#[tokio::test]
async fn instance_stale_delete_and_kind_transition_are_rejected_without_reinitialization()
-> Result<(), Box<dyn std::error::Error>> {
    let d = private()?;
    let other = private()?;
    let (store, worker) = StateStore::open(d.path().join("state.sqlite3")).await?;
    let (foreign, foreign_worker) = StateStore::open(other.path().join("state.sqlite3")).await?;
    let absent = store.load_vault_ciphertext(allowed()).await?;
    assert_eq!(absent.revision(), 0);
    assert!(absent.envelope().is_none());
    let id = Uuid::now_v7();
    let mut current = store
        .edit_vault_ciphertext(
            absent.clone(),
            VaultEdit::Initialize(id, shape_envelope(id)?),
            allowed(),
        )
        .await?;
    let other_capture = foreign.load_vault_ciphertext(allowed()).await?;
    assert!(matches!(
        store
            .edit_vault_ciphertext(
                other_capture,
                VaultEdit::Initialize(id, shape_envelope(id)?),
                allowed()
            )
            .await,
        Err(StoreError::VaultConflict)
    ));
    assert!(matches!(
        store
            .edit_vault_ciphertext(
                absent,
                VaultEdit::Initialize(id, shape_envelope(id)?),
                allowed()
            )
            .await,
        Err(StoreError::VaultConflict)
    ));
    assert!(matches!(
        store
            .edit_vault_ciphertext(
                current.clone(),
                VaultEdit::Initialize(id, shape_envelope(id)?),
                allowed()
            )
            .await,
        Err(StoreError::VaultConflict)
    ));
    let key = VaultCipher::generate()?;
    let credential = Uuid::now_v7();
    let record = CredentialRecord::seal(id, credential, "ssh_password", &key, b"fixture-secret")?;
    let prior = current.clone();
    current = store
        .edit_vault_ciphertext(current, VaultEdit::Create(record.clone()), allowed())
        .await?;
    assert!(matches!(
        store
            .edit_vault_ciphertext(prior, VaultEdit::Remove(credential), allowed())
            .await,
        Err(StoreError::VaultConflict)
    ));
    let changed_kind = CredentialRecord::seal(id, credential, "ssh_key", &key, b"fixture-key")?;
    assert!(matches!(
        store
            .edit_vault_ciphertext(current.clone(), VaultEdit::Replace(changed_kind), allowed())
            .await,
        Err(StoreError::VaultConflict)
    ));
    current = store
        .edit_vault_ciphertext(current, VaultEdit::Remove(credential), allowed())
        .await?;
    assert!(current.credentials().is_empty());
    assert!(current.envelope().is_some());
    assert!(matches!(
        store
            .edit_vault_ciphertext(
                current.clone(),
                VaultEdit::Create(record.clone()),
                allowed()
            )
            .await,
        Err(StoreError::VaultConflict)
    ));
    assert!(matches!(
        store
            .edit_vault_ciphertext(current, VaultEdit::Replace(record), allowed())
            .await,
        Err(StoreError::VaultConflict)
    ));
    drop(store);
    worker.join().await?;
    drop(foreign);
    foreign_worker.join().await?;
    Ok(())
}
#[test]
fn real_sqlite_reopen_authenticates_same_dek_and_never_stores_plaintext()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = crate::vault_kdf::TEST_SCENARIOS
        .lock()
        .map_err(|_| "fixture.lock")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let d = private()?;
    let path = d.path().join("state.sqlite3");
    let id = Uuid::now_v7();
    let credential = Uuid::now_v7();
    let password = zeroize::Zeroizing::new(b"fixed-test-password".to_vec());
    let (envelope, mut cipher) = create_validation_vault(&password, id)?;
    let record = CredentialRecord::seal(
        id,
        credential,
        "ssh_password",
        &cipher,
        b"fixed-test-credential",
    )?;
    runtime.block_on(async {
        let (store, worker) = StateStore::open(path.clone()).await?;
        let absent = store.load_vault_ciphertext(allowed()).await?;
        let initialized = store
            .edit_vault_ciphertext(absent, VaultEdit::Initialize(id, envelope), allowed())
            .await?;
        let saved = store
            .edit_vault_ciphertext(initialized, VaultEdit::Create(record), allowed())
            .await?;
        assert_eq!(saved.revision(), 2);
        drop(store);
        worker.join().await?;
        Ok::<_, StoreError>(())
    })?;
    cipher.lock();
    let connection = rusqlite::Connection::open(&path)?;
    let mut statement = connection
        .prepare("SELECT envelope FROM vault_envelopes UNION ALL SELECT ciphertext FROM secrets")?;
    let blobs = statement.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
    for bytes in blobs {
        let bytes = bytes?;
        for secret in [
            b"fixed-test-password".as_slice(),
            b"fixed-test-credential".as_slice(),
        ] {
            assert!(!bytes.windows(secret.len()).any(|w| w == secret));
        }
    }
    drop(statement);
    drop(connection);
    let reopened = runtime.block_on(async {
        let (store, worker) = StateStore::open(path).await?;
        let capture = store.load_vault_ciphertext(allowed()).await?;
        drop(store);
        worker.join().await?;
        Ok::<_, StoreError>(capture)
    })?;
    let envelope = reopened.envelope().ok_or("fixture.envelope")?;
    assert!(recover_validation_dek(b"wrong", id, envelope).is_err());
    let restored = recover_validation_dek(&password, id, envelope)?.into_cipher();
    assert_eq!(
        reopened.credentials()[0]
            .open(id, credential, "ssh_password", &restored)?
            .expose(),
        b"fixed-test-credential"
    );
    Ok(())
}
#[tokio::test]
async fn precommit_revocation_rolls_back_and_corrupt_source_never_becomes_absent()
-> Result<(), Box<dyn std::error::Error>> {
    let d = private()?;
    let path = d.path().join("state.sqlite3");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let absent = store.load_vault_ciphertext(allowed()).await?;
    let id = Uuid::now_v7();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let gate_count = count.clone();
    let gate = Arc::new(move || gate_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2);
    assert!(matches!(
        store
            .edit_vault_ciphertext(
                absent.clone(),
                VaultEdit::Initialize(id, shape_envelope(id)?),
                gate
            )
            .await,
        Err(StoreError::VaultAuthorization)
    ));
    assert_eq!(store.load_vault_ciphertext(allowed()).await?.revision(), 0);
    let saved = store
        .edit_vault_ciphertext(
            absent,
            VaultEdit::Initialize(id, shape_envelope(id)?),
            allowed(),
        )
        .await?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "PRAGMA ignore_check_constraints=ON; UPDATE vault_envelopes SET format=2;",
    )?;
    assert!(matches!(
        store.load_vault_ciphertext(allowed()).await,
        Err(StoreError::VaultRecord)
    ));
    assert!(matches!(
        store
            .edit_vault_ciphertext(
                saved,
                VaultEdit::Initialize(id, shape_envelope(id)?),
                allowed()
            )
            .await,
        Err(StoreError::VaultRecord)
    ));
    drop(connection);
    drop(store);
    worker.join().await?;
    Ok(())
}
