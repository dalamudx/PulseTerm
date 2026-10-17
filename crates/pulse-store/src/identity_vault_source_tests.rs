use crate::{
    StateStore, database, identity_vault_source,
    ssh_identities::{self, IdentityEdit},
    vault_crypto::VaultCipher,
    vault_database::{self, VaultEdit},
    vault_record::CredentialRecord,
    vault_wrap::ValidationWrappedDek,
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn private() -> std::io::Result<tempfile::TempDir> {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(d)
}
#[test]
fn one_wal_snapshot_never_mixes_reference_presence_with_newer_ciphertext() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let c = database::open(&path)?;
    let owner = Uuid::new_v4();
    let vault = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(vault.as_bytes());
    let empty = vault_database::load(&c, owner)?;
    let initialized = vault_database::edit(
        &c,
        owner,
        &empty,
        VaultEdit::Initialize(
            vault,
            ValidationWrappedDek::decode_ciphertext(&bytes, vault)?,
        ),
        || true,
    )?;
    let credential = Uuid::now_v7();
    let cipher = VaultCipher::generate()?;
    let record = CredentialRecord::seal(
        vault,
        credential,
        "ssh_password",
        &cipher,
        b"fixed-source-secret",
    )?;
    let saved = vault_database::edit(&c, owner, &initialized, VaultEdit::Create(record), || true)?;
    let identity = SshPasswordIdentity::new(
        "fixed",
        "user",
        SecretRef::new(vault, credential, SecretKind::SshPassword, false)?,
    )?;
    let empty = ssh_identities::load(&c, owner)?;
    ssh_identities::edit(&c, owner, &empty, IdentityEdit::Create(identity), || true)?;
    let tx = rusqlite::Transaction::new_unchecked(&c, rusqlite::TransactionBehavior::Deferred)?;
    let initial = ssh_identities::load(&tx, owner)?;
    let writer = database::open(&path)?;
    vault_database::edit(
        &writer,
        owner,
        &saved,
        VaultEdit::Remove(credential),
        || true,
    )?;
    let captured = identity_vault_source::load(&tx, owner)?;
    assert_eq!(captured.identities().revision(), initial.revision());
    assert!(
        captured
            .identities()
            .records()
            .next()
            .ok_or("fixture.identity")?
            .identity()
            .password()
            .has_value()
    );
    assert_eq!(captured.vault().revision(), saved.revision());
    drop(tx);
    let fresh = identity_vault_source::load(&c, owner)?;
    assert!(
        !fresh
            .identities()
            .records()
            .next()
            .ok_or("fixture.identity")?
            .identity()
            .password()
            .has_value()
    );
    assert!(fresh.vault().revision() > captured.vault().revision());
    assert!(identity_vault_source::validate(&c, owner, &captured, || true).is_err());
    assert!(identity_vault_source::validate(&c, Uuid::new_v4(), &fresh, || true).is_err());
    let blocked = std::sync::atomic::AtomicBool::new(false);
    identity_vault_source::validate(&c, owner, &fresh, || {
        if !c.is_autocommit()
            && let Ok(other) = rusqlite::Connection::open(&path)
        {
            let _ = other.busy_timeout(std::time::Duration::ZERO);
            if matches!(other.execute("UPDATE identities SET username='should-not-write'",[]),Err(rusqlite::Error::SqliteFailure(code,_))if code.code==rusqlite::ErrorCode::DatabaseBusy||code.code==rusqlite::ErrorCode::DatabaseLocked)
            {
                blocked.store(true, Ordering::SeqCst);
            }
        }
        true
    })?;
    assert!(blocked.load(Ordering::SeqCst));
    c.execute(
        "UPDATE identities SET username='changed-without-revision'",
        [],
    )?;
    assert!(identity_vault_source::validate(&c, owner, &fresh, || true).is_err());
    assert_eq!(
        fresh.identities().revision(),
        captured.identities().revision()
    );
    Ok(())
}
#[tokio::test]
async fn absent_or_corrupt_vault_never_becomes_filled_or_fallback_source() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    let source = store.load_identity_vault_source(Arc::new(|| true)).await?;
    assert!(source.vault().vault_id().is_none());
    assert_eq!(source.identities().records().count(), 0);
    assert!(
        store
            .load_identity_vault_source(Arc::new(|| false))
            .await
            .is_err()
    );
    let c = database::open(&path)?;
    c.execute(
        "INSERT INTO app_state(key,value)VALUES('vault.ciphertext.v1','broken')",
        [],
    )?;
    assert!(
        store
            .load_identity_vault_source(Arc::new(|| true))
            .await
            .is_err()
    );
    task.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn cancelled_active_source_waiter_keeps_native_predicate_until_exit() -> TestResult {
    let d = private()?;
    let (store, task) = StateStore::open(d.path().join("state.db")).await?;
    let (entered, received) = std::sync::mpsc::channel();
    let (resume, continued) = std::sync::mpsc::channel();
    let held = Arc::new(AtomicBool::new(true));
    struct Held(Arc<AtomicBool>);
    impl Drop for Held {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let resource = Held(held.clone());
    let continued = Mutex::new(continued);
    let first = AtomicBool::new(true);
    let gate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        let _keep = &resource;
        if first.swap(false, Ordering::SeqCst) {
            if entered.send(()).is_err() {
                return false;
            }
            return continued.lock().is_ok_and(|r| r.recv().is_ok());
        }
        true
    });
    let request = tokio::spawn(async move { store.load_identity_vault_source(gate).await });
    tokio::task::spawn_blocking(move || received.recv_timeout(std::time::Duration::from_secs(2)))
        .await??;
    request.abort();
    let result = request.await;
    assert!(matches!(result,Err(error)if error.is_cancelled()));
    assert!(held.load(Ordering::SeqCst));
    resume.send(())?;
    task.shutdown().await?;
    assert!(!held.load(Ordering::SeqCst));
    Ok(())
}
