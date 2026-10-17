use super::*;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::{
    ssh_identities::IdentityEdit, vault_database::VaultEdit, vault_record::CredentialRecord,
};
#[test]
fn authenticated_scoped_password_edit_exact_identity_source_and_epoch_cannot_upgrade_or_resurrect()
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
        let (store, worker) = StateStore::open(path).await?;
        let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let operation = owner.begin(p.scope.clone())?;
        let (vault, creation) = operation.begin_creation(
            PasswordInput::new(b"fixed-identity-write-master".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, vault, creation.ready().await?)
            .await?;
        let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-identity-write-master".to_vec())?,
            saved.clone(),
        )?;
        operation
            .confirm_unlock(&store, saved, request.ready().await?)
            .await?
            .accept()?;
        let secret = Uuid::now_v7();
        let value = SshPasswordIdentity::new(
            "fixed",
            "fixed-user",
            SecretRef::new(vault, secret, SecretKind::SshPassword, false)?,
        )?;
        let empty = store.load_ssh_identities(Arc::new(|| true)).await?;
        let catalog = store
            .edit_ssh_identities(empty, IdentityEdit::Create(value), Arc::new(|| true))
            .await?;
        let id = catalog.records().next().ok_or("fixture.identity")?.id();
        let scope = owner.inner.guard()?.scope.clone().ok_or("fixture.scope")?;
        let epoch = scope.available_epoch()?;
        let operation = owner.begin(p.scope.clone())?;
        let source = store.load_identity_vault_source(Arc::new(|| true)).await?;
        let record = CredentialRecord::seal_in_epoch(
            &scope,
            &epoch,
            secret,
            "ssh_password",
            b"fixed-written-password",
        )?;
        assert!(
            store
                .edit_identity_password_ciphertext(
                    source.clone(),
                    Uuid::now_v7(),
                    VaultEdit::Create(record.clone()),
                    scope.clone(),
                    epoch.clone(),
                    operation.authorization()
                )
                .await
                .is_err()
        );
        let written = operation
            .write_password_identity_source(
                &store,
                source.clone(),
                id,
                pulse_store::vault_crypto::SecretBytes::from_owned(
                    b"fixed-written-password".to_vec(),
                )?,
            )
            .await?;
        assert_eq!(
            written
                .credentials()
                .first()
                .ok_or("fixture.persisted")?
                .open_for_adapter_in_epoch(&scope, &epoch, secret, "ssh_password")?
                .expose(),
            b"fixed-written-password"
        );
        assert!(
            store
                .edit_identity_password_ciphertext(
                    source,
                    id,
                    VaultEdit::Replace(record.clone()),
                    scope.clone(),
                    epoch.clone(),
                    operation.authorization()
                )
                .await
                .is_err()
        );
        let fresh = store.load_identity_vault_source(Arc::new(|| true)).await?;
        let wrong = CredentialRecord::seal_in_epoch(
            &scope,
            &epoch,
            Uuid::now_v7(),
            "ssh_password",
            b"fixed-wrong-id",
        )?;
        assert!(
            store
                .edit_identity_password_ciphertext(
                    fresh.clone(),
                    id,
                    VaultEdit::Replace(wrong),
                    scope.clone(),
                    epoch.clone(),
                    operation.authorization()
                )
                .await
                .is_err()
        );
        let wrong = CredentialRecord::seal_in_epoch(
            &scope,
            &epoch,
            secret,
            "private_key",
            b"fixed-wrong-role",
        )?;
        assert!(
            store
                .edit_identity_password_ciphertext(
                    fresh.clone(),
                    id,
                    VaultEdit::Replace(wrong),
                    scope.clone(),
                    epoch.clone(),
                    operation.authorization()
                )
                .await
                .is_err()
        );
        // Cancel a real queued/native write while its first predicate owns the operation slot.
        let (entered, received) = std::sync::mpsc::channel();
        let (resume, continued) = std::sync::mpsc::channel();
        let (exited, exit) = tokio::sync::oneshot::channel();
        struct Exit(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for Exit {
            fn drop(&mut self) {
                if let Some(s) = self.0.take() {
                    let _ = s.send(());
                }
            }
        }
        let exit_guard = Exit(Some(exited));
        let first = std::sync::atomic::AtomicBool::new(true);
        let continued = std::sync::Mutex::new(continued);
        let current = operation.authorization();
        let gate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            let _held = &exit_guard;
            if first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                if entered.send(()).is_err() {
                    return false;
                }
                if !continued.lock().is_ok_and(|r| r.recv().is_ok()) {
                    return false;
                }
            }
            current()
        });
        let running_store = store.clone();
        let running_source = fresh.clone();
        let running_scope = scope.clone();
        let running_epoch = epoch.clone();
        let replacement = record.clone();
        let request = tokio::spawn(async move {
            running_store
                .edit_identity_password_ciphertext(
                    running_source,
                    id,
                    VaultEdit::Replace(replacement),
                    running_scope,
                    running_epoch,
                    gate,
                )
                .await
        });
        tokio::task::spawn_blocking(move || {
            received.recv_timeout(std::time::Duration::from_secs(2))
        })
        .await??;
        request.abort();
        assert!(matches!(request.await,Err(error)if error.is_cancelled()));
        drop(operation);
        assert!(matches!(
            owner.begin(p.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        resume.send(())?;
        tokio::time::timeout(std::time::Duration::from_secs(2), exit).await??;
        let after = store.load_identity_vault_source(Arc::new(|| true)).await?;
        assert_eq!(after.vault().revision(), written.revision());
        let (entered, received) = std::sync::mpsc::channel();
        let (resume, continued) = std::sync::mpsc::channel();
        let (exited, exit) = tokio::sync::oneshot::channel();
        let exit_guard = Exit(Some(exited));
        let first = std::sync::atomic::AtomicBool::new(true);
        let continued = std::sync::Mutex::new(continued);
        let main_thread = std::thread::current().id();
        let native_gate = Arc::new(move || {
            let _held = &exit_guard;
            if std::thread::current().id() != main_thread
                && first.swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                if entered.send(()).is_err() {
                    return false;
                }
                if !continued.lock().is_ok_and(|r| r.recv().is_ok()) {
                    return false;
                }
            }
            true
        });
        let operation = owner.begin_authorized(p.scope.clone(), native_gate)?;
        let mut future = Box::pin(operation.write_password_identity_source(
            &store,
            after.clone(),
            id,
            pulse_store::vault_crypto::SecretBytes::from_owned(
                b"fixed-cancelled-actor-write".to_vec(),
            )?,
        ));
        let pending = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(future.as_mut(), cx).is_pending())
        })
        .await;
        assert!(pending);
        tokio::task::spawn_blocking(move || {
            received.recv_timeout(std::time::Duration::from_secs(2))
        })
        .await??;
        drop(future);
        assert!(!(operation.authorization())());
        assert_eq!(scope.status()?, KeyStatus::Available);
        drop(operation);
        assert!(matches!(
            owner.begin(p.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        resume.send(())?;
        tokio::time::timeout(std::time::Duration::from_secs(2), exit).await??;
        let cancelled = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        assert_eq!(cancelled.revision(), written.revision());

        // A failed caller predicate cannot commit even if epoch/source remain current.
        let operation = owner.begin(p.scope.clone())?;
        assert!(
            store
                .edit_identity_password_ciphertext(
                    after.clone(),
                    id,
                    VaultEdit::Replace(record.clone()),
                    scope.clone(),
                    epoch.clone(),
                    Arc::new(|| false)
                )
                .await
                .is_err()
        );
        owner.lock()?;
        assert!(
            store
                .edit_identity_password_ciphertext(
                    after,
                    id,
                    VaultEdit::Replace(record),
                    scope.clone(),
                    epoch,
                    Arc::new(|| true)
                )
                .await
                .is_err()
        );
        drop(operation);
        let current = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let operation = owner.begin(p.scope.clone())?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-identity-write-master".to_vec())?,
            current.clone(),
        )?;
        operation
            .confirm_unlock(&store, current, request.ready().await?)
            .await?
            .accept()?;
        let epoch = scope.available_epoch()?;
        let operation = owner.begin(p.scope.clone())?;
        let old = store.load_identity_vault_source(Arc::new(|| true)).await?;
        store
            .edit_ssh_identities(
                old.identities().clone(),
                IdentityEdit::Remove(id),
                Arc::new(|| true),
            )
            .await?;
        let replacement = CredentialRecord::seal_in_epoch(
            &scope,
            &epoch,
            secret,
            "ssh_password",
            b"fixed-not-resurrected",
        )?;
        assert!(
            store
                .edit_identity_password_ciphertext(
                    old,
                    id,
                    VaultEdit::Replace(replacement.clone()),
                    scope.clone(),
                    epoch.clone(),
                    operation.authorization()
                )
                .await
                .is_err()
        );
        let deleted = store.load_identity_vault_source(Arc::new(|| true)).await?;
        assert!(
            store
                .edit_identity_password_ciphertext(
                    deleted,
                    id,
                    VaultEdit::Replace(replacement),
                    scope,
                    epoch,
                    operation.authorization()
                )
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
