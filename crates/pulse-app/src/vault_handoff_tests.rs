use super::*;
use crate::product_document::ProductDocuments;
use crate::vault_owner::PageVaultOwner;
#[cfg(unix)]
use pulse_store::{StateStore, vault_lifecycle::KeyStatus, vault_unlock::PasswordInput};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn active_timer_expires_without_frontend_read_or_extending_original_cutoff() -> TestResult {
    let p = page()?;
    let owner = PageVaultOwner::new(p.registry.clone());
    let bank = VaultHandoffBank::new(&owner);
    let id = bank.prepare(p.scope.clone(), HandoffKind::Unlock)?;
    let permit = bank.begin(id, &p.scope, HandoffKind::Unlock)?;
    let mut changed = permit.token.wake.subscribe();
    assert!((permit.authorization())());
    tokio::time::timeout(Duration::from_secs(6), changed.changed()).await??;
    assert!(!(permit.authorization())());
    assert!(bank.inner.guard()?.slot.is_none());
    assert!(bank.confirm(id, &p.scope).is_err());
    bank.close()?;
    owner.close()?;
    p.registry.shutdown()?;
    Ok(())
}

struct Page {
    registry: SessionRegistry,
    documents: ProductDocuments,
    owner: crate::registry::WindowToken,
    scope: Arc<PageLaunchScope>,
}
fn page() -> std::result::Result<Page, Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let scope = documents.current(owner, nonce)?;
    Ok(Page {
        registry,
        documents,
        owner,
        scope,
    })
}
#[tokio::test]
async fn prepared_single_use_exact_page_registry_mode_cancel_and_replacement() -> TestResult {
    let p = page()?;
    let other = page()?;
    let owner = PageVaultOwner::new(p.registry.clone());
    let bank = VaultHandoffBank::new(&owner);
    assert!(
        bank.prepare(other.scope.clone(), HandoffKind::Unlock)
            .is_err()
    );
    let id = bank.prepare(p.scope.clone(), HandoffKind::Unlock)?;
    assert!(bank.prepare(p.scope.clone(), HandoffKind::Unlock).is_err());
    assert!(
        bank.begin(Uuid::now_v7(), &p.scope, HandoffKind::Unlock)
            .is_err()
    );
    assert!(bank.begin(id, &p.scope, HandoffKind::Create).is_err());
    assert!(bank.begin(id, &other.scope, HandoffKind::Unlock).is_err());
    let permit = bank.begin(id, &p.scope, HandoffKind::Unlock)?;
    assert!(bank.begin(id, &p.scope, HandoffKind::Unlock).is_err());
    assert!((permit.authorization())());
    let foreign_owner = PageVaultOwner::new(p.registry.clone());
    let foreign_op = foreign_owner.begin(p.scope.clone())?;
    assert!(permit.register_cancel(foreign_op.cancellation()).is_err());
    assert!((foreign_op.authorization())());
    drop(foreign_op);
    assert!(bank.cancel(id, &other.scope).is_err());
    assert!((permit.authorization())());
    bank.cancel(id, &p.scope)?;
    assert!(!(permit.authorization())());
    let fresh = bank.prepare(p.scope.clone(), HandoffKind::Create)?;
    assert!(bank.cancel(id, &p.scope).is_err());
    let next = bank.begin(fresh, &p.scope, HandoffKind::Create)?;
    assert!((next.authorization())());
    next.finish_creation()?;
    assert!(bank.confirm(fresh, &p.scope).is_err());
    p.registry.shutdown()?;
    other.registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn native_retirement_drop_expired_permit_and_close_never_confirm() -> TestResult {
    let p = page()?;
    let owner = PageVaultOwner::new(p.registry.clone());
    let bank = VaultHandoffBank::new(&owner);
    let id = bank.prepare(p.scope.clone(), HandoffKind::Unlock)?;
    let permit = bank.begin(id, &p.scope, HandoffKind::Unlock)?;
    let gate = permit.authorization();
    drop(permit);
    assert!(!gate());
    let id = bank.prepare(p.scope.clone(), HandoffKind::Unlock)?;
    let permit = bank.begin(id, &p.scope, HandoffKind::Unlock)?;
    *permit
        .token
        .deadline
        .lock()
        .map_err(|_| "fixture.deadline")? = Instant::now();
    assert!(!(permit.authorization())());
    assert!(bank.confirm(id, &p.scope).is_err());
    let id = bank.prepare(p.scope.clone(), HandoffKind::Unlock)?;
    let permit = bank.begin(id, &p.scope, HandoffKind::Unlock)?;
    p.documents.started(p.owner)?;
    bank.retire()?;
    assert!(!(permit.authorization())());
    assert!(bank.confirm(id, &p.scope).is_err());
    bank.close()?;
    p.registry.shutdown()?;
    Ok(())
}
#[cfg(unix)]
#[test]
fn actual_ready_cancel_stays_locked_then_explicit_front_confirm_accepts_without_deadlock()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(3)
        .build()?;
    runtime.block_on(async {
        let p = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let bank = VaultHandoffBank::new(&owner);
        let root = tempfile::tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let operation = owner.begin(p.scope.clone())?;
        let (id, request) = operation.begin_creation(
            PasswordInput::new(b"fixed-handoff-validation-password".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, id, request.ready().await?)
            .await?;
        drop(operation);
        let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        for round in 0..3 {
            let confirmed = round == 2;
            let foreign = PageVaultOwner::new(p.registry.clone());
            let id = bank.prepare(p.scope.clone(), HandoffKind::Unlock)?;
            let permit = bank.begin(id, &p.scope, HandoffKind::Unlock)?;
            let operation = if round == 0 {
                foreign.begin_authorized(p.scope.clone(), permit.authorization())?
            } else {
                owner.begin_authorized(p.scope.clone(), permit.authorization())?
            };
            if round != 0 {
                permit.register_cancel(operation.cancellation())?;
            }
            let request = operation.begin_unlock(
                PasswordInput::new(b"fixed-handoff-validation-password".to_vec())?,
                saved.clone(),
            )?;
            let activation = operation
                .confirm_unlock(&store, saved.clone(), request.ready().await?)
                .await?;
            if round == 0 {
                assert!(permit.publish(activation).is_err());
                assert!(bank.confirm(id, &p.scope).is_err());
                bank.cancel(id, &p.scope)?;
                let op = foreign.begin(p.scope.clone())?;
                assert_eq!(op.status(saved.clone())?, Some(KeyStatus::Locked));
                drop(op);
                foreign.close()?;
                continue;
            }
            permit.publish(activation)?;
            assert!(owner.begin(p.scope.clone()).is_err());
            if confirmed {
                bank.confirm(id, &p.scope)?;
                assert!(bank.confirm(id, &p.scope).is_err());
            } else {
                bank.cancel(id, &p.scope)?;
                assert!(bank.confirm(id, &p.scope).is_err());
            }
            let operation = owner.begin(p.scope.clone())?;
            assert_eq!(
                operation.status(saved.clone())?,
                Some(if confirmed {
                    KeyStatus::Available
                } else {
                    KeyStatus::Locked
                })
            );
            drop(operation);
        }
        owner.close()?;
        bank.close()?;
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
