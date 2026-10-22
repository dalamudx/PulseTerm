use super::*;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_endpoint::SshEndpoint,
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::{
    ssh_hosts::HostEdit, ssh_identities::IdentityEdit, vault_crypto::SecretBytes,
    vault_unlock::begin_validation_unlock,
};
#[test]
fn saved_source_native_guard_page_and_original_epoch_never_become_target_permission() -> TestResult
{
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
            PasswordInput::new(b"fixed-preconditions-master".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, vault, request.ready().await?)
            .await?;
        let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-preconditions-master".to_vec())?,
            saved.clone(),
        )?;
        operation
            .confirm_unlock(&store, saved, request.ready().await?)
            .await?
            .accept()?;
        let value = SshPasswordIdentity::new(
            "fixed",
            "user",
            SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
        )?;
        let meta = store
            .edit_ssh_identities(
                store.load_ssh_identities(Arc::new(|| true)).await?,
                IdentityEdit::Create(value),
                Arc::new(|| true),
            )
            .await?;
        let id = meta.records().next().ok_or("fixture.identity")?.id();
        let hosts = store
            .edit_ssh_hosts(
                store.load_ssh_hosts().await?,
                HostEdit::Create(SshEndpoint::new("fixed-host", "fixed.example", 22, "user")?),
                Arc::new(|| true),
            )
            .await?;
        let host = hosts.records()[0].id();
        let empty = store.load_ssh_target_source(Arc::new(|| true)).await?;
        assert!(
            owner
                .begin(p.scope.clone())?
                .prepare_saved_password_use(&store, empty, host, id)
                .await
                .is_err()
        );
        let source = store.load_identity_vault_source(Arc::new(|| true)).await?;
        let saved = owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(source, id)?
            .write(&store, SecretBytes::from_owned(b"fixed-not-used".to_vec())?)
            .await?;
        let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
        owner
            .begin(p.scope.clone())?
            .prepare_saved_password_use(&store, source.clone(), host, id)
            .await?
            .validate(&store)
            .await?;
        let live = Arc::new(AtomicBool::new(true));
        let check = live.clone();
        let prepared = owner
            .begin_authorized(
                p.scope.clone(),
                Arc::new(move || check.load(Ordering::Acquire)),
            )?
            .prepare_saved_password_use(&store, source.clone(), host, id)
            .await?;
        live.store(false, Ordering::Release);
        assert!(prepared.validate(&store).await.is_err());
        let prepared = owner
            .begin(p.scope.clone())?
            .prepare_saved_password_use(&store, source.clone(), host, id)
            .await?;
        let scope = owner.inner.guard()?.scope.clone().ok_or("fixture.scope")?;
        let epoch = scope.available_epoch()?;
        scope.lock()?;
        let request = begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed-preconditions-master".to_vec())?,
            saved.envelope().ok_or("fixture.envelope")?.clone(),
        )?;
        let mut ready = request.ready().await?;
        ready.confirm(&scope)?;
        drop(ready);
        assert!(!epoch.is_current_for(&scope)?);
        assert!(prepared.validate(&store).await.is_err());
        let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
        let prepared = owner
            .begin(p.scope.clone())?
            .prepare_saved_password_use(&store, source.clone(), host, id)
            .await?;
        let _changed = store
            .edit_ssh_hosts(
                source.hosts().clone(),
                HostEdit::Update(
                    host,
                    SshEndpoint::new("changed-host", "fixed.example", 22, "user")?,
                ),
                Arc::new(|| true),
            )
            .await?;
        assert!(prepared.validate(&store).await.is_err());
        let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
        let prepared = owner
            .begin(p.scope.clone())?
            .prepare_saved_password_use(&store, source, host, id)
            .await?;
        p.documents.started(p.window)?;
        owner.retire_page()?;
        assert!(prepared.validate(&store).await.is_err());
        owner.close()?;
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
