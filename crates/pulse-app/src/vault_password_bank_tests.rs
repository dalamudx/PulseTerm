use super::*;
use crate::vault_password_handoff::PasswordWriteBank;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::{ssh_identities::IdentityEdit, vault_crypto::SecretBytes};
struct Exit(Option<tokio::sync::oneshot::Sender<()>>);
impl Drop for Exit {
    fn drop(&mut self) {
        if let Some(s) = self.0.take() {
            let _ = s.send(());
        }
    }
}
#[test]
fn native_bank_exact_owner_page_single_use_active_cancel_and_timer_keep_actual_root_credit()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let p = page()?;
        let foreign = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let bank = PasswordWriteBank::new(&owner);
        let other_owner = PageVaultOwner::new(p.registry.clone());
        let other = PasswordWriteBank::new(&other_owner);
        let root = tempfile::tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let operation = owner.begin(p.scope.clone())?;
        let (vault, request) = operation.begin_creation(
            PasswordInput::new(b"fixed-password-bank-master".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, vault, request.ready().await?)
            .await?;
        let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-password-bank-master".to_vec())?,
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
        let catalog = store
            .edit_ssh_identities(empty, IdentityEdit::Create(value), Arc::new(|| true))
            .await?;
        let id = catalog.records().next().ok_or("fixture.identity")?.id();
        let source = store.load_identity_vault_source(Arc::new(|| true)).await?;
        let intent = owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(source.clone(), id)?;
        assert!(other.publish(p.scope.clone(), intent).is_err());
        let intent = owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(source.clone(), id)?;
        let ticket = bank.publish(p.scope.clone(), intent)?;
        assert!(bank.take(&foreign.scope, ticket).is_err());
        assert!(bank.cancel(&p.scope, Uuid::now_v7()).is_err());
        assert!(bank.take(&p.scope, Uuid::now_v7()).is_err());
        let permit = bank.take(&p.scope, ticket)?;
        assert!(bank.take(&p.scope, ticket).is_err());
        bank.cancel(&p.scope, ticket)?;
        assert!(
            permit
                .write(
                    &store,
                    SecretBytes::from_owned(b"fixed-cancelled-ready".to_vec())?
                )
                .await
                .is_err()
        );
        let intent = owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(source, id)?;
        let ticket = bank.publish(p.scope.clone(), intent)?;
        let written = bank
            .take(&p.scope, ticket)?
            .write(
                &store,
                SecretBytes::from_owned(b"fixed-written-through-bank".to_vec())?,
            )
            .await?;
        let scope = owner.inner.guard()?.scope.clone().ok_or("fixture.scope")?;
        let epoch = scope.available_epoch()?;
        assert_eq!(
            written
                .credentials()
                .first()
                .ok_or("fixture.record")?
                .open_for_adapter_in_epoch(&scope, &epoch, credential, "ssh_password")?
                .expose(),
            b"fixed-written-through-bank"
        );
        assert!(bank.take(&p.scope, ticket).is_err());
        let source = store.load_identity_vault_source(Arc::new(|| true)).await?;
        let (entered, received) = std::sync::mpsc::channel();
        let (resume, continued) = std::sync::mpsc::channel();
        let (exited, exit) = tokio::sync::oneshot::channel();
        let guard = Exit(Some(exited));
        let first = std::sync::atomic::AtomicBool::new(true);
        let continued = std::sync::Mutex::new(continued);
        let main = std::thread::current().id();
        let gate = Arc::new(move || {
            let _hold = &guard;
            if std::thread::current().id() != main
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
        let intent = owner
            .begin_authorized(p.scope.clone(), gate)?
            .prepare_password_identity_write(source.clone(), id)?;
        let ticket = bank.publish(p.scope.clone(), intent)?;
        let permit = bank.take(&p.scope, ticket)?;
        let running_store = store.clone();
        let request = tokio::spawn(async move {
            permit
                .write(
                    &running_store,
                    SecretBytes::from_owned(b"fixed-cancelled-active".to_vec())
                        .map_err(|_| "fixture.input")?,
                )
                .await
                .map_err(|_| "fixture.cancelled")
        });
        tokio::task::spawn_blocking(move || {
            received.recv_timeout(std::time::Duration::from_secs(2))
        })
        .await??;
        bank.cancel(&p.scope, ticket)?;
        request.abort();
        assert!(matches!(request.await,Err(error)if error.is_cancelled()));
        assert!(matches!(
            owner.begin(p.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        resume.send(())?;
        tokio::time::timeout(std::time::Duration::from_secs(2), exit).await??;
        assert_eq!(
            store
                .load_vault_ciphertext(Arc::new(|| true))
                .await?
                .revision(),
            written.revision()
        );
        assert_eq!(scope.status()?, KeyStatus::Available);
        // Observe actual timer-driven operation destruction, with no frontend take/cancel/read to expire it.
        let (exited, exit) = tokio::sync::oneshot::channel();
        let guard = Exit(Some(exited));
        let gate = Arc::new(move || {
            let _hold = &guard;
            true
        });
        let intent = owner
            .begin_authorized(p.scope.clone(), gate)?
            .prepare_password_identity_write(source.clone(), id)?;
        let expired = bank.publish(p.scope.clone(), intent)?;
        tokio::time::timeout(std::time::Duration::from_secs(6), exit).await??;
        assert!(bank.take(&p.scope, expired).is_err());
        let operation = owner.begin(p.scope.clone())?;
        drop(operation);
        let intent = owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(source, id)?;
        let retired = bank.publish(p.scope.clone(), intent)?;
        p.documents.started(p.window)?;
        owner.retire_page()?;
        bank.retire()?;
        assert!(bank.take(&p.scope, retired).is_err());
        bank.close()?;
        other.close()?;
        owner.close()?;
        other_owner.close()?;
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        foreign.registry.shutdown()?;
        Ok(())
    })
}
