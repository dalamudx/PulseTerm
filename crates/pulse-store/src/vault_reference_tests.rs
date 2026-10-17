use crate::{
    StateStore, StoreError,
    vault_crypto::{SealedSecret, SecretContext, VaultCipher},
    vault_database::VaultEdit,
    vault_lifecycle::{KeyStatus, KeyUseError, LifecycleError, VaultKeyLifecycle},
    vault_record::CredentialRecord,
    vault_wrap::ValidationWrappedDek,
};
use pulse_core::secret_ref::SecretKind;
use std::sync::Arc;
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn envelope(id: Uuid) -> Result<ValidationWrappedDek, Box<dyn std::error::Error>> {
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    Ok(ValidationWrappedDek::decode_ciphertext(&bytes, id)?)
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
#[tokio::test]
async fn references_derive_presence_role_and_vault_without_enabling_or_resurrecting_key()
-> TestResult {
    let d = private()?;
    let (store, worker) = StateStore::open(d.path().join("state.db")).await?;
    let id = Uuid::now_v7();
    let secret_id = Uuid::now_v7();
    let absent = store.load_vault_ciphertext(allowed()).await?;
    assert!(
        absent
            .secret_ref(secret_id, SecretKind::SshPassword)?
            .is_none()
    );
    assert!(matches!(
        absent.secret_ref(Uuid::nil(), SecretKind::SshPassword),
        Err(StoreError::VaultRecord)
    ));
    let empty = store
        .edit_vault_ciphertext(absent, VaultEdit::Initialize(id, envelope(id)?), allowed())
        .await?;
    let missing = empty
        .secret_ref(secret_id, SecretKind::SshPassword)?
        .ok_or("fixture.reference")?;
    assert!(!missing.has_value());
    assert_eq!(missing.vault_id(), id);
    assert_eq!(missing.id(), secret_id);
    let key = VaultCipher::generate()?;
    let record = CredentialRecord::seal(id, secret_id, "ssh_password", &key, b"fixed-ref-fixture")?;
    let saved = store
        .edit_vault_ciphertext(empty.clone(), VaultEdit::Create(record), allowed())
        .await?;
    let present = saved
        .secret_ref(secret_id, SecretKind::SshPassword)?
        .ok_or("fixture.reference")?;
    assert!(present.has_value());
    assert_eq!(present.kind(), SecretKind::SshPassword);
    assert!(matches!(
        saved.secret_ref(secret_id, SecretKind::SshKeyPassphrase),
        Err(StoreError::VaultConflict)
    ));
    assert!(
        !empty
            .secret_ref(secret_id, SecretKind::SshPassword)?
            .ok_or("fixture.reference")?
            .has_value()
    );
    let scope = VaultKeyLifecycle::new(id)?;
    assert_eq!(scope.status()?, KeyStatus::Locked);
    let (nonce, ciphertext) = saved.credentials()[0].storage_parts();
    let sealed = SealedSecret {
        nonce: *nonce,
        ciphertext: ciphertext.to_vec(),
    };
    assert!(matches!(
        scope.open_for_adapter(
            &SecretContext::bound_to_vault(id, secret_id, present.kind().as_str())?,
            &sealed
        ),
        Err(KeyUseError::Lifecycle(LifecycleError::Locked))
    ));
    let removed = store
        .edit_vault_ciphertext(saved.clone(), VaultEdit::Remove(secret_id), allowed())
        .await?;
    assert!(
        !removed
            .secret_ref(secret_id, SecretKind::SshPassword)?
            .ok_or("fixture.reference")?
            .has_value()
    );
    assert!(
        saved
            .secret_ref(secret_id, SecretKind::SshPassword)?
            .ok_or("fixture.reference")?
            .has_value()
    );
    assert!(matches!(
        store
            .edit_vault_ciphertext(
                removed,
                VaultEdit::Create(CredentialRecord::seal(
                    id,
                    secret_id,
                    "ssh_password",
                    &key,
                    b"cannot-recreate"
                )?),
                allowed()
            )
            .await,
        Err(StoreError::VaultConflict)
    ));
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn identical_credential_uuid_in_foreign_vault_never_aliases_reference() -> TestResult {
    let d = private()?;
    let secret_id = Uuid::now_v7();
    let mut references = Vec::new();
    for name in ["a.db", "b.db"] {
        let (store, worker) = StateStore::open(d.path().join(name)).await?;
        let id = Uuid::now_v7();
        let absent = store.load_vault_ciphertext(allowed()).await?;
        let empty = store
            .edit_vault_ciphertext(absent, VaultEdit::Initialize(id, envelope(id)?), allowed())
            .await?;
        let key = VaultCipher::generate()?;
        let saved = store
            .edit_vault_ciphertext(
                empty,
                VaultEdit::Create(CredentialRecord::seal(
                    id,
                    secret_id,
                    "ssh_password",
                    &key,
                    b"fixture",
                )?),
                allowed(),
            )
            .await?;
        references.push(
            saved
                .secret_ref(secret_id, SecretKind::SshPassword)?
                .ok_or("fixture.reference")?,
        );
        drop(store);
        worker.join().await?;
    }
    assert_eq!(references[0].id(), references[1].id());
    assert!(references[0].vault_id() != references[1].vault_id());
    assert!(references[0] != references[1]);
    Ok(())
}

#[tokio::test]
async fn private_key_and_passphrase_references_reopen_with_distinct_authenticated_roles()
-> TestResult {
    use pulse_core::{ssh_identity::SshPasswordIdentity, ssh_key_identity::SshPrivateKeyIdentity};
    let d = private()?;
    let path = d.path().join("key-role.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let vault = Uuid::now_v7();
    let key_id = Uuid::now_v7();
    let pass_id = Uuid::now_v7();
    let absent = store.load_vault_ciphertext(allowed()).await?;
    assert!(
        absent
            .secret_ref(key_id, SecretKind::SshPrivateKey)?
            .is_none()
    );
    let empty = store
        .edit_vault_ciphertext(
            absent,
            VaultEdit::Initialize(vault, envelope(vault)?),
            allowed(),
        )
        .await?;
    let cipher = VaultCipher::generate()?;
    let key_record = CredentialRecord::seal(
        vault,
        key_id,
        SecretKind::SshPrivateKey.as_str(),
        &cipher,
        b"fixed-ciphertext-role-fixture-not-a-parsed-key",
    )?;
    let key_saved = store
        .edit_vault_ciphertext(empty, VaultEdit::Create(key_record), allowed())
        .await?;
    let saved = store
        .edit_vault_ciphertext(
            key_saved,
            VaultEdit::Create(CredentialRecord::seal(
                vault,
                pass_id,
                SecretKind::SshKeyPassphrase.as_str(),
                &cipher,
                b"fixed-key-passphrase-fixture",
            )?),
            allowed(),
        )
        .await?;
    let key_reference = saved
        .secret_ref(key_id, SecretKind::SshPrivateKey)?
        .ok_or("fixture.key_reference")?;
    let pass_reference = saved
        .secret_ref(pass_id, SecretKind::SshKeyPassphrase)?
        .ok_or("fixture.passphrase_reference")?;
    assert!(key_reference.has_value() && pass_reference.has_value());
    SshPrivateKeyIdentity::new("key", "user", key_reference, Some(pass_reference))?;
    assert!(SshPasswordIdentity::new("key", "user", key_reference).is_err());
    assert!(matches!(
        saved.secret_ref(key_id, SecretKind::SshPassword),
        Err(StoreError::VaultConflict)
    ));
    assert!(matches!(
        saved.secret_ref(pass_id, SecretKind::SshPrivateKey),
        Err(StoreError::VaultConflict)
    ));
    let record = saved
        .credentials()
        .iter()
        .find(|record| record.id() == key_id)
        .ok_or("fixture.key_record")?;
    let (nonce, encrypted) = record.storage_parts();
    let sealed = SealedSecret {
        nonce: *nonce,
        ciphertext: encrypted.to_vec(),
    };
    assert!(
        cipher
            .open(
                &SecretContext::bound_to_vault(vault, key_id, SecretKind::SshPrivateKey.as_str())?,
                &sealed
            )
            .is_ok()
    );
    for (other_vault, other_id, other_kind) in [
        (vault, key_id, SecretKind::SshPassword),
        (vault, key_id, SecretKind::SshKeyPassphrase),
        (vault, pass_id, SecretKind::SshPrivateKey),
        (Uuid::now_v7(), key_id, SecretKind::SshPrivateKey),
    ] {
        assert!(
            cipher
                .open(
                    &SecretContext::bound_to_vault(other_vault, other_id, other_kind.as_str())?,
                    &sealed
                )
                .is_err()
        );
    }
    drop(store);
    worker.join().await?;
    let (store, worker) = StateStore::open(path).await?;
    let reopened = store.load_vault_ciphertext(allowed()).await?;
    assert!(
        reopened
            .secret_ref(key_id, SecretKind::SshPrivateKey)?
            .ok_or("fixture.reopen_key")?
            == key_reference
    );
    assert!(
        reopened
            .secret_ref(pass_id, SecretKind::SshKeyPassphrase)?
            .ok_or("fixture.reopen_passphrase")?
            == pass_reference
    );
    let removed = store
        .edit_vault_ciphertext(reopened.clone(), VaultEdit::Remove(key_id), allowed())
        .await?;
    assert!(
        !removed
            .secret_ref(key_id, SecretKind::SshPrivateKey)?
            .ok_or("fixture.deleted_key")?
            .has_value()
    );
    assert!(
        removed
            .secret_ref(pass_id, SecretKind::SshKeyPassphrase)?
            .ok_or("fixture.kept_passphrase")?
            .has_value()
    );
    assert!(matches!(
        store
            .edit_vault_ciphertext(reopened, VaultEdit::Remove(pass_id), allowed())
            .await,
        Err(StoreError::VaultConflict)
    ));
    assert!(matches!(
        store
            .edit_vault_ciphertext(
                removed,
                VaultEdit::Create(CredentialRecord::seal(
                    vault,
                    key_id,
                    SecretKind::SshPrivateKey.as_str(),
                    &cipher,
                    b"cannot-resurrect-key"
                )?),
                allowed()
            )
            .await,
        Err(StoreError::VaultConflict)
    ));
    drop(store);
    worker.join().await?;
    Ok(())
}
