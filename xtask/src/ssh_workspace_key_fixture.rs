//! Fixed encrypted Ed25519 seed, confined to the owned real-window acceptance database.
use anyhow::{Context, Result};
use pulse_app::{registry::PageLaunchScope, vault_owner::PageVaultOwner};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_key_identity::SshPrivateKeyIdentity,
};
use pulse_store::{StateStore, ssh_key_identities::KeyIdentityEdit, vault_crypto::SecretBytes};
use std::sync::Arc;
use uuid::Uuid;

pub(super) async fn seed(
    owner: &PageVaultOwner,
    page: Arc<PageLaunchScope>,
    store: &StateStore,
    vault: Uuid,
) -> Result<(Uuid, russh::keys::PublicKey)> {
    let client = russh::keys::PrivateKey::new(
        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[3; 32]).into(),
        "",
    )?;
    let public = client.public_key().clone();
    let encrypted = client.encrypt(&mut rand::rng(), b"fixed-key-passphrase")?;
    let pem = encrypted.to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
    let rows = store
        .edit_ssh_key_identities(
            store.load_ssh_key_identities(Arc::new(|| true)).await?,
            KeyIdentityEdit::Create(SshPrivateKeyIdentity::new(
                "GUI_KEY_IDENTITY",
                "user",
                SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPrivateKey, false)?,
                Some(SecretRef::new(
                    vault,
                    Uuid::now_v7(),
                    SecretKind::SshKeyPassphrase,
                    false,
                )?),
            )?),
            Arc::new(|| true),
        )
        .await?;
    let id = rows
        .records()
        .next()
        .context("fixture.key_identity_missing")?
        .id();
    owner
        .begin(page)?
        .prepare_private_key_identity_write(
            store
                .load_key_identity_vault_source(Arc::new(|| true))
                .await?,
            id,
        )?
        .write(
            store,
            SecretBytes::from_owned(pem.as_bytes().to_vec())?,
            Some(SecretBytes::from_owned(b"fixed-key-passphrase".to_vec())?),
            Arc::new(|| true),
        )
        .await?;
    Ok((id, public))
}
