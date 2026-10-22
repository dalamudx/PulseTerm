use super::*;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_endpoint::SshEndpoint,
    ssh_key_identity::SshPrivateKeyIdentity,
};
use pulse_store::{
    StateStoreTask, ssh_hosts::HostEdit, ssh_key_identities::KeyIdentityEdit,
    ssh_key_target_source::SshKeyTargetSource, vault_record::CredentialRecord,
};
use std::sync::atomic::AtomicUsize;
#[path = "vault_key_target_action_tests.rs"]
mod target_action;

#[path = "vault_key_target_session_tests.rs"]
mod target_session;
struct Fixture {
    page: Page,
    owner: PageVaultOwner,
    store: StateStore,
    worker: StateStoreTask,
    _root: tempfile::TempDir,
    vault: Uuid,
    host: Uuid,
    identity: Uuid,
    key: Uuid,
}
impl Fixture {
    async fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let page = page()?;
        let owner = PageVaultOwner::new(page.registry.clone());
        let root = crate::test_tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let operation = owner.begin(page.scope.clone())?;
        let (vault, request) = operation.begin_creation(
            PasswordInput::new(b"fixed-private-preparation-master".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, vault, request.ready().await?)
            .await?;
        let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-private-preparation-master".to_vec())?,
            saved.clone(),
        )?;
        operation
            .confirm_unlock(&store, saved, request.ready().await?)
            .await?
            .accept()?;
        let key = Uuid::now_v7();
        let identity = store
            .edit_ssh_key_identities(
                store.load_ssh_key_identities(Arc::new(|| true)).await?,
                KeyIdentityEdit::Create(SshPrivateKeyIdentity::new(
                    "fixed-key",
                    "user",
                    SecretRef::new(vault, key, SecretKind::SshPrivateKey, false)?,
                    None,
                )?),
                Arc::new(|| true),
            )
            .await?
            .records()
            .next()
            .ok_or("fixture.identity")?
            .id();
        let hosts = store
            .edit_ssh_hosts(
                store.load_ssh_hosts().await?,
                HostEdit::Create(SshEndpoint::new(
                    "fixed-target",
                    "fixed.example",
                    22,
                    "user",
                )?),
                Arc::new(|| true),
            )
            .await?;
        let host = hosts.records()[0].id();
        Ok(Self {
            page,
            owner,
            store,
            worker,
            _root: root,
            vault,
            host,
            identity,
            key,
        })
    }
    async fn seal(&self, id: Uuid, kind: SecretKind) -> TestResult {
        let scope = self
            .owner
            .inner
            .guard()?
            .scope
            .clone()
            .ok_or("fixture.scope")?;
        let epoch = scope.available_epoch()?;
        let record = CredentialRecord::seal_in_epoch(
            &scope,
            &epoch,
            id,
            kind.as_str(),
            b"fixed-role-bytes",
        )?;
        self.store
            .edit_vault_ciphertext(
                self.store.load_vault_ciphertext(Arc::new(|| true)).await?,
                VaultEdit::Create(record),
                Arc::new(|| true),
            )
            .await?;
        Ok(())
    }
    async fn source(&self) -> std::result::Result<SshKeyTargetSource, Box<dyn std::error::Error>> {
        Ok(self
            .store
            .load_ssh_key_target_source(Arc::new(|| true))
            .await?)
    }
    async fn prepare(
        &self,
    ) -> std::result::Result<super::super::SavedPrivateKeyUsePreparation, Box<dyn std::error::Error>>
    {
        Ok(self
            .owner
            .begin(self.page.scope.clone())?
            .prepare_saved_private_key_use(
                &self.store,
                self.source().await?,
                self.host,
                self.identity,
            )
            .await?)
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
fn key_preparation_requires_selected_cipher_roles_and_remains_only_a_point_check() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        assert!(f.prepare().await.is_err());
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        f.prepare().await?.validate(&f.store).await?;
        assert!(
            f.owner
                .begin(f.page.scope.clone())?
                .prepare_saved_private_key_use(
                    &f.store,
                    f.source().await?,
                    Uuid::now_v7(),
                    f.identity
                )
                .await
                .is_err()
        );
        assert!(
            f.owner
                .begin(f.page.scope.clone())?
                .prepare_saved_private_key_use(&f.store, f.source().await?, f.host, Uuid::now_v7())
                .await
                .is_err()
        );
        let pass = Uuid::now_v7();
        f.store
            .edit_ssh_key_identities(
                f.source().await?.identities().clone(),
                KeyIdentityEdit::Update(
                    f.identity,
                    SshPrivateKeyIdentity::new(
                        "fixed-key",
                        "user",
                        SecretRef::new(f.vault, f.key, SecretKind::SshPrivateKey, false)?,
                        Some(SecretRef::new(
                            f.vault,
                            pass,
                            SecretKind::SshKeyPassphrase,
                            false,
                        )?),
                    )?,
                ),
                Arc::new(|| true),
            )
            .await?;
        assert!(f.prepare().await.is_err());
        let scope = f
            .owner
            .inner
            .guard()?
            .scope
            .clone()
            .ok_or("fixture.scope")?;
        let epoch = scope.available_epoch()?;
        let wrong = CredentialRecord::seal_in_epoch(
            &scope,
            &epoch,
            pass,
            SecretKind::SshPassword.as_str(),
            b"fixed-wrong-role",
        )?;
        assert!(
            wrong
                .open_for_adapter_in_epoch(
                    &scope,
                    &epoch,
                    pass,
                    SecretKind::SshKeyPassphrase.as_str()
                )
                .is_err()
        );
        f.seal(pass, SecretKind::SshKeyPassphrase).await?;
        f.prepare().await?.validate(&f.store).await?;
        let old = f.prepare().await?;
        f.store
            .edit_vault_ciphertext(
                f.source().await?.vault().clone(),
                VaultEdit::Remove(pass),
                Arc::new(|| true),
            )
            .await?;
        assert!(old.validate(&f.store).await.is_err());
        assert!(f.prepare().await.is_err());
        f.finish().await
    })
}
#[test]
fn key_preparation_cannot_upgrade_epoch_source_page_native_guard_or_foreign_store() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let original = f.prepare().await?;
        let scope = f
            .owner
            .inner
            .guard()?
            .scope
            .clone()
            .ok_or("fixture.scope")?;
        let epoch = scope.available_epoch()?;
        scope.lock()?;
        assert!(scope.available_epoch().is_err());
        let source = f.source().await?;
        assert!(f.prepare().await.is_err());
        let saved = f.store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = pulse_store::vault_unlock::begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed-private-preparation-master".to_vec())?,
            saved.envelope().ok_or("fixture.envelope")?.clone(),
        )?;
        let mut ready = request.ready().await?;
        ready.confirm(&scope)?;
        drop(ready);
        assert!(!epoch.is_current_for(&scope)?);
        assert!(original.validate(&f.store).await.is_err());
        assert!(
            f.owner
                .begin(f.page.scope.clone())?
                .prepare_saved_private_key_use_pinned(
                    &f.store,
                    source,
                    f.host,
                    f.identity,
                    Some(epoch)
                )
                .await
                .is_err()
        );
        f.prepare().await?.validate(&f.store).await?;
        let original = f.prepare().await?;
        let source = f.source().await?;
        f.store
            .edit_ssh_hosts(
                source.hosts().clone(),
                HostEdit::Update(
                    f.host,
                    SshEndpoint::new("changed-target", "other.example", 22, "user")?,
                ),
                Arc::new(|| true),
            )
            .await?;
        assert!(original.validate(&f.store).await.is_err());
        let live = Arc::new(AtomicBool::new(true));
        let guard = live.clone();
        let native = f
            .owner
            .begin_authorized(
                f.page.scope.clone(),
                Arc::new(move || guard.load(Ordering::Acquire)),
            )?
            .prepare_saved_private_key_use(&f.store, f.source().await?, f.host, f.identity)
            .await?;
        live.store(false, Ordering::Release);
        assert!(native.validate(&f.store).await.is_err());
        let other_dir = pulse_store::state_directory::prepare(&f._root.path().join("other"))?;
        let (other, other_worker) = StateStore::open(other_dir.join("state.db")).await?;
        assert!(
            f.owner
                .begin(f.page.scope.clone())?
                .prepare_saved_private_key_use(&other, f.source().await?, f.host, f.identity)
                .await
                .is_err()
        );
        drop(other);
        other_worker.join().await?;
        let page = f.prepare().await?;
        f.page.documents.started(f.page.window)?;
        f.owner.retire_page()?;
        assert!(page.validate(&f.store).await.is_err());
        f.finish().await
    })
}
#[test]
fn key_preparation_cancelled_waiter_keeps_native_guard_until_sql_exit() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        let armed = Arc::new(AtomicBool::new(false));
        let armed_guard = armed.clone();
        let calls = AtomicUsize::new(0);
        let (entered, observed) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered));
        let (release, released) = std::sync::mpsc::channel();
        let released = Mutex::new(released);
        let prepared = f
            .owner
            .begin_authorized(
                f.page.scope.clone(),
                Arc::new(move || {
                    let _retained = &lease;
                    if armed_guard.load(Ordering::Acquire)
                        && calls.fetch_add(1, Ordering::AcqRel) == 1
                    {
                        if let Some(entered) =
                            entered.lock().unwrap_or_else(|p| p.into_inner()).take()
                        {
                            let _ = entered.send(());
                        }
                        let _ = released
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .recv_timeout(std::time::Duration::from_secs(3));
                    }
                    true
                }),
            )?
            .prepare_saved_private_key_use(&f.store, f.source().await?, f.host, f.identity)
            .await?;
        armed.store(true, Ordering::Release);
        let store = f.store.clone();
        let task = tokio::spawn(async move { prepared.validate(&store).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), observed).await??;
        task.abort();
        assert!(task.await.is_err());
        assert!(weak.upgrade().is_some());
        release.send(())?;
        f.source().await?;
        assert!(weak.upgrade().is_none());
        f.finish().await
    })
}
