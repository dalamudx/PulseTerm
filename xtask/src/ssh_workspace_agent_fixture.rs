//! Fixed bounded single-selected-key Agent, confined to the owned real-window fixture directory.
use anyhow::{Context, Result, ensure};
use russh::keys::signature::Signer;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub(super) struct Agent {
    task: tokio::task::JoinHandle<()>,
    path: PathBuf,
}
impl Drop for Agent {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}
fn field(out: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    out.extend_from_slice(&u32::try_from(value.len())?.to_be_bytes());
    out.extend_from_slice(value);
    Ok(())
}
fn take<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8]> {
    let n = u32::from_be_bytes(bytes.get(..4).context("fixture.agent_field")?.try_into()?) as usize;
    *bytes = bytes.get(4..).context("fixture.agent_field")?;
    let value = bytes.get(..n).context("fixture.agent_field")?;
    *bytes = bytes.get(n..).context("fixture.agent_field")?;
    Ok(value)
}
async fn serve(mut stream: tokio::net::UnixStream) -> Result<()> {
    let selected = russh::keys::PrivateKey::new(
        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[3; 32]).into(),
        "",
    )?;
    let other = russh::keys::PrivateKey::new(
        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[8; 32]).into(),
        "",
    )?;
    let selected_blob = selected.public_key().to_bytes()?;
    let mut used = false;
    loop {
        let n = match stream.read_u32().await {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        ensure!((1..=131072).contains(&n), "fixture.agent_frame_budget");
        let mut request = vec![0; n as usize];
        stream.read_exact(&mut request).await?;
        let mut response = Vec::new();
        match request.first() {
            Some(11) => {
                ensure!(request.len() == 1, "fixture.agent_list_shape");
                response.push(12);
                response.extend_from_slice(&2u32.to_be_bytes());
                for key in [&other, &selected] {
                    field(&mut response, &key.public_key().to_bytes()?)?;
                    field(&mut response, b"fixed public metadata")?;
                }
                eprintln!("fixture.agent_public_metadata_read");
            }
            Some(13) => {
                ensure!(!used, "fixture.agent_duplicate_sign");
                let mut body = request.get(1..).context("fixture.agent_sign")?;
                ensure!(take(&mut body)? == selected_blob, "fixture.agent_wrong_key");
                let data = take(&mut body)?;
                ensure!(body == 0u32.to_be_bytes(), "fixture.agent_flags");
                let signature = selected.try_sign(data)?;
                response.push(14);
                field(&mut response, &Vec::<u8>::try_from(signature)?)?;
                used = true;
                eprintln!("fixture.agent_selected_signed_once");
            }
            _ => {
                response.push(5);
            }
        }
        stream.write_u32(u32::try_from(response.len())?).await?;
        stream.write_all(&response).await?;
        stream.flush().await?;
    }
}
pub(super) fn start(path: PathBuf) -> Result<Agent> {
    ensure!(
        std::env::var_os("SSH_AUTH_SOCK").as_deref() == Some(path.as_os_str()),
        "fixture.agent_socket_owned_path"
    );
    ensure!(!path.exists(), "fixture.agent_existing_socket_refused");
    let listener = tokio::net::UnixListener::bind(&path)?;
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                if serve(stream).await.is_err() {
                    eprintln!("fixture.agent_connection_failed");
                }
            });
        }
    });
    eprintln!("fixture.agent_listening");
    Ok(Agent { task, path })
}

/// Preexisting public metadata for cold saved-ID acceptance; saving grants no signing authority.
pub(super) async fn seed_saved_identity(
    store: &pulse_store::StateStore,
) -> Result<(uuid::Uuid, russh::keys::ssh_key::PublicKey)> {
    let selected = russh::keys::PrivateKey::new(
        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[3; 32]).into(),
        "",
    )?;
    let public = selected.public_key().clone();
    let snapshot = store
        .load_ssh_agent_identities(std::sync::Arc::new(|| true))
        .await?;
    let saved = store
        .edit_ssh_agent_identity(
            snapshot,
            pulse_store::ssh_agent_identities::AgentIdentityEdit::Create(
                pulse_core::ssh_agent_identity::SshAgentIdentity::new(
                    "GUI_COLD_AGENT",
                    "user",
                    "ssh-ed25519",
                    &public
                        .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
                        .to_string(),
                )?,
            ),
            std::sync::Arc::new(|| true),
        )
        .await?;
    let id = saved
        .records()
        .next()
        .context("fixture.cold_agent_identity")?
        .id();
    Ok((id, public))
}
