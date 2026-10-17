use crate::{
    StateStore,
    key_identity_ciphertext::KeyIdentityCiphertextEdit,
    key_identity_vault_source::KeyIdentityVaultSource,
    ssh_key_identities::KeyIdentityEdit,
    vault_database::VaultEdit,
    vault_lifecycle::{AvailableKeyEpoch, VaultKeyLifecycle},
    vault_record::CredentialRecord,
    vault_wrap::{ValidationWrappedDek, create_validation_vault, recover_validation_dek},
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_key_identity::SshPrivateKeyIdentity,
};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
const MASTER: &[u8] = b"fixed-key-pair-master";
fn allowed() -> Arc<dyn Fn() -> bool + Send + Sync> {
    Arc::new(|| true)
}
fn private() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
fn recover(
    vault: Uuid,
    envelope: &ValidationWrappedDek,
) -> Result<crate::vault_wrap::AuthenticatedDek, Box<dyn std::error::Error>> {
    let _serial = crate::vault_kdf::TEST_SCENARIOS
        .lock()
        .map_err(|_| "fixture.poison")?;
    Ok(recover_validation_dek(MASTER, vault, envelope)?)
}
fn confirmed(
    vault: Uuid,
    envelope: &ValidationWrappedDek,
) -> Result<VaultKeyLifecycle, Box<dyn std::error::Error>> {
    let scope = VaultKeyLifecycle::new(vault)?;
    let mut permit = scope.begin()?;
    let mut candidate = permit.worker()?.finish(recover(vault, envelope)?)?;
    scope.confirm(&mut permit, &mut candidate)?;
    Ok(scope)
}
struct Fixture {
    vault: Uuid,
    key: Uuid,
    pass: Option<Uuid>,
    identity: Uuid,
    scope: VaultKeyLifecycle,
    epoch: AvailableKeyEpoch,
}
impl Fixture {
    async fn new(store: &StateStore, pass: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let vault = Uuid::now_v7();
        let (envelope, cipher) = {
            let _serial = crate::vault_kdf::TEST_SCENARIOS
                .lock()
                .map_err(|_| "fixture.poison")?;
            create_validation_vault(MASTER, vault)?
        };
        drop(cipher);
        let scope = confirmed(vault, &envelope)?;
        let epoch = scope.available_epoch()?;
        store
            .edit_vault_ciphertext(
                store.load_vault_ciphertext(allowed()).await?,
                VaultEdit::Initialize(vault, envelope),
                allowed(),
            )
            .await?;
        let key = Uuid::from_bytes([1; 16]);
        let pass = pass.then_some(Uuid::from_bytes([2; 16]));
        let identity = SshPrivateKeyIdentity::new(
            "fixed-pair-identity",
            "user",
            SecretRef::new(vault, key, SecretKind::SshPrivateKey, false)?,
            pass.map(|id| SecretRef::new(vault, id, SecretKind::SshKeyPassphrase, false))
                .transpose()?,
        )?;
        let saved = store
            .edit_ssh_key_identities(
                store.load_ssh_key_identities(allowed()).await?,
                KeyIdentityEdit::Create(identity),
                allowed(),
            )
            .await?;
        let identity = saved.records().next().ok_or("fixture.row")?.id();
        Ok(Self {
            vault,
            key,
            pass,
            identity,
            scope,
            epoch,
        })
    }
    fn record(
        &self,
        id: Uuid,
        kind: SecretKind,
        plain: &[u8],
    ) -> Result<CredentialRecord, Box<dyn std::error::Error>> {
        Ok(CredentialRecord::seal_in_epoch(
            &self.scope,
            &self.epoch,
            id,
            kind.as_str(),
            plain,
        )?)
    }
    fn edit(
        &self,
        source: KeyIdentityVaultSource,
        key: &[u8],
        pass: &[u8],
    ) -> Result<KeyIdentityCiphertextEdit, Box<dyn std::error::Error>> {
        Ok(KeyIdentityCiphertextEdit::new(
            source,
            self.identity,
            self.record(self.key, SecretKind::SshPrivateKey, key)?,
            self.pass
                .map(|id| self.record(id, SecretKind::SshKeyPassphrase, pass))
                .transpose()?,
        )?)
    }
}
fn same_plain(
    source: &KeyIdentityVaultSource,
    scope: &VaultKeyLifecycle,
    epoch: &AvailableKeyEpoch,
    id: Uuid,
    kind: SecretKind,
    plain: &[u8],
) -> TestResult {
    let record = source
        .vault()
        .credentials()
        .iter()
        .find(|r| r.id() == id)
        .ok_or("fixture.record")?;
    let opened = record.open_for_adapter_in_epoch(scope, epoch, id, kind.as_str())?;
    assert!(opened.expose() == plain);
    Ok(())
}
#[tokio::test]
async fn key_pair_atomic_create_replace_reopen_and_ciphertext_only_disk() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let f = Fixture::new(&store, true).await?;
    for (key, pass) in [
        (
            b"fixed-pair-key-one".as_slice(),
            b"fixed-pair-pass-one".as_slice(),
        ),
        (
            b"fixed-pair-key-two".as_slice(),
            b"fixed-pair-pass-two".as_slice(),
        ),
    ] {
        let before = store.load_key_identity_vault_source(allowed()).await?;
        let revision = before.vault().revision();
        let saved = store
            .edit_key_identity_ciphertext(
                f.edit(before, key, pass)?,
                f.scope.clone(),
                f.epoch.clone(),
                allowed(),
            )
            .await?;
        assert_eq!(saved.revision(), revision + 1);
        let current = store.load_key_identity_vault_source(allowed()).await?;
        assert_eq!(current.vault().credentials().len(), 2);
        let row = current.identities().records().next().ok_or("fixture.row")?;
        assert!(row.identity().private_key().has_value());
        assert!(
            row.identity()
                .passphrase()
                .ok_or("fixture.pass")?
                .has_value()
        );
        same_plain(
            &current,
            &f.scope,
            &f.epoch,
            f.key,
            SecretKind::SshPrivateKey,
            key,
        )?;
        same_plain(
            &current,
            &f.scope,
            &f.epoch,
            f.pass.ok_or("fixture.pass")?,
            SecretKind::SshKeyPassphrase,
            pass,
        )?;
    }
    drop(store);
    worker.join().await?;
    let raw = std::fs::read(&path)?;
    for plain in [
        b"fixed-pair-key-one".as_slice(),
        b"fixed-pair-pass-one".as_slice(),
        b"fixed-pair-key-two".as_slice(),
        b"fixed-pair-pass-two".as_slice(),
    ] {
        assert!(!raw.windows(plain.len()).any(|bytes| bytes == plain));
    }
    let (store, worker) = StateStore::open(path).await?;
    let source = store.load_key_identity_vault_source(allowed()).await?;
    same_plain(
        &source,
        &f.scope,
        &f.epoch,
        f.key,
        SecretKind::SshPrivateKey,
        b"fixed-pair-key-two",
    )?;
    same_plain(
        &source,
        &f.scope,
        &f.epoch,
        f.pass.ok_or("fixture.pass")?,
        SecretKind::SshKeyPassphrase,
        b"fixed-pair-pass-two",
    )?;
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn key_pair_roles_ids_namespace_and_optional_reference_are_checked_before_dispatch()
-> TestResult {
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let f = Fixture::new(&store, true).await?;
    let source = store.load_key_identity_vault_source(allowed()).await?;
    for key in [
        f.record(f.key, SecretKind::SshPassword, b"fixed")?,
        f.record(Uuid::now_v7(), SecretKind::SshPrivateKey, b"fixed")?,
        CredentialRecord::seal(
            Uuid::now_v7(),
            f.key,
            "ssh_private_key",
            &crate::vault_crypto::VaultCipher::generate()?,
            b"fixed",
        )?,
    ] {
        assert!(
            KeyIdentityCiphertextEdit::new(
                source.clone(),
                f.identity,
                key,
                Some(f.record(
                    f.pass.ok_or("fixture.pass")?,
                    SecretKind::SshKeyPassphrase,
                    b"fixed"
                )?)
            )
            .is_err()
        );
    }
    assert!(
        KeyIdentityCiphertextEdit::new(
            source.clone(),
            f.identity,
            f.record(f.key, SecretKind::SshPrivateKey, b"fixed")?,
            None
        )
        .is_err()
    );
    assert!(
        KeyIdentityCiphertextEdit::new(
            source,
            f.identity,
            f.record(f.key, SecretKind::SshPrivateKey, b"fixed")?,
            Some(f.record(
                f.pass.ok_or("fixture.pass")?,
                SecretKind::SshPassword,
                b"fixed"
            )?)
        )
        .is_err()
    );
    assert!(
        store
            .load_vault_ciphertext(allowed())
            .await?
            .credentials()
            .is_empty()
    );
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn key_pair_changed_metadata_foreign_instance_and_tombstone_do_not_overwrite_or_resurrect()
-> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let f = Fixture::new(&store, true).await?;
    let stale = store.load_key_identity_vault_source(allowed()).await?;
    let (other, other_worker) = StateStore::open(path).await?;
    assert!(
        other
            .edit_key_identity_ciphertext(
                f.edit(stale.clone(), b"key", b"pass")?,
                f.scope.clone(),
                f.epoch.clone(),
                allowed()
            )
            .await
            .is_err()
    );
    let changed = SshPrivateKeyIdentity::new(
        "changed-label",
        "user",
        SecretRef::new(f.vault, f.key, SecretKind::SshPrivateKey, false)?,
        f.pass
            .map(|id| SecretRef::new(f.vault, id, SecretKind::SshKeyPassphrase, false))
            .transpose()?,
    )?;
    store
        .edit_ssh_key_identities(
            stale.identities().clone(),
            KeyIdentityEdit::Update(f.identity, changed),
            allowed(),
        )
        .await?;
    assert!(
        store
            .edit_key_identity_ciphertext(
                f.edit(stale, b"key", b"pass")?,
                f.scope.clone(),
                f.epoch.clone(),
                allowed()
            )
            .await
            .is_err()
    );
    let source = store.load_key_identity_vault_source(allowed()).await?;
    store
        .edit_key_identity_ciphertext(
            f.edit(source, b"key", b"pass")?,
            f.scope.clone(),
            f.epoch.clone(),
            allowed(),
        )
        .await?;
    let stale = store.load_key_identity_vault_source(allowed()).await?;
    store
        .edit_vault_ciphertext(stale.vault().clone(), VaultEdit::Remove(f.key), allowed())
        .await?;
    assert!(
        store
            .edit_key_identity_ciphertext(
                f.edit(stale, b"resurrect", b"pass")?,
                f.scope.clone(),
                f.epoch.clone(),
                allowed()
            )
            .await
            .is_err()
    );
    assert!(
        f.edit(
            store.load_key_identity_vault_source(allowed()).await?,
            b"resurrect",
            b"pass"
        )
        .is_err()
    );
    assert_eq!(
        store
            .load_vault_ciphertext(allowed())
            .await?
            .credentials()
            .len(),
        1
    );
    drop(store);
    drop(other);
    worker.join().await?;
    other_worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn key_pair_second_sql_insert_failure_rolls_back_both_records_and_revision() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let f = Fixture::new(&store, true).await?;
    let source = store.load_key_identity_vault_source(allowed()).await?;
    let revision = source.vault().revision();
    let connection = rusqlite::Connection::open(path)?;
    connection.execute_batch(&format!("CREATE TRIGGER reject_fixed_pass BEFORE INSERT ON secrets WHEN NEW.id='{}' BEGIN SELECT RAISE(ABORT,'fixed second record failure'); END;", f.pass.ok_or("fixture.pass")?))?;
    assert!(
        store
            .edit_key_identity_ciphertext(
                f.edit(source, b"key", b"pass")?,
                f.scope.clone(),
                f.epoch.clone(),
                allowed()
            )
            .await
            .is_err()
    );
    let current = store.load_vault_ciphertext(allowed()).await?;
    assert!(current.credentials().is_empty());
    assert_eq!(current.revision(), revision);
    connection.execute_batch("DROP TRIGGER reject_fixed_pass")?;
    drop(connection);
    store
        .edit_key_identity_ciphertext(
            f.edit(
                store.load_key_identity_vault_source(allowed()).await?,
                b"key",
                b"pass",
            )?,
            f.scope.clone(),
            f.epoch.clone(),
            allowed(),
        )
        .await?;
    assert_eq!(
        store
            .load_vault_ciphertext(allowed())
            .await?
            .credentials()
            .len(),
        2
    );
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn key_pair_final_authorization_epoch_lock_and_same_dek_reunlock_never_commit_stale_edit()
-> TestResult {
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let f = Fixture::new(&store, true).await?;
    for limit in 0..3 {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let before = store.load_key_identity_vault_source(allowed()).await?;
        let revision = before.vault().revision();
        let result = store
            .edit_key_identity_ciphertext(
                f.edit(before, b"key", b"pass")?,
                f.scope.clone(),
                f.epoch.clone(),
                Arc::new(move || seen.fetch_add(1, Ordering::SeqCst) < limit),
            )
            .await;
        assert!(result.is_err());
        let current = store.load_vault_ciphertext(allowed()).await?;
        assert!(current.credentials().is_empty());
        assert_eq!(current.revision(), revision);
    }
    let before = store.load_key_identity_vault_source(allowed()).await?;
    let edit = f.edit(before.clone(), b"key", b"pass")?;
    let stale = f.edit(before, b"key", b"pass")?;
    let count = Arc::new(AtomicUsize::new(0));
    let scope = f.scope.clone();
    assert!(
        store
            .edit_key_identity_ciphertext(
                edit,
                f.scope.clone(),
                f.epoch.clone(),
                Arc::new(move || {
                    if count.fetch_add(1, Ordering::SeqCst) == 2 {
                        return scope.lock().is_ok();
                    }
                    true
                })
            )
            .await
            .is_err()
    );
    let envelope = store
        .load_vault_ciphertext(allowed())
        .await?
        .envelope()
        .ok_or("fixture.envelope")?
        .clone();
    let mut permit = f.scope.begin()?;
    let mut candidate = permit.worker()?.finish(recover(f.vault, &envelope)?)?;
    f.scope.confirm(&mut permit, &mut candidate)?;
    assert!(
        store
            .edit_key_identity_ciphertext(stale, f.scope.clone(), f.epoch.clone(), allowed())
            .await
            .is_err()
    );
    assert!(
        store
            .load_vault_ciphertext(allowed())
            .await?
            .credentials()
            .is_empty()
    );
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn key_pair_cancelled_wait_keeps_native_credit_until_sql_exits_and_rolls_back() -> TestResult
{
    struct Credit(Arc<AtomicUsize>);
    impl Drop for Credit {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let f = Fixture::new(&store, true).await?;
    let edit = f.edit(
        store.load_key_identity_vault_source(allowed()).await?,
        b"key",
        b"pass",
    )?;
    let credit = Arc::new(AtomicUsize::new(1));
    let held = Credit(credit.clone());
    let (entered, received) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let resume = Arc::new((Mutex::new(false), Condvar::new()));
    let paused = resume.clone();
    let predicate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        let _ = &held;
        if let Ok(mut tx) = entered.lock()
            && let Some(tx) = tx.take()
        {
            let _ = tx.send(());
        }
        if let Ok(ready) = paused.0.lock() {
            let _ =
                paused
                    .1
                    .wait_timeout_while(ready, std::time::Duration::from_secs(5), |ready| !*ready);
        }
        false
    });
    let caller = store.clone();
    let scope = f.scope.clone();
    let epoch = f.epoch.clone();
    let waiting = tokio::spawn(async move {
        caller
            .edit_key_identity_ciphertext(edit, scope, epoch, predicate)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), received).await??;
    waiting.abort();
    assert!(waiting.await.is_err());
    assert_eq!(credit.load(Ordering::SeqCst), 1);
    *resume.0.lock().map_err(|_| "fixture.poison")? = true;
    resume.1.notify_all();
    assert!(
        store
            .load_vault_ciphertext(allowed())
            .await?
            .credentials()
            .is_empty()
    );
    assert_eq!(credit.load(Ordering::SeqCst), 0);
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn key_pair_two_store_concurrent_cas_has_one_whole_pair_winner() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (a, wa) = StateStore::open(path.clone()).await?;
    let f = Fixture::new(&a, true).await?;
    let (b, wb) = StateStore::open(path).await?;
    let ea = f.edit(
        a.load_key_identity_vault_source(allowed()).await?,
        b"key-a",
        b"pass-a",
    )?;
    let eb = f.edit(
        b.load_key_identity_vault_source(allowed()).await?,
        b"key-b",
        b"pass-b",
    )?;
    let (left, right) = tokio::join!(
        a.edit_key_identity_ciphertext(ea, f.scope.clone(), f.epoch.clone(), allowed()),
        b.edit_key_identity_ciphertext(eb, f.scope.clone(), f.epoch.clone(), allowed())
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let current = a.load_key_identity_vault_source(allowed()).await?;
    let (key, pass) = if left.is_ok() {
        (b"key-a".as_slice(), b"pass-a".as_slice())
    } else {
        (b"key-b".as_slice(), b"pass-b".as_slice())
    };
    same_plain(
        &current,
        &f.scope,
        &f.epoch,
        f.key,
        SecretKind::SshPrivateKey,
        key,
    )?;
    same_plain(
        &current,
        &f.scope,
        &f.epoch,
        f.pass.ok_or("fixture.pass")?,
        SecretKind::SshKeyPassphrase,
        pass,
    )?;
    assert_eq!(current.vault().credentials().len(), 2);
    drop(a);
    drop(b);
    wa.join().await?;
    wb.join().await?;
    Ok(())
}
#[tokio::test]
async fn key_pair_unencrypted_identity_accepts_only_single_key_record() -> TestResult {
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let f = Fixture::new(&store, false).await?;
    let source = store.load_key_identity_vault_source(allowed()).await?;
    assert!(
        KeyIdentityCiphertextEdit::new(
            source.clone(),
            f.identity,
            f.record(f.key, SecretKind::SshPrivateKey, b"key")?,
            Some(f.record(Uuid::now_v7(), SecretKind::SshKeyPassphrase, b"pass")?)
        )
        .is_err()
    );
    store
        .edit_key_identity_ciphertext(
            f.edit(source, b"key", b"")?,
            f.scope.clone(),
            f.epoch.clone(),
            allowed(),
        )
        .await?;
    assert_eq!(
        store
            .load_vault_ciphertext(allowed())
            .await?
            .credentials()
            .len(),
        1
    );
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn key_pair_independent_process_reopens_and_authenticates_both_ciphertexts() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let f = Fixture::new(&store, true).await?;
    store
        .edit_key_identity_ciphertext(
            f.edit(
                store.load_key_identity_vault_source(allowed()).await?,
                b"key-child",
                b"pass-child",
            )?,
            f.scope.clone(),
            f.epoch.clone(),
            allowed(),
        )
        .await?;
    drop(store);
    worker.join().await?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "key_identity_ciphertext_tests::key_pair_child_reader",
            "--ignored",
            "--nocapture",
        ])
        .env("PULSE_KEY_PAIR_CHILD_PATH", path)
        .output()?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    Ok(())
}
#[tokio::test]
#[ignore = "exact child helper invoked by independent-process acceptance"]
async fn key_pair_child_reader() -> TestResult {
    let path = std::env::var_os("PULSE_KEY_PAIR_CHILD_PATH").ok_or("fixture.child-path")?;
    let (store, worker) = StateStore::open(path.into()).await?;
    let source = store.load_key_identity_vault_source(allowed()).await?;
    let row = source
        .identities()
        .records()
        .next()
        .ok_or("fixture.child-row")?;
    let scope = confirmed(
        source.vault().vault_id().ok_or("fixture.vault")?,
        source.vault().envelope().ok_or("fixture.envelope")?,
    )?;
    let epoch = scope.available_epoch()?;
    same_plain(
        &source,
        &scope,
        &epoch,
        row.identity().private_key().id(),
        SecretKind::SshPrivateKey,
        b"key-child",
    )?;
    same_plain(
        &source,
        &scope,
        &epoch,
        row.identity().passphrase().ok_or("fixture.pass")?.id(),
        SecretKind::SshKeyPassphrase,
        b"pass-child",
    )?;
    drop(store);
    worker.join().await?;
    Ok(())
}
