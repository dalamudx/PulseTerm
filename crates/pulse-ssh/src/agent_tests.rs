//! Real Unix framing and selected-key SSH authentication, never an Agent echo probe.
use super::*;
use russh::{
    Signer as AuthSigner,
    keys::{PrivateKey, agent::AgentIdentity, signature::Signer as CryptoSigner},
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
fn allow() -> Authorize {
    Arc::new(|| true)
}
fn key(seed: u8) -> PrivateKey {
    PrivateKey::from(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(
        &[seed; 32],
    ))
}
fn field(out: &mut Vec<u8>, data: &[u8]) -> TestResult {
    out.extend_from_slice(&u32::try_from(data.len())?.to_be_bytes());
    out.extend_from_slice(data);
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
}
fn identities(keys: &[PrivateKey]) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let mut result = vec![12];
    result.extend_from_slice(&u32::try_from(keys.len())?.to_be_bytes());
    for key in keys {
        field(&mut result, &key.public_key().to_bytes()?)?;
        field(&mut result, b"untrusted\ncomment")?;
    }
    Ok(result)
}
async fn read(
    stream: &mut tokio::net::UnixStream,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let len = stream.read_u32().await?;
    assert!(len <= 262144);
    let mut out = vec![0; usize::try_from(len)?];
    stream.read_exact(&mut out).await?;
    Ok(out)
}
async fn send(stream: &mut tokio::net::UnixStream, out: &[u8]) -> TestResult {
    stream
        .write_all(&u32::try_from(out.len())?.to_be_bytes())
        .await?;
    stream.write_all(out).await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
}
fn take_field<'a>(
    bytes: &mut &'a [u8],
) -> Result<&'a [u8], Box<dyn std::error::Error + Send + Sync>> {
    let length = u32::from_be_bytes(bytes.get(..4).ok_or("test.field.header")?.try_into()?);
    *bytes = bytes.get(4..).ok_or("test.field.header")?;
    let size = usize::try_from(length)?;
    let result = bytes.get(..size).ok_or("test.field.body")?;
    *bytes = bytes.get(size..).ok_or("test.field.body")?;
    Ok(result)
}
#[tokio::test]
async fn bounded_discovery_rejects_overflow_duplicate_trailing_and_revoked_frames_before_publish()
-> TestResult {
    let first = key(11);
    let data = identities(std::slice::from_ref(&first))?;
    let decoded = wire::decode_keys(&data)?;
    assert_eq!(decoded.len(), 1);
    assert_eq!(
        decoded.first().ok_or("test.key")?.fingerprint(),
        first
            .public_key()
            .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
            .to_string()
    );
    for malformed in [
        vec![12, 255, 255, 255, 255],
        identities(&[first.clone(), first])?,
        {
            let mut x = data.clone();
            x.push(0);
            x
        },
    ] {
        assert!(wire::decode_keys(&malformed).is_err());
    }
    for length in [0_u32, 262145, u32::MAX] {
        let (client, mut server) = tokio::net::UnixStream::pair()?;
        let task = tokio::spawn(async move {
            assert_eq!(read(&mut server).await?, [11]);
            server.write_all(&length.to_be_bytes()).await?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });
        assert!(matches!(
            AgentConnection::read_stream(client, allow()).await,
            Err(AgentError::Protocol)
        ));
        task.await??;
    }
    let (client, mut server) = tokio::net::UnixStream::pair()?;
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    let grant: Authorize = Arc::new(move || flag.load(Ordering::Acquire));
    let task = tokio::spawn(async move {
        assert_eq!(read(&mut server).await?, [11]);
        alive.store(false, Ordering::Release);
        send(&mut server, &data).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    assert!(matches!(
        AgentConnection::read_stream(client, grant).await,
        Err(AgentError::Denied)
    ));
    task.await??;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
}
#[tokio::test]
async fn private_signer_refuses_foreign_key_replay_refusal_and_revocation_with_no_fallback()
-> TestResult {
    let first = key(12);
    let other = key(13);
    let (client, mut server) = tokio::net::UnixStream::pair()?;
    let mut signer = wire::SelectedSigner {
        wire: wire::Wire::new(client),
        key: first.public_key().clone(),
        current: allow(),
        used: false,
    };
    assert!(matches!(
        signer
            .auth_sign(
                &AgentIdentity::from(other.public_key().clone()),
                None,
                vec![1]
            )
            .await,
        Err(AgentError::Denied)
    ));
    let mut probe = [0; 1];
    assert_eq!(
        server
            .try_read(&mut probe)
            .err()
            .ok_or("test.must_not_read")?
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    let task = tokio::spawn(async move {
        let request = read(&mut server).await?;
        assert_eq!(request.first(), Some(&13));
        send(&mut server, &[5]).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    let selected = AgentIdentity::from(first.public_key().clone());
    assert!(matches!(
        signer.auth_sign(&selected, None, vec![1]).await,
        Err(AgentError::Refused)
    ));
    assert!(matches!(
        signer.auth_sign(&selected, None, vec![1]).await,
        Err(AgentError::Denied)
    ));
    task.await??;
    let (client, mut server) = tokio::net::UnixStream::pair()?;
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    let grant: Authorize = Arc::new(move || flag.load(Ordering::Acquire));
    let mut signer = wire::SelectedSigner {
        wire: wire::Wire::new(client),
        key: first.public_key().clone(),
        current: grant,
        used: false,
    };
    let task = tokio::spawn(async move {
        let request = read(&mut server).await?;
        let mut payload = request.get(1..).ok_or("test.sign")?;
        assert_eq!(take_field(&mut payload)?, first.public_key().to_bytes()?);
        let to_sign = take_field(&mut payload)?;
        let signature = first.try_sign(to_sign)?;
        let mut response = vec![14];
        field(&mut response, &Vec::<u8>::try_from(signature)?)?;
        alive.store(false, Ordering::Release);
        send(&mut server, &response).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    assert!(matches!(
        signer.auth_sign(&selected, None, vec![1, 2, 3]).await,
        Err(AgentError::Denied)
    ));
    task.await??;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
}
struct Server {
    selected: PublicKey,
    authenticated: Arc<AtomicUsize>,
}
impl russh::server::Handler for Server {
    type Error = russh::Error;
    async fn auth_publickey_offered(
        &mut self,
        _: &str,
        _: &PublicKey,
    ) -> Result<russh::server::Auth, Self::Error> {
        Ok(russh::server::Auth::Accept)
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> Result<russh::server::Auth, Self::Error> {
        if user == "agent-user" && key == &self.selected {
            self.authenticated.fetch_add(1, Ordering::AcqRel);
            Ok(russh::server::Auth::Accept)
        } else {
            Ok(russh::server::Auth::reject())
        }
    }
}
#[tokio::test]
async fn real_ssh_authenticates_only_saved_agent_key_and_closes_agent_and_native_transport()
-> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(12), async {
        let host = key(20);
        let selected = key(21);
        let other = key(22);
        let authenticated = Arc::new(AtomicUsize::new(0));
        let signs = Arc::new(AtomicUsize::new(0));
        let tcp = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = tcp.local_addr()?;
        let selected_public = selected.public_key().clone();
        let count = authenticated.clone();
        let public = selected_public.clone();
        let server_key = host.clone();
        let peer = tokio::spawn(async move {
            let (stream, _) = tcp.accept().await?;
            let config = Arc::new(russh::server::Config {
                keys: vec![server_key],
                auth_rejection_time: std::time::Duration::ZERO,
                auth_rejection_time_initial: Some(std::time::Duration::ZERO),
                ..Default::default()
            });
            let running = russh::server::run_stream(
                config,
                stream,
                Server {
                    selected: public,
                    authenticated: count,
                },
            )
            .await?;
            // EOF after explicit client teardown proves completion, not a remote process result.
            let _remote_end = running.await;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });
        let (client, mut server) = tokio::net::UnixStream::pair()?;
        let sign_count = signs.clone();
        let agent = tokio::spawn(async move {
            assert_eq!(read(&mut server).await?, [11]);
            send(&mut server, &identities(&[other, selected.clone()])?).await?;
            let request = read(&mut server).await?;
            assert_eq!(request.first(), Some(&13));
            let mut payload = request.get(1..).ok_or("test.agent_request")?;
            assert_eq!(take_field(&mut payload)?, selected.public_key().to_bytes()?);
            let data = take_field(&mut payload)?;
            assert_eq!(payload, &0u32.to_be_bytes());
            let signature = selected.try_sign(data)?;
            sign_count.fetch_add(1, Ordering::AcqRel);
            let mut response = vec![14];
            field(&mut response, &Vec::<u8>::try_from(signature)?)?;
            send(&mut server, &response).await?;
            assert!(server.read_u32().await.is_err());
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });
        let agent_connection = AgentConnection::read_stream(client, allow()).await?;
        assert_eq!(agent_connection.keys().len(), 2);
        let target = crate::HostTarget::new("127.0.0.1", address.port())?;
        let stream = tokio::net::TcpStream::connect(address).await?;
        let mut transport = crate::peer::PinnedTransport::connect_stream(
            target.clone(),
            Some(vec![crate::HostPin::new(target, host.public_key().clone())]),
            None,
            Arc::new(russh::client::Config::default()),
            stream,
        )
        .await?;
        let actual_peer = transport.peer()?;
        let evidence = transport.peer()?;
        let signing: Authorize = Arc::new(move || evidence.is_current());
        let saved = SshAgentIdentity::new(
            "Chosen Agent key",
            "agent-user",
            "ssh-ed25519",
            &selected_public
                .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
                .to_string(),
        )?;
        assert!(
            agent_connection
                .authenticate(transport.client(&actual_peer)?, &saved, signing)
                .await?
        );
        eprintln!("fixture.agent_selected_key_authenticated");
        assert_eq!(signs.load(Ordering::Acquire), 1);
        assert_eq!(authenticated.load(Ordering::Acquire), 1);
        agent.await??;
        transport.close_and_join().await?;
        peer.await??;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
}
