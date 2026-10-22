use super::*;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::{
    ssh_identities::IdentityEdit, vault_crypto::SecretBytes, vault_unlock::begin_validation_unlock,
};
#[test]
fn single_use_intent_never_recaptures_later_authenticated_same_dek_epoch_or_upgrades_retired_page()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let p = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let root = crate::test_tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let operation = owner.begin(p.scope.clone())?;
        let (vault, request) = operation.begin_creation(
            PasswordInput::new(b"fixed-pinned-intent-master".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, vault, request.ready().await?)
            .await?;
        let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-pinned-intent-master".to_vec())?,
            saved.clone(),
        )?;
        operation
            .confirm_unlock(&store, saved, request.ready().await?)
            .await?
            .accept()?;
        let credential = Uuid::now_v7();
        let value = SshPasswordIdentity::new(
            "fixed",
            "fixed-user",
            SecretRef::new(vault, credential, SecretKind::SshPassword, false)?,
        )?;
        let empty = store.load_ssh_identities(Arc::new(|| true)).await?;
        let identity = store
            .edit_ssh_identities(empty, IdentityEdit::Create(value), Arc::new(|| true))
            .await?;
        let id = identity.records().next().ok_or("fixture.identity")?.id();
        let source = store.load_identity_vault_source(Arc::new(|| true)).await?;
        let intent = owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(source, id)?;
        let saved = intent
            .write(
                &store,
                SecretBytes::from_owned(b"fixed-pinned-password".to_vec())?,
            )
            .await?;
        let scope = owner.inner.guard()?.scope.clone().ok_or("fixture.scope")?;
        let original = scope.available_epoch()?;
        let source = store.load_identity_vault_source(Arc::new(|| true)).await?;
        let intent = owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(source, id)?;
        scope.lock()?;
        let request = begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed-pinned-intent-master".to_vec())?,
            saved.envelope().ok_or("fixture.envelope")?.clone(),
        )?;
        let mut ready = request.ready().await?;
        ready.confirm(&scope)?;
        drop(ready);
        assert_eq!(scope.status()?, KeyStatus::Available);
        assert!(!original.is_current_for(&scope)?);
        assert!((intent.operation.authorization())());
        let current = scope.available_epoch()?;
        assert_eq!(
            saved
                .credentials()
                .first()
                .ok_or("fixture.record")?
                .open_for_adapter_in_epoch(&scope, &current, credential, "ssh_password")?
                .expose(),
            b"fixed-pinned-password"
        );
        assert!(
            intent
                .write(
                    &store,
                    SecretBytes::from_owned(b"fixed-not-upgraded".to_vec())?
                )
                .await
                .is_err()
        );
        let now = store.load_identity_vault_source(Arc::new(|| true)).await?;
        assert_eq!(now.vault().revision(), saved.revision());
        assert_eq!(scope.status()?, KeyStatus::Available);
        let intent = owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(now, id)?;
        p.documents.started(p.window)?;
        owner.retire_page()?;
        assert!(
            intent
                .write(
                    &store,
                    SecretBytes::from_owned(b"fixed-not-retired".to_vec())?
                )
                .await
                .is_err()
        );
        assert_eq!(
            store
                .load_vault_ciphertext(Arc::new(|| true))
                .await?
                .revision(),
            saved.revision()
        );
        owner.close()?;
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
