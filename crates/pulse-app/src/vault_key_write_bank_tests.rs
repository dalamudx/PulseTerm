use super::*;
use crate::vault_key_write_handoff::PrivateKeyWriteBank;
#[test]
fn private_key_bank_original_controller_duplicate_wrong_ticket_and_success() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        let bank = PrivateKeyWriteBank::new(&f.owner);
        let other_owner = PageVaultOwner::new(f.page.registry.clone());
        let foreign_bank = PrivateKeyWriteBank::new(&other_owner);
        assert!(
            foreign_bank
                .publish(f.page.scope.clone(), f.intent().await?)
                .is_err()
        );
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        assert!(bank.take(&f.page.scope, Uuid::new_v4()).is_err());
        let foreign = page()?;
        assert!(bank.take(&foreign.scope, id).is_err());
        let permit = bank.take(&f.page.scope, id)?;
        assert!(bank.take(&f.page.scope, id).is_err());
        permit
            .write(
                &f.store,
                bytes(b"fixed-bank-key")?,
                Some(bytes(b"fixed-bank-pass")?),
            )
            .await?;
        f.verify(b"fixed-bank-key", Some(b"fixed-bank-pass"))
            .await?;
        bank.close()?;
        other_owner.close()?;
        foreign.registry.shutdown()?;
        f.finish().await
    })
}
#[test]
fn private_key_bank_taken_cancel_and_dropped_permit_leave_no_ciphertext() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        let bank = PrivateKeyWriteBank::new(&f.owner);
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        let permit = bank.take(&f.page.scope, id)?;
        assert!(bank.cancel(&f.page.scope, Uuid::new_v4()).is_err());
        bank.cancel(&f.page.scope, id)?;
        assert!(
            permit
                .write(&f.store, bytes(b"key")?, Some(bytes(b"pass")?))
                .await
                .is_err()
        );
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        drop(bank.take(&f.page.scope, id)?);
        assert!(bank.take(&f.page.scope, id).is_err());
        assert!(f.source().await?.vault().credentials().is_empty());
        bank.close()?;
        f.finish().await
    })
}
#[test]
fn private_key_bank_refresh_rejects_original_and_replacement_pages_then_native_retire_releases()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(false).await?;
        let bank = PrivateKeyWriteBank::new(&f.owner);
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        f.page.documents.started(f.page.window)?;
        f.owner.retire_page()?;
        let nonce = f.page.documents.finished(f.page.window)?;
        f.page.documents.confirm(f.page.window, nonce)?;
        let current = f.page.documents.current(f.page.window, nonce)?;
        assert!(bank.take(&f.page.scope, id).is_err());
        assert!(bank.take(&current, id).is_err());
        bank.retire()?;
        let intent = f
            .owner
            .begin(current.clone())?
            .prepare_private_key_identity_write(f.source().await?, f.identity)?;
        let new_id = bank.publish(current.clone(), intent)?;
        bank.take(&current, new_id)?
            .write(&f.store, bytes(b"current-key")?, None)
            .await?;
        f.verify(b"current-key", None).await?;
        bank.close()?;
        f.finish().await
    })
}
#[test]
fn private_key_bank_same_dek_reunlock_rejects_old_epoch_at_take() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        let bank = PrivateKeyWriteBank::new(&f.owner);
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        let scope = f.key_scope()?;
        scope.lock()?;
        let source = f.source().await?;
        let mut permit = scope.begin()?;
        let mut candidate = permit.worker()?.finish(recover_validation_dek(
            MASTER,
            f.vault,
            source.vault().envelope().ok_or("fixture.envelope")?,
        )?)?;
        scope.confirm(&mut permit, &mut candidate)?;
        assert!(bank.take(&f.page.scope, id).is_err());
        bank.retire()?;
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        bank.take(&f.page.scope, id)?
            .write(&f.store, bytes(b"new-key")?, Some(bytes(b"new-pass")?))
            .await?;
        f.verify(b"new-key", Some(b"new-pass")).await?;
        bank.close()?;
        f.finish().await
    })
}
#[test]
fn private_key_bank_actual_five_second_timer_retires_ready_and_taken_intents() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(false).await?;
        let bank = PrivateKeyWriteBank::new(&f.owner);
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        tokio::time::sleep(std::time::Duration::from_millis(5050)).await;
        assert!(bank.take(&f.page.scope, id).is_err());
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        let permit = bank.take(&f.page.scope, id)?;
        tokio::time::sleep(std::time::Duration::from_millis(5050)).await;
        assert!(
            permit
                .write(&f.store, bytes(b"expired-key")?, None)
                .await
                .is_err()
        );
        assert!(f.source().await?.vault().credentials().is_empty());
        bank.close()?;
        f.finish().await
    })
}
#[test]
fn private_key_bank_close_prevents_late_publication_and_preserves_confirmed_master_key()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        let bank = PrivateKeyWriteBank::new(&f.owner);
        let id = bank.publish(f.page.scope.clone(), f.intent().await?)?;
        let permit = bank.take(&f.page.scope, id)?;
        bank.close()?;
        assert!(
            permit
                .write(&f.store, bytes(b"key")?, Some(bytes(b"pass")?))
                .await
                .is_err()
        );
        assert_eq!(f.key_scope()?.status()?, KeyStatus::Available);
        assert!(
            bank.publish(f.page.scope.clone(), f.intent().await?)
                .is_err()
        );
        assert!(f.source().await?.vault().credentials().is_empty());
        f.finish().await
    })
}
#[test]
fn private_key_bank_cancel_active_native_predicate_retains_owner_slot_until_actual_sql_exit()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        let bank = PrivateKeyWriteBank::new(&f.owner);
        let source = f.source().await?;
        let thread = std::thread::current().id();
        let (enter, entered) = tokio::sync::oneshot::channel();
        let enter = Mutex::new(Some(enter));
        let resume = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let paused = resume.clone();
        let gate: Authorize = Arc::new(move || {
            if std::thread::current().id() != thread {
                if let Ok(mut enter) = enter.lock()
                    && let Some(enter) = enter.take()
                {
                    let _ = enter.send(());
                }
                if let Ok(ready) = paused.0.lock() {
                    let _ = paused.1.wait_timeout_while(
                        ready,
                        std::time::Duration::from_secs(5),
                        |ready| !*ready,
                    );
                }
            }
            true
        });
        let intent = f
            .owner
            .begin_authorized(f.page.scope.clone(), gate)?
            .prepare_private_key_identity_write(source, f.identity)?;
        let id = bank.publish(f.page.scope.clone(), intent)?;
        let permit = bank.take(&f.page.scope, id)?;
        let caller = f.store.clone();
        let task = tokio::spawn(async move {
            permit
                .write(
                    &caller,
                    SecretBytes::from_owned(b"key".to_vec()).map_err(|_| "fixture.key")?,
                    Some(SecretBytes::from_owned(b"pass".to_vec()).map_err(|_| "fixture.pass")?),
                )
                .await
                .map_err(|_| "fixture.dispatch")
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), entered).await??;
        bank.cancel(&f.page.scope, id)?;
        task.abort();
        assert!(task.await.is_err());
        assert!(matches!(
            f.owner.begin(f.page.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        *resume.0.lock().map_err(|_| "fixture.poison")? = true;
        resume.1.notify_all();
        assert!(f.source().await?.vault().credentials().is_empty());
        drop(f.owner.begin(f.page.scope.clone())?);
        bank.close()?;
        f.finish().await
    })
}
