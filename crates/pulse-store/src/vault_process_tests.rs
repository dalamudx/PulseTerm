//! Isolated test passwords/credentials only; no system Vault, keyring or user directory.
use crate::{
    StateStore, StoreError,
    vault_database::VaultEdit,
    vault_record::CredentialRecord,
    vault_wrap::{create_validation_vault, recover_validation_dek},
};
use std::{
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn allowed() -> Arc<dyn Fn() -> bool + Send + Sync> {
    Arc::new(|| true)
}
#[test]
fn separate_processes_recover_same_key_and_preserve_deleted_credential_identity() -> TestResult {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = d.path().join("state.sqlite3");
    let id = Uuid::now_v7();
    let credential = Uuid::now_v7();
    let exe = std::env::current_exe()?;
    for mode in ["write", "recover_remove", "deleted"] {
        let mut child = Command::new(&exe)
            .args([
                "--exact",
                "vault_process_tests::isolated_vault_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("PULSE_VAULT_FIXTURE_DB", &path)
            .env("PULSE_VAULT_FIXTURE_ID", id.to_string())
            .env("PULSE_VAULT_FIXTURE_CREDENTIAL", credential.to_string())
            .env("PULSE_VAULT_FIXTURE_MODE", mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err("fixture.child_deadline".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let result = child.wait_with_output()?;
        assert!(
            result.status.success(),
            "fixed Vault subprocess failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let marker = format!("fixture.vault.{mode}.confirmed");
        assert_eq!(
            String::from_utf8_lossy(&result.stdout)
                .matches(&marker)
                .count(),
            1
        );
    }
    Ok(())
}
#[test]
#[ignore = "explicit child fixture only, always spawned with isolated private database"]
fn isolated_vault_child() -> TestResult {
    let path = std::path::PathBuf::from(std::env::var("PULSE_VAULT_FIXTURE_DB")?);
    let id = Uuid::parse_str(&std::env::var("PULSE_VAULT_FIXTURE_ID")?)?;
    let credential = Uuid::parse_str(&std::env::var("PULSE_VAULT_FIXTURE_CREDENTIAL")?)?;
    let mode = std::env::var("PULSE_VAULT_FIXTURE_MODE")?;
    let password = zeroize::Zeroizing::new(b"fixed-child-test-password".to_vec());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (store, worker) = StateStore::open(path).await?;
        let snapshot = store.load_vault_ciphertext(allowed()).await?;
        match mode.as_str() {
            "write" => {
                assert_eq!(snapshot.revision(), 0);
                let (envelope, mut key) = create_validation_vault(&password, id)?;
                let record = CredentialRecord::seal(
                    id,
                    credential,
                    "ssh_password",
                    &key,
                    b"fixed-child-test-secret",
                )?;
                let saved = store
                    .edit_vault_ciphertext(snapshot, VaultEdit::Initialize(id, envelope), allowed())
                    .await?;
                let saved = store
                    .edit_vault_ciphertext(saved, VaultEdit::Create(record), allowed())
                    .await?;
                assert_eq!(saved.revision(), 2);
                key.lock();
            }
            "recover_remove" => {
                assert_eq!(snapshot.revision(), 2);
                assert_eq!(snapshot.vault_id(), Some(id));
                let envelope = snapshot.envelope().ok_or("fixture.missing_envelope")?;
                assert!(recover_validation_dek(b"wrong-fixed-password", id, envelope).is_err());
                let mut key = recover_validation_dek(&password, id, envelope)?.into_cipher();
                assert_eq!(
                    snapshot.credentials()[0]
                        .open(id, credential, "ssh_password", &key)?
                        .expose(),
                    b"fixed-child-test-secret"
                );
                let deleted = store
                    .edit_vault_ciphertext(snapshot, VaultEdit::Remove(credential), allowed())
                    .await?;
                assert_eq!(deleted.revision(), 3);
                let record =
                    CredentialRecord::seal(id, credential, "ssh_password", &key, b"replacement")?;
                assert!(matches!(
                    store
                        .edit_vault_ciphertext(deleted, VaultEdit::Create(record), allowed())
                        .await,
                    Err(StoreError::VaultConflict)
                ));
                key.lock();
            }
            "deleted" => {
                assert_eq!(snapshot.revision(), 3);
                assert!(snapshot.credentials().is_empty());
                assert_eq!(snapshot.vault_id(), Some(id));
                let mut key = recover_validation_dek(
                    &password,
                    id,
                    snapshot.envelope().ok_or("fixture.missing_envelope")?,
                )?
                .into_cipher();
                let record =
                    CredentialRecord::seal(id, credential, "ssh_password", &key, b"replacement")?;
                assert!(matches!(
                    store
                        .edit_vault_ciphertext(snapshot, VaultEdit::Create(record), allowed())
                        .await,
                    Err(StoreError::VaultConflict)
                ));
                key.lock();
            }
            _ => return Err("fixture.mode".into()),
        }
        drop(store);
        worker.join().await?;
        println!("fixture.vault.{mode}.confirmed");
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
