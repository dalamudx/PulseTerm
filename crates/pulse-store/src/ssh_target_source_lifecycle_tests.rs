use super::*;
use crate::{
    database,
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
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn private() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
#[test]
fn absent_to_present_and_same_identity_revision_ciphertext_changes_reject_old_capture() -> TestResult
{
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let empty = load(&c, owner)?;
    let vault = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(vault.as_bytes());
    let initialized = vault_database::edit(
        &c,
        owner,
        empty.identity_vault.vault(),
        VaultEdit::Initialize(
            vault,
            ValidationWrappedDek::decode_ciphertext(&bytes, vault)?,
        ),
        || true,
    )?;
    assert!(matches!(
        validate(&c, owner, &empty, || true),
        Err(StoreError::IdentityConflict)
    ));
    let credential = Uuid::now_v7();
    let identity = SshPasswordIdentity::new(
        "fixed",
        "user",
        SecretRef::new(vault, credential, SecretKind::SshPassword, false)?,
    )?;
    let meta = ssh_identities::edit(
        &c,
        owner,
        &ssh_identities::load(&c, owner)?,
        IdentityEdit::Create(identity),
        || true,
    )?;
    let before = load(&c, owner)?;
    assert!(
        !meta
            .records()
            .next()
            .ok_or("fixture.identity_missing")?
            .identity()
            .password()
            .has_value()
    );
    let cipher = VaultCipher::generate()?;
    let record = CredentialRecord::seal(
        vault,
        credential,
        "ssh_password",
        &cipher,
        b"fixed-source-only",
    )?;
    let _written =
        vault_database::edit(&c, owner, &initialized, VaultEdit::Create(record), || true)?;
    let after = load(&c, owner)?;
    assert_eq!(
        before.identity_vault.identities().revision(),
        after.identity_vault.identities().revision()
    );
    assert!(
        after
            .identity_vault
            .identities()
            .records()
            .next()
            .ok_or("fixture.identity_missing")?
            .identity()
            .password()
            .has_value()
    );
    assert!(matches!(
        validate(&c, owner, &before, || true),
        Err(StoreError::IdentityConflict)
    ));
    validate(&c, owner, &after, || true)?;
    Ok(())
}
#[tokio::test]
async fn active_validation_cancel_retains_native_predicate_credit_until_actual_exit() -> TestResult
{
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
    let credit = Arc::new(());
    let weak = Arc::downgrade(&credit);
    let calls = AtomicUsize::new(0);
    let (tx, entered) = oneshot::channel();
    let entered_tx = Mutex::new(Some(tx));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release = Mutex::new(release_rx);
    let gate = Arc::new(move || {
        let _held = &credit;
        if calls.fetch_add(1, Ordering::SeqCst) == 1 {
            if let Ok(mut tx) = entered_tx.lock()
                && let Some(tx) = tx.take()
            {
                let _sent = tx.send(());
            }
            if let Ok(rx) = release.lock() {
                let _released = rx.recv();
            }
        }
        true
    });
    let pending_store = store.clone();
    let pending =
        tokio::spawn(async move { pending_store.validate_ssh_target_source(source, gate).await });
    entered.await?;
    pending.abort();
    assert!(pending.await.is_err_and(|error| error.is_cancelled()));
    assert!(weak.upgrade().is_some());
    let _released = release_tx.send(());
    let _fresh = store.load_ssh_target_source(Arc::new(|| true)).await?;
    assert!(weak.upgrade().is_none());
    drop(store);
    worker.join().await?;
    Ok(())
}
