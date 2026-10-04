//! Two controlled SSH endpoints with a bounded, destination-pinned jump relay.
use anyhow::{Context, Result, ensure};
use russh::server::Server as _;
use russh::{
    Channel, ChannelId, ChannelMsg, Disconnect, client,
    keys::{Algorithm, PrivateKey, PublicKey, PublicKeyOrCertificate},
    server,
};
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Semaphore, oneshot},
};
const USER: &str = "jump-probe";
const PASS: &str = "synthetic-jump-password";
#[path = "ssh_pressure_probe.rs"]
mod pressure;
pub fn run_pressure() -> Result<()> {
    let root = super::root()?;
    let path = root.join("target/probes/ssh-pressure.json");
    std::fs::create_dir_all(root.join("target/probes"))?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let report = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(40), pressure::probe()).await
    })??;
    pressure::validate(&report)?;
    std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    println!("PASS SSH ACK pressure and shared-connection head-of-line limitation observed");
    Ok(())
}
#[derive(Default)]
struct Metrics {
    active: AtomicUsize,
    forward: AtomicUsize,
    forward_peak: AtomicUsize,
    denied: AtomicUsize,
    bytes: AtomicUsize,
    produced: AtomicUsize,
    producing: AtomicUsize,
    producer_done: AtomicBool,
}
struct Handler {
    metrics: Arc<Metrics>,
    target: Option<SocketAddr>,
    slots: Arc<Semaphore>,
    connection: bool,
    writers: std::collections::HashMap<ChannelId, russh::ChannelWriteHalf<server::Msg>>,
    producers: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Handler {
    fn drop(&mut self) {
        for task in &self.producers {
            task.abort();
        }
        if self.connection {
            self.metrics.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
impl server::Server for Handler {
    type Handler = Self;
    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Self {
        self.metrics.active.fetch_add(1, Ordering::SeqCst);
        Self {
            metrics: self.metrics.clone(),
            target: self.target,
            slots: self.slots.clone(),
            connection: true,
            writers: Default::default(),
            producers: vec![],
        }
    }
}
impl server::Handler for Handler {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        user: &str,
        pass: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        Ok(if user == USER && pass == PASS {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if self.target.is_none() && self.writers.len() < 4 {
            let id = channel.id();
            let (_read, write) = channel.split();
            self.writers.insert(id, write);
            reply.accept().await;
        }
        Ok(())
    }
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if self.target.is_some() || data != pressure::EXEC || !self.producers.is_empty() {
            session.channel_failure(channel)?;
            return Ok(());
        }
        let Some(writer) = self.writers.remove(&channel) else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        session.channel_success(channel)?;
        let metrics = self.metrics.clone();
        self.producers.push(tokio::spawn(async move {
            let _ = pressure::produce(writer, metrics).await;
        }));
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if self.target.is_some() {
            return Ok(());
        }
        if data.len() > 8192 {
            return Err(russh::Error::Disconnect);
        }
        self.metrics.bytes.fetch_add(data.len(), Ordering::SeqCst);
        session.data(channel, data.to_vec())?;
        Ok(())
    }
    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _origin: &str,
        _origin_port: u32,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        let Some(target) = allowed_destination(self.target, host, port) else {
            self.metrics.denied.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        };
        let Ok(permit) = self.slots.clone().try_acquire_owned() else {
            self.metrics.denied.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        };
        let mut socket =
            match tokio::time::timeout(Duration::from_secs(2), TcpStream::connect(target)).await {
                Ok(Ok(socket)) => socket,
                _ => return Ok(()),
            };
        reply.accept().await;
        let metrics = self.metrics.clone();
        let active = metrics.forward.fetch_add(1, Ordering::SeqCst) + 1;
        metrics.forward_peak.fetch_max(active, Ordering::SeqCst);
        let guard = ForwardGuard(metrics);
        tokio::spawn(async move {
            let _guard = guard;
            let _permit = permit;
            let mut stream = channel.into_stream();
            let _ = tokio::time::timeout(
                Duration::from_secs(6),
                tokio::io::copy_bidirectional(&mut stream, &mut socket),
            )
            .await;
        });
        Ok(())
    }
}
fn allowed_destination(target: Option<SocketAddr>, host: &str, port: u32) -> Option<SocketAddr> {
    target.filter(|a| a.ip().is_loopback() && host == "127.0.0.1" && port == u32::from(a.port()))
}

struct ForwardGuard(Arc<Metrics>);
impl Drop for ForwardGuard {
    fn drop(&mut self) {
        self.0.forward.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Endpoint {
    address: SocketAddr,
    key: PublicKey,
    metrics: Arc<Metrics>,
    stop: server::RunningServerHandle,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.shutdown("probe complete".into());
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
async fn endpoint(target: Option<SocketAddr>) -> Result<Endpoint> {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let expected = key.public_key().clone();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let metrics = Arc::new(Metrics::default());
    let mut handler = Handler {
        metrics: metrics.clone(),
        target,
        slots: Arc::new(Semaphore::new(1)),
        connection: false,
        writers: Default::default(),
        producers: vec![],
    };
    let cfg = Arc::new(server::Config {
        keys: vec![key],
        auth_rejection_time: Duration::from_millis(20),
        auth_rejection_time_initial: Some(Duration::ZERO),
        inactivity_timeout: Some(Duration::from_secs(5)),
        maximum_packet_size: 8192,
        ..Default::default()
    });
    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let running = handler.run_on_socket(cfg, &listener);
        let _ = tx.send(running.handle());
        running.await
    });
    Ok(Endpoint {
        address,
        key: expected,
        metrics,
        stop: rx.await?,
        task: Some(task),
    })
}
struct Pin {
    key: PublicKey,
    rejected: Arc<AtomicBool>,
}
impl client::Handler for Pin {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        presented: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let accepted =
            matches!(presented,PublicKeyOrCertificate::PublicKey{key,..}if key==&self.key);
        if !accepted {
            self.rejected.store(true, Ordering::SeqCst);
        }
        Ok(accepted)
    }
}
async fn hop(endpoint: &Endpoint) -> Result<client::Handle<super::ssh_probe::PinnedClient>> {
    let mut hop = super::ssh_probe::connect(
        endpoint.address,
        endpoint.key.clone(),
        Arc::new(AtomicBool::new(false)),
    )
    .await?;
    ensure!(
        hop.authenticate_password(USER, PASS).await?.success(),
        "jump auth denied"
    );
    Ok(hop)
}
async fn drained(endpoint: &Endpoint) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(3), async {
        while endpoint.metrics.active.load(Ordering::SeqCst) != 0
            || endpoint.metrics.forward.load(Ordering::SeqCst) != 0
            || endpoint.metrics.producing.load(Ordering::SeqCst) != 0
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("jump endpoint did not drain")?;
    Ok(())
}
async fn nested(
    hop: &mut client::Handle<super::ssh_probe::PinnedClient>,
    inner: &Endpoint,
    key: PublicKey,
    rejected: Arc<AtomicBool>,
) -> Result<client::Handle<Pin>> {
    let channel = hop
        .channel_open_direct_tcpip("127.0.0.1", u32::from(inner.address.port()), "127.0.0.1", 0)
        .await?;
    let cfg = Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(5)),
        maximum_packet_size: 8192,
        ..Default::default()
    });
    Ok(tokio::time::timeout(
        Duration::from_secs(3),
        client::connect_stream(cfg, channel.into_stream(), Pin { key, rejected }),
    )
    .await??)
}
async fn stop(endpoint: &mut Endpoint) -> Result<()> {
    endpoint.stop.shutdown("test complete".into());
    if let Some(task) = &mut endpoint.task {
        tokio::time::timeout(Duration::from_secs(3), task).await???;
    }
    endpoint.task = None;
    drained(endpoint).await
}
async fn routed_ssh(inner: &Endpoint) -> Result<serde_json::Value> {
    let (mut refused_proxies, refused_stream) =
        super::route_probe::ProxyChain::connect(inner.address).await?;
    let rejected_wrong = Arc::new(AtomicBool::new(false));
    let wrong = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?
        .public_key()
        .clone();
    let refused = tokio::time::timeout(
        Duration::from_secs(3),
        client::connect_stream(
            Arc::new(client::Config::default()),
            refused_stream,
            Pin {
                key: wrong,
                rejected: rejected_wrong.clone(),
            },
        ),
    )
    .await?;
    ensure!(
        refused.is_err() && rejected_wrong.load(Ordering::SeqCst),
        "proxy route bypassed SSH host key verification"
    );
    refused_proxies.finish().await?;
    drained(inner).await?;
    let (mut proxies, stream) = super::route_probe::ProxyChain::connect(inner.address).await?;
    let rejected = Arc::new(AtomicBool::new(false));
    let config = Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(5)),
        maximum_packet_size: 8192,
        ..Default::default()
    });
    let mut connection = tokio::time::timeout(
        Duration::from_secs(3),
        client::connect_stream(
            config,
            stream,
            Pin {
                key: inner.key.clone(),
                rejected: rejected.clone(),
            },
        ),
    )
    .await??;
    ensure!(
        !rejected.load(Ordering::SeqCst)
            && connection
                .authenticate_password(USER, PASS)
                .await?
                .success(),
        "routed SSH hostkey/authentication failed"
    );
    let mut channel = connection.channel_open_session().await?;
    let payload: Vec<_> = (0..512).map(|n| (n % 251) as u8).collect();
    channel.data(payload.as_slice()).await?;
    let mut received = vec![];
    tokio::time::timeout(Duration::from_secs(3), async {
        while received.len() < payload.len() {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    ensure!(
                        received.len() + data.len() <= 512,
                        "routed SSH payload bound"
                    );
                    received.extend_from_slice(&data);
                }
                Some(ChannelMsg::Close | ChannelMsg::Eof) | None => {
                    anyhow::bail!("routed SSH premature EOF")
                }
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    ensure!(received == payload, "routed SSH byte corruption");
    channel.eof().await?;
    let (mut eof, mut close) = (false, false);
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Eof => eof = true,
                ChannelMsg::Close => {
                    close = true;
                    break;
                }
                ChannelMsg::Data { .. } | ChannelMsg::ExtendedData { .. } => {
                    anyhow::bail!("routed SSH unexpected tail")
                }
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    ensure!(eof && close, "routed SSH EOF/Close missing");
    connection
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(channel);
    drop(connection);
    proxies.finish().await?;
    Ok(
        json!({"proxyOrder":["http-connect","socks5"],"proxyCount":2,"binaryBytes":512,"protocolNeutralBoxStream":true,"sshHostKeyPinned":true,"hostKeyMismatchRejected":true,"refusedConnectionRelaysJoined":true,"sshPasswordAuthenticated":true,"binaryChannelBytesExact":true,"sshEofAndClose":true,"proxyListenersAndRelaysJoined":true}),
    )
}
fn validate_routed(r: &serde_json::Value) -> Result<()> {
    ensure!(
        r["proxyOrder"] == json!(["http-connect", "socks5"])
            && r["proxyCount"] == 2
            && r["binaryBytes"] == 512,
        "routed SSH proxy/byte counts"
    );
    for field in [
        "protocolNeutralBoxStream",
        "sshHostKeyPinned",
        "hostKeyMismatchRejected",
        "refusedConnectionRelaysJoined",
        "sshPasswordAuthenticated",
        "binaryChannelBytesExact",
        "sshEofAndClose",
        "proxyListenersAndRelaysJoined",
    ] {
        ensure!(r[field] == true, "routed SSH evidence missing");
    }
    Ok(())
}

async fn probe() -> Result<serde_json::Value> {
    let mut inner = endpoint(None).await?;
    let mut outer = endpoint(Some(inner.address)).await?;
    let mut first = hop(&outer).await?;
    ensure!(
        first
            .channel_open_direct_tcpip("198.51.100.1", 22, "127.0.0.1", 0)
            .await
            .is_err(),
        "unapproved forward accepted"
    );
    let wrong = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?
        .public_key()
        .clone();
    let rejected = Arc::new(AtomicBool::new(false));
    ensure!(
        nested(&mut first, &inner, wrong, rejected.clone())
            .await
            .is_err()
            && rejected.load(Ordering::SeqCst),
        "inner host key was trusted from outer hop"
    );
    first
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(first);
    drained(&outer).await?;
    drained(&inner).await?;
    let mut outer_session = hop(&outer).await?;
    let mut connection = nested(
        &mut outer_session,
        &inner,
        inner.key.clone(),
        Arc::new(AtomicBool::new(false)),
    )
    .await?;
    ensure!(
        outer_session
            .channel_open_direct_tcpip("127.0.0.1", u32::from(inner.address.port()), "127.0.0.1", 0)
            .await
            .is_err(),
        "single forward credit exceeded"
    );
    ensure!(
        connection
            .authenticate_password(USER, PASS)
            .await?
            .success(),
        "inner auth denied"
    );
    let mut channel = connection.channel_open_session().await?;
    let payload = "nested literal ; $(not-executed) 中文🌍\n".repeat(100);
    channel.data(payload.as_bytes()).await?;
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while received.len() < payload.len() {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    ensure!(received.len() + data.len() <= 8192, "nested output cap");
                    received.extend_from_slice(&data);
                }
                Some(ChannelMsg::Close) | None => anyhow::bail!("nested stream lost"),
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    ensure!(
        received == payload.as_bytes(),
        "nested SSH byte loss/reorder"
    );
    let gbk_bytes = super::gbk_probe::fixture()?;
    channel.data(gbk_bytes.as_slice()).await?;
    let mut gbk_returned = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while gbk_returned.len() < gbk_bytes.len() {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    ensure!(gbk_returned.len() + data.len() <= 4096, "GBK echo bound");
                    gbk_returned.extend_from_slice(&data);
                }
                Some(ChannelMsg::Close) | None => anyhow::bail!("GBK channel closed early"),
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let gbk_report = super::gbk_probe::verify_network_reply(&gbk_returned)?;
    let actor_report =
        super::ssh_actor_probe::run(connection.channel_open_session().await?).await?;

    channel.eof().await?;
    let mut eof = false;
    let mut closed = false;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Eof => eof = true,
                ChannelMsg::Close => {
                    closed = true;
                    break;
                }
                _ => {}
            }
        }
    })
    .await?;
    ensure!(eof && closed, "inner EOF/Close missing");
    connection
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(connection);
    outer_session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(outer_session);
    drained(&outer).await?;
    drained(&inner).await?;
    let routed_report = routed_ssh(&inner).await?;
    drained(&inner).await?;
    ensure!(
        outer.metrics.forward_peak.load(Ordering::SeqCst) == 1
            && outer.metrics.denied.load(Ordering::SeqCst) == 2
            && inner.metrics.bytes.load(Ordering::SeqCst)
                == payload.len() + gbk_bytes.len() + 16 + 512,
        "jump evidence counters missing"
    );
    stop(&mut outer).await?;
    stop(&mut inner).await?;
    Ok(
        json!({"schemaVersion":1,"twoLoopbackSshEndpoints":true,"eachHopHostKeyPinned":true,"innerKeyMismatchRejected":true,"unapprovedDestinationRejected":true,"singleForwardCredit":1,"secondForwardRejected":true,"nestedUtf8Bytes":payload.len(),"gbk":gbk_report,"gbkActor":actor_report,"routedSsh":routed_report,"nestedBytesExact":true,"innerEofAndClose":true,"bothEndpointConnectionsAndRelaysDrained":true,"bothListenersJoined":true,"limitations":["Controlled russh hop/target, not real jump-host implementation matrix","No production Route pool, remote forwarding, arbitrary destinations, SFTP, Agent/FIDO2 or production actor SSH transport"],"platform":std::env::consts::OS}),
    )
}
fn validate_report(r: &serde_json::Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1 && r["singleForwardCredit"] == 1,
        "jump report version/credit"
    );
    for field in [
        "twoLoopbackSshEndpoints",
        "eachHopHostKeyPinned",
        "innerKeyMismatchRejected",
        "unapprovedDestinationRejected",
        "secondForwardRejected",
        "nestedBytesExact",
        "innerEofAndClose",
        "bothEndpointConnectionsAndRelaysDrained",
        "bothListenersJoined",
    ] {
        ensure!(r[field] == true, "missing jump observation: {field}");
    }
    ensure!(
        r["nestedUtf8Bytes"]
            .as_u64()
            .is_some_and(|n| (1024..=8192).contains(&n)),
        "jump byte evidence"
    );
    super::gbk_probe::validate_report(&r["gbk"])?;
    super::ssh_actor_probe::validate(&r["gbkActor"])?;
    validate_routed(&r["routedSsh"])?;
    ensure!(
        r["limitations"].as_array().is_some_and(|a| !a.is_empty()),
        "jump limitations missing"
    );
    Ok(())
}

pub fn run() -> Result<()> {
    let root = super::root()?;
    let destination = root.join("target/probes/ssh-jump.json");
    std::fs::create_dir_all(root.join("target/probes"))?;
    if destination.exists() {
        std::fs::remove_file(&destination)?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let report = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(25), probe()).await })??;
    validate_report(&report)?;
    std::fs::write(destination, serde_json::to_vec_pretty(&report)?)?;
    println!("PASS separately pinned SSH jump/target and bounded relay lifecycle");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routed_ssh_report_requires_key_refusal_and_exact_binary_protocol_counts() {
        let mut r = json!({"proxyOrder":["http-connect","socks5"],"proxyCount":2,"binaryBytes":512,"protocolNeutralBoxStream":true,"sshHostKeyPinned":true,"hostKeyMismatchRejected":true,"refusedConnectionRelaysJoined":true,"sshPasswordAuthenticated":true,"binaryChannelBytesExact":true,"sshEofAndClose":true,"proxyListenersAndRelaysJoined":true});
        assert!(validate_routed(&r).is_ok());
        r["hostKeyMismatchRejected"] = json!(false);
        assert!(validate_routed(&r).is_err());
        r["hostKeyMismatchRejected"] = json!(true);
        r["proxyCount"] = json!(1);
        assert!(validate_routed(&r).is_err());
        assert!(validate_routed(&json!({"completed":true})).is_err());
    }

    #[test]
    fn only_owned_loopback_target_and_exact_port_are_allowed() {
        let address = SocketAddr::from(([127, 0, 0, 1], 2222));
        assert_eq!(
            allowed_destination(Some(address), "127.0.0.1", 2222),
            Some(address)
        );
        for (host, port) in [
            ("localhost", 2222),
            ("198.51.100.1", 2222),
            ("127.0.0.1", 22),
        ] {
            assert!(allowed_destination(Some(address), host, port).is_none());
        }
        assert!(allowed_destination(None, "127.0.0.1", 2222).is_none());
        assert!(
            allowed_destination(
                Some(SocketAddr::from(([198, 51, 100, 1], 2222))),
                "127.0.0.1",
                2222
            )
            .is_none()
        );
    }
    #[test]
    fn single_success_flag_cannot_claim_jump_pinning_or_cleanup() {
        assert!(
            validate_report(&json!({"schemaVersion":1,"singleForwardCredit":1,"completed":true}))
                .is_err()
        );
        assert!(validate_report(&json!({})).is_err());
    }
}
