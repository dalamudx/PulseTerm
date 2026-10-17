use super::*;
use crate::{
    database,
    ssh_hosts::{self, HostEdit},
    ssh_key_identities::KeyIdentityEdit,
    ssh_trust::{self, TrustEdit, TrustRecord, TrustTarget},
    vault_crypto::VaultCipher,
    vault_database::{self, VaultEdit},
    vault_record::CredentialRecord,
    vault_wrap::ValidationWrappedDek,
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_endpoint::SshEndpoint,
    ssh_key_identity::SshPrivateKeyIdentity,
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
fn initialize(
    c: &Connection,
    owner: Uuid,
) -> std::result::Result<Uuid, Box<dyn std::error::Error>> {
    // Envelope shape only: this source comparison does not authenticate or unlock it.
    let vault = Uuid::now_v7();
    let mut wire = vec![0; 110];
    wire[..6].copy_from_slice(b"PVDE\x01\x01");
    wire[6..22].copy_from_slice(vault.as_bytes());
    vault_database::edit(
        c,
        owner,
        &vault_database::load(c, owner)?,
        VaultEdit::Initialize(
            vault,
            ValidationWrappedDek::decode_ciphertext(&wire, vault)?,
        ),
        || true,
    )?;
    Ok(vault)
}
fn identity(
    vault: Uuid,
    key: Uuid,
) -> std::result::Result<SshPrivateKeyIdentity, Box<dyn std::error::Error>> {
    Ok(SshPrivateKeyIdentity::new(
        "fixed-key",
        "user",
        SecretRef::new(vault, key, SecretKind::SshPrivateKey, true)?,
        None,
    )?)
}
fn conflict(result: Result<()>) {
    assert!(matches!(result, Err(StoreError::IdentityConflict)));
}
#[test]
fn endpoint_vault_private_key_and_trust_changes_each_reject_original_source() -> TestResult {
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let empty = load(&c, owner)?;
    ssh_hosts::edit(
        &c,
        owner,
        empty.hosts(),
        HostEdit::Create(SshEndpoint::new("fixed-host", "fixed.example", 22, "user")?),
        || true,
    )?;
    conflict(validate(&c, owner, &empty, || true));
    let before_vault = load(&c, owner)?;
    let vault = initialize(&c, owner)?;
    conflict(validate(&c, owner, &before_vault, || true));
    let before_identity = load(&c, owner)?;
    let key = Uuid::now_v7();
    ssh_key_identities::edit(
        &c,
        owner,
        before_identity.identities(),
        KeyIdentityEdit::Create(identity(vault, key)?),
        || true,
    )?;
    conflict(validate(&c, owner, &before_identity, || true));
    let before_ciphertext = load(&c, owner)?;
    assert!(
        !before_ciphertext
            .identities()
            .records()
            .next()
            .ok_or("fixture.identity")?
            .identity()
            .private_key()
            .has_value()
    );
    let cipher = VaultCipher::generate()?;
    vault_database::edit(
        &c,
        owner,
        before_ciphertext.vault(),
        VaultEdit::Create(CredentialRecord::seal(
            vault,
            key,
            "ssh_private_key",
            &cipher,
            b"fixed-encrypted-key",
        )?),
        || true,
    )?;
    conflict(validate(&c, owner, &before_ciphertext, || true));
    let before_pin = load(&c, owner)?;
    assert!(
        before_pin
            .identities()
            .records()
            .next()
            .ok_or("fixture.identity")?
            .identity()
            .private_key()
            .has_value()
    );
    ssh_trust::edit(
        &c,
        owner,
        before_pin.trust(),
        TrustEdit::Pin(TrustRecord::new(
            TrustTarget::new("fixed.example", 22)?,
            "ssh-ed25519 AAECAwQF".to_owned(),
        )?),
    )?;
    conflict(validate(&c, owner, &before_pin, || true));
    validate(&c, owner, &load(&c, owner)?, || true)?;
    Ok(())
}
#[test]
fn same_revision_key_metadata_drift_cannot_upgrade_source() -> TestResult {
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let vault = initialize(&c, owner)?;
    let empty = load(&c, owner)?;
    ssh_key_identities::edit(
        &c,
        owner,
        empty.identities(),
        KeyIdentityEdit::Create(identity(vault, Uuid::now_v7())?),
        || true,
    )?;
    let before = load(&c, owner)?;
    c.execute("UPDATE key_identities SET label='external-metadata'", [])?;
    assert_eq!(
        load(&c, owner)?.identities().revision(),
        before.identities().revision()
    );
    conflict(validate(&c, owner, &before, || true));
    validate(&c, owner, &load(&c, owner)?, || true)?;
    Ok(())
}
#[test]
fn wal_view_keeps_key_directory_and_common_sources_together() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let c = database::open(&path)?;
    let other = database::open(&path)?;
    let owner = Uuid::new_v4();
    let vault = initialize(&c, owner)?;
    let initial = load(&c, owner)?;
    let tx = Transaction::new_unchecked(&c, TransactionBehavior::Deferred)?;
    let frozen = read(&tx, owner)?;
    ssh_key_identities::edit(
        &other,
        owner,
        initial.identities(),
        KeyIdentityEdit::Create(identity(vault, Uuid::now_v7())?),
        || true,
    )?;
    let old_view = read(&tx, owner)?;
    assert_eq!(old_view.identities().records().count(), 0);
    assert_eq!(old_view.vault().revision(), frozen.vault().revision());
    tx.rollback()?;
    assert_eq!(load(&c, owner)?.identities().records().count(), 1);
    conflict(validate(&c, owner, &frozen, || true));
    Ok(())
}
#[test]
fn foreign_owner_and_revoked_final_context_are_rejected() -> TestResult {
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let source = load(&c, owner)?;
    let calls = AtomicUsize::new(0);
    conflict(validate(&c, Uuid::new_v4(), &source, || {
        calls.fetch_add(1, Ordering::SeqCst);
        true
    }));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        validate(&c, owner, &source, || calls.fetch_add(1, Ordering::SeqCst)
            == 0),
        Err(StoreError::IdentityAuthorization)
    ));
    assert!(matches!(
        validate(&c, owner, &source, || false),
        Err(StoreError::IdentityAuthorization)
    ));
    Ok(())
}
#[tokio::test]
async fn public_worker_rejects_foreign_instance_without_calling_its_predicate() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (a, wa) = StateStore::open(path.clone()).await?;
    let (b, wb) = StateStore::open(path).await?;
    let source = a.load_ssh_key_target_source(Arc::new(|| true)).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    assert!(matches!(
        b.validate_ssh_key_target_source(
            source,
            Arc::new(move || {
                seen.fetch_add(1, Ordering::SeqCst);
                true
            })
        )
        .await,
        Err(StoreError::IdentityConflict)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        a.load_ssh_key_target_source(Arc::new(|| panic!("fixed-authorizer-panic")))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    drop(a);
    drop(b);
    wa.join().await?;
    wb.join().await?;
    Ok(())
}
#[tokio::test]
async fn canceled_native_read_and_validation_keep_credit_until_actual_exit() -> TestResult {
    for validating in [false, true] {
        let dir = private()?;
        let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
        let source = store.load_ssh_key_target_source(Arc::new(|| true)).await?;
        let credit = Arc::new(());
        let weak = Arc::downgrade(&credit);
        let calls = AtomicUsize::new(0);
        let (tx, entered) = oneshot::channel();
        let signal = Mutex::new(Some(tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release = Mutex::new(release_rx);
        let gate = Arc::new(move || {
            let _held = &credit;
            if calls.fetch_add(1, Ordering::SeqCst) == usize::from(validating) {
                if let Ok(mut signal) = signal.lock()
                    && let Some(tx) = signal.take()
                {
                    let _sent = tx.send(());
                }
                if let Ok(rx) = release.lock() {
                    let _released = rx.recv_timeout(std::time::Duration::from_secs(3));
                }
            }
            true
        });
        let client = store.clone();
        let pending = tokio::spawn(async move {
            if validating {
                client.validate_ssh_key_target_source(source, gate).await
            } else {
                client.load_ssh_key_target_source(gate).await.map(|_| ())
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered).await??;
        pending.abort();
        assert!(pending.await.is_err_and(|error| error.is_cancelled()));
        assert!(weak.upgrade().is_some());
        release_tx.send(())?;
        store.load_ssh_key_target_source(Arc::new(|| true)).await?;
        assert!(weak.upgrade().is_none());
        drop(store);
        worker.join().await?;
    }
    Ok(())
}
