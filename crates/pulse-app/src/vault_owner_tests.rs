use super::*;
#[cfg(unix)]
#[path = "vault_key_write_tests.rs"]
mod key_write;
use crate::product_document::ProductDocuments;
use crate::registry::WindowToken;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[cfg(all(unix, feature = "ssh-use"))]
#[path = "vault_key_preparation_tests.rs"]
mod key_preparation;
#[cfg(all(unix, feature = "ssh-use"))]
#[path = "vault_session_gesture_tests.rs"]
mod session_gesture;
#[test]
fn poisoned_owner_closes_key_scope_instead_of_only_returning_denied() -> TestResult {
    let p = page()?;
    let owner = PageVaultOwner::new(p.registry.clone());
    let scope = VaultKeyLifecycle::new(Uuid::now_v7())?;
    owner.inner.state.lock().map_err(|_| "fixture.owner")?.scope = Some(scope.clone());
    let permit = scope.begin()?;
    let poison = owner.clone();
    assert!(
        std::thread::spawn(move || {
            let _state = poison.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            panic!("fixture.owner_poison");
        })
        .join()
        .is_err()
    );
    assert!(owner.lock().is_err());
    assert!(scope.status().is_err());
    assert!(owner.begin(p.scope).is_err());
    drop(permit);
    p.registry.shutdown()?;
    Ok(())
}

struct Page {
    registry: SessionRegistry,
    documents: ProductDocuments,
    window: WindowToken,
    scope: Arc<PageLaunchScope>,
}
fn page() -> std::result::Result<Page, Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let window = registry.open_window()?;
    documents.install(window)?;
    documents.started(window)?;
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let scope = documents.current(window, nonce)?;
    Ok(Page {
        registry,
        documents,
        window,
        scope,
    })
}
#[cfg(unix)]
fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(4)
        .build()
}
#[test]
fn actual_predicate_lifetime_keeps_cancelled_slot_and_foreign_retired_pages_reject() -> TestResult {
    let p = page()?;
    let foreign = page()?;
    let owner = PageVaultOwner::new(p.registry.clone());
    assert!(matches!(
        owner.begin(foreign.scope),
        Err(VaultOwnerError::Denied)
    ));
    let operation = owner.begin(p.scope.clone())?;
    let native = operation.authorization();
    assert!(native());
    drop(operation);
    assert!(!native());
    assert!(matches!(
        owner.begin(p.scope.clone()),
        Err(VaultOwnerError::Busy)
    ));
    drop(native);
    let operation = owner.begin(p.scope.clone())?;
    let native = operation.authorization();
    p.documents.started(p.window)?;
    owner.retire_page()?;
    assert!(!native());
    drop(operation);
    assert!(owner.begin(p.scope).is_err());
    drop(native);
    p.registry.shutdown()?;
    foreign.registry.shutdown()?;
    Ok(())
}
#[cfg(unix)]
#[test]
fn foreign_same_empty_source_never_aliases_or_closes_either_worker() -> TestResult {
    runtime()?.block_on(async {
        let p = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let operation = owner.begin(p.scope.clone())?;
        let root = crate::test_tempdir()?;
        let a = pulse_store::state_directory::prepare(&root.path().join("a"))?;
        let b = pulse_store::state_directory::prepare(&root.path().join("b"))?;
        let (a, aw) = StateStore::open(a.join("state.db")).await?;
        let (b, bw) = StateStore::open(b.join("state.db")).await?;
        let one = a.load_vault_ciphertext(Arc::new(|| true)).await?;
        let two = b.load_vault_ciphertext(Arc::new(|| true)).await?;
        assert_eq!(operation.status(one)?, None);
        assert!(matches!(
            operation.status(two),
            Err(VaultOwnerError::Denied)
        ));
        assert!(
            a.load_vault_ciphertext(Arc::new(|| true))
                .await?
                .vault_id()
                .is_none()
        );
        assert!(
            b.load_vault_ciphertext(Arc::new(|| true))
                .await?
                .vault_id()
                .is_none()
        );
        drop(operation);
        drop(a);
        drop(b);
        aw.join().await?;
        bw.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
#[cfg(unix)]
#[test]
fn actual_create_unlock_refresh_lock_and_pending_retirement_remain_distinct() -> TestResult {
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
        let (id, request) = operation.begin_creation(
            PasswordInput::new(b"fixed-owner-validation-password".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, id, request.ready().await?)
            .await?;
        let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        assert_eq!(operation.status(saved.clone())?, Some(KeyStatus::Locked));
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-owner-validation-password".to_vec())?,
            saved.clone(),
        )?;
        operation
            .confirm_unlock(&store, saved.clone(), request.ready().await?)
            .await?
            .accept()?;
        let operation = owner.begin(p.scope.clone())?;
        assert_eq!(operation.status(saved.clone())?, Some(KeyStatus::Available));
        drop(operation);
        p.documents.started(p.window)?;
        owner.retire_page()?;
        let nonce = p.documents.finished(p.window)?;
        p.documents.confirm(p.window, nonce)?;
        let current = p.documents.current(p.window, nonce)?;
        let operation = owner.begin(current.clone())?;
        assert_eq!(operation.status(saved.clone())?, Some(KeyStatus::Available));
        owner.lock()?;
        drop(operation);
        let operation = owner.begin(current.clone())?;
        assert_eq!(operation.status(saved.clone())?, Some(KeyStatus::Locked));
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-owner-validation-password".to_vec())?,
            saved.clone(),
        )?;
        let lost = operation
            .confirm_unlock(&store, saved.clone(), request.ready().await?)
            .await?;
        let cancel = lost.operation.cancellation();
        cancel.cancel(); // native waiter lost after SQL enabled but before result acceptance
        assert!(lost.accept().is_err());
        let operation = owner.begin(current.clone())?;
        assert_eq!(operation.status(saved.clone())?, Some(KeyStatus::Locked));
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-owner-validation-password".to_vec())?,
            saved.clone(),
        )?;
        let ready = request.ready().await?;
        p.documents.started(p.window)?;
        owner.retire_page()?;
        assert!(
            operation
                .confirm_unlock(&store, saved, ready)
                .await
                .is_err()
        );
        let scope = owner
            .inner
            .state
            .lock()
            .map_err(|_| "fixture.owner")?
            .scope
            .clone()
            .ok_or("fixture.scope")?;
        assert_eq!(scope.status()?, KeyStatus::Locked);
        owner.close()?;
        owner.close()?;
        assert!(scope.status().is_err());
        assert!(
            store
                .load_ssh_identities(Arc::new(|| true))
                .await?
                .vault_id()
                .is_some()
        );
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
#[path = "vault_cancel_observer_tests.rs"]
mod cancellation_observer_tests;
#[cfg(unix)]
#[path = "vault_identity_source_tests.rs"]
mod identity_source_tests;
#[cfg(unix)]
#[path = "vault_identity_write_tests.rs"]
mod identity_write_tests;
#[cfg(unix)]
#[path = "vault_password_bank_tests.rs"]
mod password_bank_tests;
#[cfg(unix)]
#[path = "vault_password_intent_tests.rs"]
mod password_intent_tests;
#[cfg(all(unix, feature = "ssh-use"))]
#[path = "vault_ssh_preparation_tests.rs"]
mod ssh_preparation_tests;
#[cfg(all(unix, feature = "ssh-use"))]
#[path = "vault_target_action_tests.rs"]
mod target_action_tests;
