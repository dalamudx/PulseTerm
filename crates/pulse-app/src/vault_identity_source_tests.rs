use super::*;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::{
    ssh_identities::IdentityEdit, vault_crypto::VaultCipher, vault_database::VaultEdit,
    vault_record::CredentialRecord,
};
#[test]
fn authenticated_epoch_saved_filled_identity_source_and_retirement_are_independent_of_target_permission()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let p = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let root = tempfile::tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let path = dir.join("state.db");
        let (store, worker) = StateStore::open(path.clone()).await?;
        let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let operation = owner.begin(p.scope.clone())?;
        let (vault, creation) = operation.begin_creation(
            PasswordInput::new(b"fixed-identity-source-master".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, vault, creation.ready().await?)
            .await?;
        let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-identity-source-master".to_vec())?,
            saved.clone(),
        )?;
        operation
            .confirm_unlock(&store, saved.clone(), request.ready().await?)
            .await?
            .accept()?;
        let credential = Uuid::now_v7();
        let identity = SshPasswordIdentity::new(
            "fixed",
            "fixed-user",
            SecretRef::new(vault, credential, SecretKind::SshPassword, true)?,
        )?;
        let empty = store.load_ssh_identities(Arc::new(|| true)).await?;
        let catalog = store
            .edit_ssh_identities(empty, IdentityEdit::Create(identity), Arc::new(|| true))
            .await?;
        let id = catalog.records().next().ok_or("fixture.identity")?.id();
        let operation = owner.begin(p.scope.clone())?;
        let unfilled = store.load_identity_vault_source(Arc::new(|| true)).await?;
        assert!(
            operation
                .validate_password_identity_source(&store, unfilled, id)
                .await
                .is_err()
        );
        // Structural ciphertext is intentionally encrypted under a different test key: preparation
        // must never be mistaken for successful credential authentication or target-use permission.
        let record = CredentialRecord::seal(
            vault,
            credential,
            "ssh_password",
            &VaultCipher::generate()?,
            b"fixed-structural-only-secret",
        )?;
        store
            .edit_vault_ciphertext(saved, VaultEdit::Create(record), Arc::new(|| true))
            .await?;
        let filled = store.load_identity_vault_source(Arc::new(|| true)).await?;
        operation
            .validate_password_identity_source(&store, filled.clone(), id)
            .await?;
        assert!(
            operation
                .validate_password_identity_source(&store, filled.clone(), Uuid::now_v7())
                .await
                .is_err()
        );
        let (other, other_worker) = StateStore::open(path.clone()).await?;
        assert!(
            operation
                .validate_password_identity_source(&other, filled.clone(), id)
                .await
                .is_err()
        );
        drop(other);
        other_worker.join().await?;
        owner.lock()?;
        assert!(
            operation
                .validate_password_identity_source(&store, filled, id)
                .await
                .is_err()
        );
        drop(operation);
        let current = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let operation = owner.begin(p.scope.clone())?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-identity-source-master".to_vec())?,
            current.clone(),
        )?;
        operation
            .confirm_unlock(&store, current.clone(), request.ready().await?)
            .await?
            .accept()?;
        let operation = owner.begin(p.scope.clone())?;
        let old = store.load_identity_vault_source(Arc::new(|| true)).await?;
        store
            .edit_vault_ciphertext(current, VaultEdit::Remove(credential), Arc::new(|| true))
            .await?;
        assert!(
            operation
                .validate_password_identity_source(&store, old, id)
                .await
                .is_err()
        );
        let now = store.load_identity_vault_source(Arc::new(|| true)).await?;
        assert!(
            operation
                .validate_password_identity_source(&store, now.clone(), id)
                .await
                .is_err()
        );
        p.documents.started(p.window)?;
        owner.retire_page()?;
        assert!(
            operation
                .validate_password_identity_source(&store, now, id)
                .await
                .is_err()
        );
        drop(operation);
        owner.close()?;
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
