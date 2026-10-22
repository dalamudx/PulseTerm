//! Native Windows named-pipe fixtures only; not the user's OpenSSH service or product GUI.
use super::*;
use russh::{
    Signer as AuthSigner,
    keys::{PrivateKey, agent::AgentIdentity, signature::Signer as CryptoSigner},
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions},
};
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn pair() -> Result<(AgentStream, NamedPipeServer), Box<dyn std::error::Error + Send + Sync>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let name = format!(
        r"\\.\pipe\pulseterm-agent-fixture-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)?;
    let client = ClientOptions::new().open(&name)?;
    Ok((client, server))
}
fn field(output: &mut Vec<u8>, value: &[u8]) -> TestResult {
    output.extend_from_slice(&u32::try_from(value.len())?.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}
async fn read(
    server: &mut NamedPipeServer,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let length = usize::try_from(server.read_u32().await?)?;
    assert!((1..=262144).contains(&length));
    let mut body = vec![0; length];
    server.read_exact(&mut body).await?;
    Ok(body)
}
async fn send(server: &mut NamedPipeServer, body: &[u8]) -> TestResult {
    server
        .write_all(&u32::try_from(body.len())?.to_be_bytes())
        .await?;
    server.write_all(body).await?;
    Ok(())
}
fn key(seed: u8) -> PrivateKey {
    PrivateKey::from(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(
        &[seed; 32],
    ))
}

#[tokio::test]
async fn named_pipe_discovery_keeps_exact_fingerprint_and_closes_connection() -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let selected = key(21);
        let expected = selected
            .public_key()
            .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
            .to_string();
        let mut body = vec![12, 0, 0, 0, 1];
        field(&mut body, &selected.public_key().to_bytes()?)?;
        field(&mut body, b"untrusted comment must not appear in metadata")?;
        let (client, mut server) = pair()?;
        let fixture = async {
            server.connect().await?;
            assert_eq!(read(&mut server).await?, [11]);
            send(&mut server, &body).await?;
            let mut byte = [0];
            match server.read(&mut byte).await {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
                result => return Err(format!("unexpected pipe close: {result:?}").into()),
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let discover = async {
            let connection = AgentConnection::read_stream(client, Arc::new(|| true)).await?;
            assert_eq!(connection.keys().len(), 1);
            assert_eq!(connection.keys()[0].fingerprint(), expected);
            drop(connection);
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (client, server) = tokio::join!(discover, fixture);
        client?;
        server?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn named_pipe_oversize_and_revocation_never_publish_keys() -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for length in [0, 262145, u32::MAX] {
            let (client, mut server) = pair()?;
            let fixture = async {
                server.connect().await?;
                assert_eq!(read(&mut server).await?, [11]);
                server.write_all(&length.to_be_bytes()).await?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
            };
            let (result, fixture) = tokio::join!(
                AgentConnection::read_stream(client, Arc::new(|| true)),
                fixture
            );
            assert!(matches!(result, Err(AgentError::Protocol)));
            fixture?;
        }
        let (client, mut server) = pair()?;
        let alive = Arc::new(AtomicBool::new(true));
        let flag = alive.clone();
        let current: Authorize = Arc::new(move || flag.load(Ordering::Acquire));
        let fixture = async {
            server.connect().await?;
            assert_eq!(read(&mut server).await?, [11]);
            alive.store(false, Ordering::Release);
            send(&mut server, &[12, 0, 0, 0, 0]).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (result, fixture) =
            tokio::join!(AgentConnection::read_stream(client, current), fixture);
        assert!(matches!(result, Err(AgentError::Denied)));
        fixture?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn named_pipe_signer_validates_selected_signature_once_without_foreign_key_fallback()
-> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let selected = key(22);
        let public = selected.public_key().clone();
        let foreign = key(23);
        let data = vec![1, 2, 3, 4, 5];
        let (client, mut server) = pair()?;
        let fixture = async {
            server.connect().await?;
            let mut expected = vec![13];
            field(&mut expected, &selected.public_key().to_bytes()?)?;
            field(&mut expected, &data)?;
            expected.extend_from_slice(&0_u32.to_be_bytes());
            assert_eq!(read(&mut server).await?, expected);
            let signature = selected.try_sign(&data)?;
            let mut response = vec![14];
            field(&mut response, &Vec::<u8>::try_from(signature)?)?;
            send(&mut server, &response).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let client = async {
            let mut signer = wire::SelectedSigner {
                wire: wire::Wire::new(client),
                key: public.clone(),
                current: Arc::new(|| true),
                used: false,
            };
            assert!(matches!(
                signer
                    .auth_sign(
                        &AgentIdentity::from(foreign.public_key().clone()),
                        None,
                        data.clone()
                    )
                    .await,
                Err(AgentError::Denied)
            ));
            let signed = signer
                .auth_sign(&AgentIdentity::from(public.clone()), None, data.clone())
                .await?;
            assert!(signed.starts_with(&data));
            assert!(matches!(
                signer
                    .auth_sign(&AgentIdentity::from(public), None, data.clone())
                    .await,
                Err(AgentError::Denied)
            ));
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (result, fixture) = tokio::join!(client, fixture);
        result?;
        fixture?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    Ok(())
}
