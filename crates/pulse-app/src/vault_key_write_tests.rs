use super::*;
#[path = "vault_key_write_bank_tests.rs"]
mod bank;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_key_identity::SshPrivateKeyIdentity,
};
use pulse_store::{
    StateStoreTask, key_identity_vault_source::KeyIdentityVaultSource,
    ssh_key_identities::KeyIdentityEdit, vault_crypto::SecretBytes,
    vault_wrap::recover_validation_dek,
};
const MASTER: &[u8] = b"fixed-private-ingress-master";
fn allowed() -> Authorize {
    Arc::new(|| true)
}
fn bytes(value: &[u8]) -> std::result::Result<SecretBytes, Box<dyn std::error::Error>> {
    Ok(SecretBytes::from_owned(value.to_vec())?)
}
struct Fixture {
    page: Page,
    owner: PageVaultOwner,
    store: StateStore,
    worker: StateStoreTask,
    _root: tempfile::TempDir,
    vault: Uuid,
    key: Uuid,
    pass: Option<Uuid>,
    identity: Uuid,
}
impl Fixture {
    async fn new(with_pass: bool) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let page = page()?;
        let owner = PageVaultOwner::new(page.registry.clone());
        let root = tempfile::tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let empty = store.load_vault_ciphertext(allowed()).await?;
        let operation = owner.begin(page.scope.clone())?;
        let (vault, request) =
            operation.begin_creation(PasswordInput::new(MASTER.to_vec())?, empty.clone())?;
        operation
            .persist_creation(&store, empty, vault, request.ready().await?)
            .await?;
        let source = store.load_vault_ciphertext(allowed()).await?;
        let request =
            operation.begin_unlock(PasswordInput::new(MASTER.to_vec())?, source.clone())?;
        operation
            .confirm_unlock(&store, source, request.ready().await?)
            .await?
            .accept()?;
        let key = Uuid::now_v7();
        let pass = with_pass.then(Uuid::now_v7);
        let saved = store
            .edit_ssh_key_identities(
                store.load_ssh_key_identities(allowed()).await?,
                KeyIdentityEdit::Create(SshPrivateKeyIdentity::new(
                    "fixed-ingress",
                    "user",
                    SecretRef::new(vault, key, SecretKind::SshPrivateKey, false)?,
                    pass.map(|id| SecretRef::new(vault, id, SecretKind::SshKeyPassphrase, false))
                        .transpose()?,
                )?),
                allowed(),
            )
            .await?;
        let identity = saved.records().next().ok_or("fixture.identity")?.id();
        Ok(Self {
            page,
            owner,
            store,
            worker,
            _root: root,
            vault,
            key,
            pass,
            identity,
        })
    }
    async fn source(
        &self,
    ) -> std::result::Result<KeyIdentityVaultSource, Box<dyn std::error::Error>> {
        Ok(self.store.load_key_identity_vault_source(allowed()).await?)
    }
    async fn intent(
        &self,
    ) -> std::result::Result<PrivateKeyIdentityWriteIntent, Box<dyn std::error::Error>> {
        Ok(self
            .owner
            .begin(self.page.scope.clone())?
            .prepare_private_key_identity_write(self.source().await?, self.identity)?)
    }
    fn key_scope(&self) -> std::result::Result<VaultKeyLifecycle, Box<dyn std::error::Error>> {
        Ok(self
            .owner
            .inner
            .guard()?
            .scope
            .clone()
            .ok_or("fixture.scope")?)
    }
    async fn verify(&self, key: &[u8], pass: Option<&[u8]>) -> TestResult {
        let source = self.source().await?;
        let scope = self.key_scope()?;
        let epoch = scope.available_epoch()?;
        let record = source
            .vault()
            .credentials()
            .iter()
            .find(|record| record.id() == self.key)
            .ok_or("fixture.key")?;
        let clear = record.open_for_adapter_in_epoch(
            &scope,
            &epoch,
            self.key,
            SecretKind::SshPrivateKey.as_str(),
        )?;
        assert!(clear.expose() == key);
        if let Some(pass) = pass {
            let id = self.pass.ok_or("fixture.pass")?;
            let record = source
                .vault()
                .credentials()
                .iter()
                .find(|record| record.id() == id)
                .ok_or("fixture.record")?;
            let clear = record.open_for_adapter_in_epoch(
                &scope,
                &epoch,
                id,
                SecretKind::SshKeyPassphrase.as_str(),
            )?;
            assert!(clear.expose() == pass);
        }
        Ok(())
    }
    async fn finish(self) -> TestResult {
        self.owner.close()?;
        drop(self.store);
        self.worker.join().await?;
        self.page.registry.shutdown()?;
        Ok(())
    }
}
#[test]
fn private_key_write_real_epoch_seals_both_roles_and_replaces_one_atomic_pair() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        for (key, pass) in [
            (
                b"fixed-ingress-key-one".as_slice(),
                b"fixed-ingress-pass-one".as_slice(),
            ),
            (
                b"fixed-ingress-key-two".as_slice(),
                b"fixed-ingress-pass-two".as_slice(),
            ),
        ] {
            let revision = f.source().await?.vault().revision();
            let saved = f
                .intent()
                .await?
                .write(&f.store, bytes(key)?, Some(bytes(pass)?), allowed())
                .await?;
            assert_eq!(saved.revision(), revision + 1);
            assert_eq!(saved.credentials().len(), 2);
            f.verify(key, Some(pass)).await?;
        }
        f.finish().await
    })
}
#[test]
fn private_key_write_locked_then_same_dek_reunlock_cannot_upgrade_original_intent() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        let intent = f.intent().await?;
        let scope = f.key_scope()?;
        scope.lock()?;
        let source = f.source().await?;
        let envelope = source.vault().envelope().ok_or("fixture.envelope")?;
        let mut permit = scope.begin()?;
        let mut candidate = permit
            .worker()?
            .finish(recover_validation_dek(MASTER, f.vault, envelope)?)?;
        scope.confirm(&mut permit, &mut candidate)?;
        assert!(
            intent
                .write(&f.store, bytes(b"key")?, Some(bytes(b"pass")?), allowed())
                .await
                .is_err()
        );
        assert!(f.source().await?.vault().credentials().is_empty());
        scope.lock()?;
        assert!(
            f.owner
                .begin(f.page.scope.clone())?
                .prepare_private_key_identity_write(f.source().await?, f.identity)
                .is_err()
        );
        f.finish().await
    })
}
#[test]
fn private_key_write_navigation_and_owner_close_revoke_original_intent() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(false).await?;
        let intent = f.intent().await?;
        f.page.documents.started(f.page.window)?;
        f.owner.retire_page()?;
        let nonce = f.page.documents.finished(f.page.window)?;
        f.page.documents.confirm(f.page.window, nonce)?;
        assert!(
            intent
                .write(&f.store, bytes(b"key")?, None, allowed())
                .await
                .is_err()
        );
        assert!(f.source().await?.vault().credentials().is_empty());
        let current = f.page.documents.current(f.page.window, nonce)?;
        let intent = f
            .owner
            .begin(current)?
            .prepare_private_key_identity_write(f.source().await?, f.identity)?;
        f.owner.close()?;
        assert!(
            intent
                .write(&f.store, bytes(b"key")?, None, allowed())
                .await
                .is_err()
        );
        assert!(f.source().await?.vault().credentials().is_empty());
        f.finish().await
    })
}
#[test]
fn private_key_write_changed_source_foreign_instance_and_optional_role_mismatch_reject()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        assert!(
            f.intent()
                .await?
                .write(&f.store, bytes(b"key")?, None, allowed())
                .await
                .is_err()
        );
        assert!(
            f.intent()
                .await?
                .write(
                    &f.store,
                    bytes(b"key")?,
                    Some(bytes(&[b'p'; 4097])?),
                    allowed()
                )
                .await
                .is_err()
        );
        let stale = f.intent().await?;
        let before = f.source().await?;
        f.store
            .edit_ssh_key_identities(
                before.identities().clone(),
                KeyIdentityEdit::Update(
                    f.identity,
                    SshPrivateKeyIdentity::new(
                        "changed-ingress",
                        "user",
                        SecretRef::new(f.vault, f.key, SecretKind::SshPrivateKey, false)?,
                        f.pass
                            .map(|id| {
                                SecretRef::new(f.vault, id, SecretKind::SshKeyPassphrase, false)
                            })
                            .transpose()?,
                    )?,
                ),
                allowed(),
            )
            .await?;
        assert!(
            stale
                .write(&f.store, bytes(b"key")?, Some(bytes(b"pass")?), allowed())
                .await
                .is_err()
        );
        let path = f._root.path().join("state/state.db");
        let (other, worker) = StateStore::open(path).await?;
        assert!(
            f.intent()
                .await?
                .write(&other, bytes(b"key")?, Some(bytes(b"pass")?), allowed())
                .await
                .is_err()
        );
        assert!(f.source().await?.vault().credentials().is_empty());
        drop(other);
        worker.join().await?;
        f.finish().await
    })
}
#[test]
fn private_key_write_dispatch_gate_precommit_rejection_leaves_revision_and_both_records_unchanged()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        for limit in 0..4 {
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let seen = count.clone();
            let revision = f.source().await?.vault().revision();
            assert!(
                f.intent()
                    .await?
                    .write(
                        &f.store,
                        bytes(b"key")?,
                        Some(bytes(b"pass")?),
                        Arc::new(move || seen.fetch_add(1, Ordering::SeqCst) < limit)
                    )
                    .await
                    .is_err()
            );
            let source = f.source().await?;
            assert_eq!(source.vault().revision(), revision);
            assert!(source.vault().credentials().is_empty());
        }
        f.finish().await
    })
}
#[test]
fn private_key_write_cancelled_sql_wait_keeps_owner_busy_until_actual_exit() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new(true).await?;
        let intent = f.intent().await?;
        let (enter, entered) = tokio::sync::oneshot::channel();
        let enter = Mutex::new(Some(enter));
        let resume = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let paused = resume.clone();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let gate: Authorize = Arc::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 1 {
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
        let caller = f.store.clone();
        let waiting = tokio::spawn(async move {
            intent
                .write(
                    &caller,
                    SecretBytes::from_owned(b"key".to_vec())
                        .map_err(|_| VaultOwnerError::Denied)?,
                    Some(
                        SecretBytes::from_owned(b"pass".to_vec())
                            .map_err(|_| VaultOwnerError::Denied)?,
                    ),
                    gate,
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), entered).await??;
        waiting.abort();
        assert!(waiting.await.is_err());
        assert!(matches!(
            f.owner.begin(f.page.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        *resume.0.lock().map_err(|_| "fixture.poison")? = true;
        resume.1.notify_all();
        assert!(f.source().await?.vault().credentials().is_empty());
        drop(f.owner.begin(f.page.scope.clone())?);
        f.finish().await
    })
}
