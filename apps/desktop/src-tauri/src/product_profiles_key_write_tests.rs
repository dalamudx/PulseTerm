use super::super::super::vault::Intent;
use super::*;
use pulse_app::{product_document::ProductDocuments, registry::WindowToken};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_key_identity::SshPrivateKeyIdentity,
};
use pulse_store::{ssh_key_identities::KeyIdentityEdit, vault_unlock::PasswordInput};
use russh::keys::{
    PrivateKey,
    ssh_key::{Cipher, Kdf, LineEnding, private::Ed25519Keypair},
};
use std::ops::AsyncFnOnce;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
const MASTER: &[u8] = b"fixed-native-private-key-master";
const PASS: &[u8] = b"fixed-native-private-key-passphrase";
struct Fixture {
    _temp: tempfile::TempDir,
    registry: SessionRegistry,
    documents: ProductDocuments,
    window: WindowToken,
    page: Arc<PageLaunchScope>,
    reader: Arc<ProfileReader>,
    owner: PageVaultOwner,
    bank: PrivateKeyWriteBank,
    directory: PathBuf,
    store: StateStore,
    identity: Uuid,
    pem: SecretBytes,
    encrypted: bool,
}
impl Fixture {
    async fn new(encrypted: bool) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        Self::new_at(encrypted, None).await
    }
    async fn new_at(
        encrypted: bool,
        directory: Option<PathBuf>,
    ) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let registry = SessionRegistry::new();
        let documents = ProductDocuments::new(registry.clone());
        let window = registry.open_window()?;
        documents.install(window)?;
        documents.started(window)?;
        let nonce = documents.finished(window)?;
        documents.confirm(window, nonce)?;
        let page = documents.current(window, nonce)?;
        let reader = Arc::new(ProfileReader::new());
        let owner = PageVaultOwner::new(registry.clone());
        let bank = PrivateKeyWriteBank::new(&owner);
        let temp = tempfile::tempdir()?;
        let directory = directory.unwrap_or_else(|| temp.path().join("state"));
        reader
            .vault(
                registry.clone(),
                page.clone(),
                directory.clone(),
                owner.clone(),
                Intent::Create(PasswordInput::new(MASTER.to_vec())?),
            )
            .await?;
        reader
            .vault(
                registry.clone(),
                page.clone(),
                directory.clone(),
                owner.clone(),
                Intent::Unlock(PasswordInput::new(MASTER.to_vec())?),
            )
            .await?;
        let store = reader.store.get().ok_or("fixture.store")?.store.clone();
        let vault_id = store
            .load_vault_ciphertext(Arc::new(|| true))
            .await?
            .vault_id()
            .ok_or("fixture.vault")?;
        let identity = SshPrivateKeyIdentity::new(
            "fixed-native-key",
            "fixed-user",
            SecretRef::new(vault_id, Uuid::now_v7(), SecretKind::SshPrivateKey, false)?,
            encrypted
                .then(|| {
                    SecretRef::new(
                        vault_id,
                        Uuid::now_v7(),
                        SecretKind::SshKeyPassphrase,
                        false,
                    )
                })
                .transpose()?,
        )?;
        let source = store.load_ssh_key_identities(Arc::new(|| true)).await?;
        let catalog = store
            .edit_ssh_key_identities(source, KeyIdentityEdit::Create(identity), Arc::new(|| true))
            .await?;
        let identity = catalog.records().next().ok_or("fixture.identity")?.id();
        let mut key = PrivateKey::new(
            Ed25519Keypair::from_seed(&[19; 32]).into(),
            "fixed-native-key",
        )?;
        if encrypted {
            key = key.encrypt_with(
                Cipher::Aes256Ctr,
                Kdf::Bcrypt {
                    salt: vec![7; 16],
                    rounds: 1,
                },
                19,
                PASS,
            )?;
        }
        let pem = SecretBytes::from_owned(key.to_openssh(LineEnding::LF)?.as_bytes().to_vec())?;
        Ok(Self {
            _temp: temp,
            registry,
            documents,
            window,
            page,
            reader,
            owner,
            bank,
            directory,
            store,
            identity,
            pem,
            encrypted,
        })
    }
    async fn ticket(&self) -> std::result::Result<Uuid, &'static str> {
        self.reader
            .key_prepare(
                self.registry.clone(),
                self.page.clone(),
                self.directory.clone(),
                self.owner.clone(),
                self.bank.clone(),
                self.identity,
            )
            .await
    }
    fn key(&self) -> std::result::Result<SecretBytes, pulse_store::vault_crypto::CryptoError> {
        SecretBytes::from_owned(self.pem.expose().to_vec())
    }
    fn pass(
        &self,
    ) -> std::result::Result<Option<SecretBytes>, pulse_store::vault_crypto::CryptoError> {
        self.encrypted
            .then(|| SecretBytes::from_owned(PASS.to_vec()))
            .transpose()
    }
    async fn submit(&self, permit: PrivateKeyWritePermit, pass: Option<SecretBytes>) -> Result<()> {
        self.reader
            .key_submit(
                self.registry.clone(),
                self.page.clone(),
                self.directory.clone(),
                permit,
                self.key().map_err(|_| "fixture.key")?,
                pass,
            )
            .await
    }
    async fn exact(&self) -> TestResult {
        let source = self
            .store
            .load_key_identity_vault_source(Arc::new(|| true))
            .await?;
        let row = source
            .identities()
            .records()
            .find(|r| r.id() == self.identity)
            .ok_or("fixture.identity")?;
        let identity = row.identity();
        if !identity.private_key().has_value()
            || identity.passphrase().is_some_and(|p| !p.has_value())
        {
            return Err("fixture.presence".into());
        }
        let vault = source.vault();
        let scope = pulse_store::vault_lifecycle::VaultKeyLifecycle::new(
            vault.vault_id().ok_or("fixture.vault")?,
        )?;
        let request = pulse_store::vault_unlock::begin_validation_unlock(
            &scope,
            PasswordInput::new(MASTER.to_vec())?,
            vault.envelope().ok_or("fixture.envelope")?.clone(),
        )?;
        let mut ready = request.ready().await?;
        ready.confirm(&scope)?;
        let epoch = scope.available_epoch()?;
        for (reference, role, expected) in [
            (
                Some(identity.private_key()),
                "ssh_private_key",
                self.pem.expose(),
            ),
            (identity.passphrase(), "ssh_key_passphrase", PASS),
        ] {
            if let Some(reference) = reference {
                let record = vault
                    .credentials()
                    .iter()
                    .find(|r| r.id() == reference.id())
                    .ok_or("fixture.credential")?;
                let bytes =
                    record.open_for_adapter_in_epoch(&scope, &epoch, reference.id(), role)?;
                if bytes.expose() != expected {
                    return Err("fixture.private_bytes".into());
                }
            }
        }
        scope.close()?;
        Ok(())
    }
    async fn finish(self) -> TestResult {
        self.bank.close()?;
        self.owner.close()?;
        self.reader.close();
        self.reader.shutdown().await?;
        self.registry.shutdown()?;
        Ok(())
    }
}
async fn run<F>(encrypted: bool, check: F) -> TestResult
where
    F: for<'a> AsyncFnOnce(&'a Fixture) -> TestResult,
{
    let _serial = super::super::VAULT_TEST_SERIAL.lock().await;
    let fixture = Fixture::new(encrypted).await?;
    let result = check(&fixture).await;
    fixture.finish().await?;
    result
}
#[tokio::test]
async fn native_key_write_unencrypted_exact_ciphertext() -> TestResult {
    run(false, async |f| {
        let id = f.ticket().await?;
        f.submit(f.bank.take(&f.page, id)?, None).await?;
        if f.reader.credit.available_permits() != 1 {
            return Err("fixture.credit".into());
        }
        f.exact().await
    })
    .await
}
#[tokio::test]
async fn native_key_write_encrypted_wrong_passphrase_does_not_write() -> TestResult {
    run(true, async |f| {
        let before = f
            .store
            .load_vault_ciphertext(Arc::new(|| true))
            .await?
            .revision();
        let id = f.ticket().await?;
        if f.submit(
            f.bank.take(&f.page, id)?,
            Some(SecretBytes::from_owned(b"fixed-wrong-passphrase".to_vec())?),
        )
        .await
        .is_ok()
        {
            return Err("fixture.expected_refusal".into());
        }
        if f.store
            .load_vault_ciphertext(Arc::new(|| true))
            .await?
            .revision()
            != before
        {
            return Err("fixture.unintended_write".into());
        }
        let id = f.ticket().await?;
        f.submit(f.bank.take(&f.page, id)?, f.pass()?).await?;
        f.exact().await
    })
    .await
}
#[tokio::test]
async fn native_key_write_old_page_and_wrong_directory_denied() -> TestResult {
    run(false, async |f| {
        let id = f.ticket().await?;
        let permit = f.bank.take(&f.page, id)?;
        if f.reader
            .key_submit(
                f.registry.clone(),
                f.page.clone(),
                f.directory.join("wrong"),
                permit,
                f.key()?,
                None,
            )
            .await
            .is_ok()
        {
            return Err("fixture.wrong_directory".into());
        }
        let id = f.ticket().await?;
        let permit = f.bank.take(&f.page, id)?;
        f.documents.started(f.window)?;
        f.owner.retire_page()?;
        f.bank.retire()?;
        if f.submit(permit, None).await.is_ok() || f.ticket().await.is_ok() {
            return Err("fixture.retired_page".into());
        }
        Ok(())
    })
    .await
}
#[tokio::test]
async fn native_key_write_old_epoch_and_external_source_denied() -> TestResult {
    run(false, async |f| {
        let id = f.ticket().await?;
        let permit = f.bank.take(&f.page, id)?;
        f.owner.lock()?;
        if f.submit(permit, None).await.is_ok() {
            return Err("fixture.locked".into());
        }
        f.reader
            .vault(
                f.registry.clone(),
                f.page.clone(),
                f.directory.clone(),
                f.owner.clone(),
                Intent::Unlock(PasswordInput::new(MASTER.to_vec())?),
            )
            .await?;
        let id = f.ticket().await?;
        let permit = f.bank.take(&f.page, id)?;
        let snapshot = f.store.load_ssh_key_identities(Arc::new(|| true)).await?;
        let old = snapshot
            .records()
            .next()
            .ok_or("fixture.identity")?
            .identity();
        let next = SshPrivateKeyIdentity::new(
            "fixed-external-change",
            old.username(),
            old.private_key(),
            old.passphrase(),
        )?;
        f.store
            .edit_ssh_key_identities(
                snapshot,
                KeyIdentityEdit::Update(f.identity, next),
                Arc::new(|| true),
            )
            .await?;
        if f.submit(permit, None).await.is_ok() {
            return Err("fixture.changed_source".into());
        }
        Ok(())
    })
    .await
}
#[tokio::test]
async fn native_key_write_shared_cpu_busy_and_ticket_single_use() -> TestResult {
    run(false, async |f| {
        let cpu = crate::ssh_private_key_decode::DECODE_BUDGET
            .clone()
            .acquire_owned()
            .await?;
        let id = f.ticket().await?;
        let permit = f.bank.take(&f.page, id)?;
        if f.bank.take(&f.page, id).is_ok() {
            return Err("fixture.duplicate_ticket".into());
        }
        if f.submit(permit, None).await.is_ok() {
            return Err("fixture.shared_cpu_busy".into());
        }
        drop(cpu);
        if f.reader.credit.available_permits() != 1 {
            return Err("fixture.credit".into());
        }
        let id = f.ticket().await?;
        f.submit(f.bank.take(&f.page, id)?, None).await?;
        f.exact().await
    })
    .await
}

struct Release(Option<std::sync::mpsc::Sender<()>>);
impl Release {
    fn now(&mut self) {
        if let Some(sender) = self.0.take() {
            let _sent = sender.send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.now();
    }
}
#[tokio::test]
async fn native_key_write_cancel_active_parser_retains_cpu_and_native_credit() -> TestResult {
    run(false, async |f| {
        let id = f.ticket().await?;
        let permit = f.bank.take(&f.page, id)?;
        let cancel = permit.cancellation();
        let cpu = crate::ssh_private_key_decode::DECODE_BUDGET
            .clone()
            .acquire_owned()
            .await?;
        let (entered, received) = tokio::sync::oneshot::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let mut release = Release(Some(release));
        let work = spawn_ingress_validation(permit, f.key()?, None, cpu, move |key, pass| {
            let _entered = entered.send(());
            resume
                .recv_timeout(std::time::Duration::from_secs(3))
                .map_err(|_| pulse_app::vault_owner::VaultOwnerError::Failed)?;
            decode_ingress(key, pass)
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), received).await??;
        cancel.cancel();
        work.abort(); // An already active blocking closure must still own its resources.
        let held = crate::ssh_private_key_decode::DECODE_BUDGET.available_permits() == 0
            && f.reader.credit.available_permits() == 0
            && f.reader.reserve().is_err();
        release.now();
        let result = work.await?;
        if !held || result.is_ok() {
            return Err("fixture.early_release_or_late_permission".into());
        }
        if crate::ssh_private_key_decode::DECODE_BUDGET.available_permits() != 1
            || f.reader.credit.available_permits() != 1
        {
            return Err("fixture.exit_credit".into());
        }
        if !f
            .store
            .load_vault_ciphertext(Arc::new(|| true))
            .await?
            .credentials()
            .is_empty()
        {
            return Err("fixture.unintended_write".into());
        }
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "fixed private ciphertext fixture, explicitly invoked by the real-window parent"]
async fn private_key_window_fixture() -> TestResult {
    let root = PathBuf::from(std::env::var("PULSE_KEY_IMPORT_FIXTURE_ROOT")?);
    if root.parent() != Some(std::path::Path::new("/data/temp"))
        || !root
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("workspace-key-import."))
        || root.symlink_metadata()?.file_type().is_symlink()
    {
        return Err("fixture.path".into());
    }
    let directory = root.join("data/dev.pulseterm.workspace-preview");
    match std::env::var("PULSE_KEY_IMPORT_FIXTURE_MODE")?.as_str() {
        "seed" => {
            let _serial = super::super::VAULT_TEST_SERIAL.lock().await;
            let fixture = Fixture::new_at(true, Some(directory)).await?;
            let result = std::fs::write(root.join("fixed-key.pem"), fixture.pem.expose());
            fixture.finish().await?;
            result?;
            println!("workspace.key_import_fixed_ciphertext_and_metadata_fixture_confirmed");
        }
        "verify" => {
            let expected = SecretBytes::from_owned(std::fs::read(root.join("fixed-key.pem"))?)?;
            let (store, worker) = StateStore::open(directory.join("state.db")).await?;
            let result: TestResult = async {
                let source = store
                    .load_key_identity_vault_source(Arc::new(|| true))
                    .await?;
                let identity = source
                    .identities()
                    .records()
                    .next()
                    .ok_or("fixture.identity")?
                    .identity();
                if !identity.private_key().has_value()
                    || !identity.passphrase().is_some_and(|p| p.has_value())
                {
                    return Err("fixture.presence".into());
                }
                let vault = source.vault();
                let scope = pulse_store::vault_lifecycle::VaultKeyLifecycle::new(
                    vault.vault_id().ok_or("fixture.vault")?,
                )?;
                let request = pulse_store::vault_unlock::begin_validation_unlock(
                    &scope,
                    PasswordInput::new(MASTER.to_vec())?,
                    vault.envelope().ok_or("fixture.envelope")?.clone(),
                )?;
                let mut ready = request.ready().await?;
                ready.confirm(&scope)?;
                let epoch = scope.available_epoch()?;
                for (reference, role, expected) in [
                    (
                        Some(identity.private_key()),
                        "ssh_private_key",
                        expected.expose(),
                    ),
                    (identity.passphrase(), "ssh_key_passphrase", PASS),
                ] {
                    if let Some(reference) = reference {
                        let record = vault
                            .credentials()
                            .iter()
                            .find(|r| r.id() == reference.id())
                            .ok_or("fixture.credential")?;
                        let opened = record.open_for_adapter_in_epoch(
                            &scope,
                            &epoch,
                            reference.id(),
                            role,
                        )?;
                        if opened.expose() != expected {
                            return Err("fixture.private_bytes".into());
                        }
                    }
                }
                scope.close()?;
                Ok(())
            }
            .await;
            drop(store);
            worker.join().await?;
            result?;
            println!("workspace.key_import_saved_private_bytes_independent_process_confirmed");
        }
        _ => return Err("fixture.mode".into()),
    }
    Ok(())
}
