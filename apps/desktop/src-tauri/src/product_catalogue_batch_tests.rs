//! Fixed metadata-only fixture for one real-window batch flow, never user data or network.
use super::*;
#[test]
fn batch_wire_rejects_duplicate_empty_and_noncanonical_ids() {
    for ids in [
        vec![],
        vec![Uuid::now_v7().to_string(); 2],
        vec![Uuid::new_v4().to_string()],
        vec![Uuid::now_v7().to_string(); 257],
    ] {
        assert!(batch_ids(ids).is_err());
    }
    assert_eq!(
        batch_ids(vec![Uuid::now_v7().to_string(), Uuid::now_v7().to_string()]).map(|v| v.len()),
        Ok(2)
    );
}
#[tokio::test]
#[ignore = "explicit fixed private fixture used only by real-window parent"]
async fn catalogue_window_fixture() -> std::result::Result<(), Box<dyn std::error::Error>> {
    use pulse_core::{
        secret_ref::{SecretKind, SecretRef},
        ssh_identity::SshPasswordIdentity,
        ssh_key_identity::SshPrivateKeyIdentity,
    };
    use pulse_store::{
        ssh_identities::IdentityEdit, ssh_key_identities::KeyIdentityEdit,
        vault_database::VaultEdit, vault_wrap::ValidationWrappedDek,
    };
    let dir = std::path::PathBuf::from(std::env::var("PULSE_CATALOGUE_BATCH_DIRECTORY")?);
    if !dir.is_absolute()
        || !dir.starts_with("/data/temp")
        || dir.file_name().and_then(|s| s.to_str()) != Some("dev.pulseterm.workspace-preview")
        || !dir
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("catalogue-batch."))
    {
        return Err("fixture.path".into());
    }
    let dir = pulse_store::state_directory::prepare(&dir)?;
    let (store, worker) = pulse_store::StateStore::open(dir.join("state.db")).await?;
    let vault = Uuid::now_v7();
    let mut wrapped = vec![0; 110];
    wrapped[..6].copy_from_slice(b"PVDE\x01\x01");
    wrapped[6..22].copy_from_slice(vault.as_bytes());
    let allow = || Arc::new(|| true) as Arc<dyn Fn() -> bool + Send + Sync>;
    store
        .edit_vault_ciphertext(
            store.load_vault_ciphertext(allow()).await?,
            VaultEdit::Initialize(
                vault,
                ValidationWrappedDek::decode_ciphertext(&wrapped, vault)?,
            ),
            allow(),
        )
        .await?;
    for label in ["Batch one", "Batch two", "Keep three"] {
        store
            .edit_ssh_hosts(
                store.load_ssh_hosts().await?,
                HostEdit::Create(SshEndpoint::new(
                    label,
                    "fixture.invalid",
                    22,
                    "batch-user",
                )?),
                allow(),
            )
            .await?;
        store
            .edit_ssh_identities(
                store.load_ssh_identities(allow()).await?,
                IdentityEdit::Create(SshPasswordIdentity::new(
                    label,
                    "batch-user",
                    SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
                )?),
                allow(),
            )
            .await?;
        store
            .edit_ssh_key_identities(
                store.load_ssh_key_identities(allow()).await?,
                KeyIdentityEdit::Create(SshPrivateKeyIdentity::new(
                    label,
                    "batch-user",
                    SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPrivateKey, false)?,
                    Some(SecretRef::new(
                        vault,
                        Uuid::now_v7(),
                        SecretKind::SshKeyPassphrase,
                        false,
                    )?),
                )?),
                allow(),
            )
            .await?;
    }
    drop(store);
    worker.join().await?;
    println!("workspace.catalogue_batch_fixture_confirmed");
    Ok(())
}
