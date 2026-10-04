//! Owned loopback HTTP/SOCKS fixtures, protocol-neutral binary target; no external network.
#[cfg(test)]
#[path = "route_probe_tests.rs"]
mod tests;
use anyhow::{Context, Result, ensure};
use pulse_route::{Credentials, Destination, Dialer, Hop, NetworkDialer, Route, RouteError};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
const PREFIX: &[u8] = b"\0\xffRFB\x1b\x80\xc3";
const PAYLOAD: usize = 256 * 1024;
#[derive(Clone, Copy)]
enum Kind {
    Echo,
    Http,
    Socks,
}
struct Endpoint {
    address: SocketAddr,
    accepted: Arc<AtomicUsize>,
    forwarded: Arc<AtomicUsize>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<()>>>,
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
impl Endpoint {
    async fn stop(&mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.as_mut() {
            tokio::time::timeout(Duration::from_secs(2), task).await???;
        }
        self.task = None;
        Ok(())
    }
}
async fn header(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut h = vec![];
    while !h.ends_with(b"\r\n\r\n") {
        ensure!(h.len() < 8192, "fixture header budget");
        let mut b = [0];
        stream.read_exact(&mut b).await?;
        h.push(b[0]);
    }
    Ok(h)
}
async fn endpoint(kind: Kind, target: Option<SocketAddr>) -> Result<Endpoint> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let accepted = Arc::new(AtomicUsize::new(0));
    let forwarded = Arc::new(AtomicUsize::new(0));
    let a = accepted.clone();
    let f = forwarded.clone();
    let (tx, mut rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut rx => {},
            result = serve_endpoint(listener, kind, target, a, f) => {result?;}
        }
        Ok(())
    });
    Ok(Endpoint {
        address,
        accepted,
        forwarded,
        stop: Some(tx),
        task: Some(task),
    })
}
async fn serve_endpoint(
    listener: TcpListener,
    kind: Kind,
    target: Option<SocketAddr>,
    accepted: Arc<AtomicUsize>,
    forwarded: Arc<AtomicUsize>,
) -> Result<()> {
    let (mut stream, _) = listener.accept().await?;
    accepted.fetch_add(1, Ordering::SeqCst);
    if matches!(kind, Kind::Echo) {
        stream.write_all(PREFIX).await?;
        let mut buffer = [0; 4096];
        let mut total = 0;
        loop {
            let n = stream.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            total += n;
            ensure!(total <= PAYLOAD, "fixture bytes bound");
            stream.write_all(&buffer[..n]).await?;
        }
        ensure!(total == PAYLOAD, "target incomplete input");
        stream.shutdown().await?;
        return Ok(());
    }
    let target = target.context("proxy target missing")?;
    let approved = match kind {
        Kind::Http => {
            let h = header(&mut stream).await?;
            let first = format!("CONNECT {target} HTTP/1.1\r\n");
            ensure!(
                h.starts_with(first.as_bytes()),
                "HTTP fixture unowned target"
            );
            if !h
                .windows(b"Proxy-Authorization: Basic cm91dGU6cHJvYmU=\r\n".len())
                .any(|s| s == b"Proxy-Authorization: Basic cm91dGU6cHJvYmU=\r\n")
            {
                stream
                    .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                    .await?;
                false
            } else {
                true
            }
        }
        Kind::Socks => {
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).await?;
            ensure!(
                greeting == [5, 1, 2],
                "SOCKS fixture requires explicit auth"
            );
            stream.write_all(&[5, 2]).await?;
            let mut first = [0; 2];
            stream.read_exact(&mut first).await?;
            ensure!(first[0] == 1, "SOCKS auth version");
            let mut user = vec![0; first[1] as usize];
            stream.read_exact(&mut user).await?;
            let mut len = [0];
            stream.read_exact(&mut len).await?;
            let mut pass = vec![0; len[0] as usize];
            stream.read_exact(&mut pass).await?;
            if user != b"route" || pass != b"probe" {
                stream.write_all(&[1, 1]).await?;
                false
            } else {
                stream.write_all(&[1, 0]).await?;
                let mut request = [0; 10];
                stream.read_exact(&mut request).await?;
                ensure!(request[..4] == [5, 1, 0, 1], "SOCKS fixed IPv4 CONNECT");
                let requested = SocketAddr::from((
                    [request[4], request[5], request[6], request[7]],
                    u16::from_be_bytes([request[8], request[9]]),
                ));
                ensure!(requested == target, "SOCKS fixture unowned target");
                true
            }
        }
        Kind::Echo => unreachable!(),
    };
    if approved {
        let mut next = TcpStream::connect(target).await?;
        forwarded.fetch_add(1, Ordering::SeqCst);
        match kind {
            Kind::Http => {
                for byte in b"HTTP/1.1 200 Connection established\r\nX-Fixture: split\r\n\r\n" {
                    stream.write_all(&[*byte]).await?;
                }
            }
            Kind::Socks => {
                stream.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await?;
            }
            Kind::Echo => unreachable!(),
        }
        tokio::io::copy_bidirectional(&mut stream, &mut next).await?;
    }
    Ok(())
}
fn credential(bad: bool) -> Result<Credentials> {
    Ok(Credentials::new(
        "route".into(),
        if bad { "wrong" } else { "probe" }.into(),
    )?)
}
async fn case(name: &str) -> Result<Value> {
    let target = endpoint(Kind::Echo, None).await?;
    let inner_kind = if name == "socks-http" {
        Kind::Http
    } else {
        Kind::Socks
    };
    let inner = endpoint(inner_kind, Some(target.address)).await?;
    let outer_kind = if name == "socks-http" {
        Kind::Socks
    } else {
        Kind::Http
    };
    let outer = endpoint(outer_kind, Some(inner.address)).await?;
    let hop = |kind, address, bad| -> Result<Hop> {
        Ok(match kind {
            Kind::Http => Hop::http_connect(address, Some(credential(bad)?))?,
            Kind::Socks => Hop::socks5(address, Some(credential(bad)?))?,
            Kind::Echo => anyhow::bail!("unexpected echo hop"),
        })
    };
    let route = Route::new(if name == "direct" {
        vec![]
    } else {
        vec![
            hop(outer_kind, outer.address, name == "http-denied")?,
            hop(inner_kind, inner.address, name == "socks-denied")?,
        ]
    })?;
    let destination = Destination::new(target.address.ip().to_string(), target.address.port())?;
    let dialer = NetworkDialer::new(Duration::from_secs(3))?;
    let result = dialer.dial(&route, &destination).await;
    let denied = name.ends_with("denied");
    if denied {
        ensure!(
            matches!(result, Err(RouteError::Authentication)),
            "wrong proxy auth accepted"
        );
    } else {
        let stream = result?;
        let (mut reader, mut writer) = tokio::io::split(stream);
        let send = async {
            for offset in (0..PAYLOAD).step_by(4096) {
                let block: Vec<_> = (offset..offset + 4096).map(|n| (n % 251) as u8).collect();
                writer.write_all(&block).await?;
            }
            writer.shutdown().await?;
            Ok::<_, anyhow::Error>(())
        };
        let receive = async {
            let mut prefix = vec![0; PREFIX.len()];
            reader.read_exact(&mut prefix).await?;
            ensure!(prefix == PREFIX, "handshake consumed first binary bytes");
            let mut count = 0;
            let mut buffer = [0; 3071];
            loop {
                let n = reader.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                ensure!(count + n <= PAYLOAD, "duplicate route bytes");
                for (i, byte) in buffer[..n].iter().enumerate() {
                    ensure!(
                        *byte == ((count + i) % 251) as u8,
                        "route corruption/reorder"
                    );
                }
                count += n;
            }
            ensure!(count == PAYLOAD, "route EOF data loss");
            Ok::<_, anyhow::Error>(())
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::try_join!(send, receive)
        })
        .await??;
    }
    let (mut target, mut inner, mut outer) = (target, inner, outer);
    if !denied {
        for e in [&mut target, &mut inner, &mut outer] {
            if e.accepted.load(Ordering::SeqCst) > 0 {
                let task = e.task.as_mut().context("endpoint task")?;
                tokio::time::timeout(Duration::from_secs(2), task).await???;
                e.task = None;
            }
        }
    }
    let counts = |e: &Endpoint| {
        (
            e.accepted.load(Ordering::SeqCst),
            e.forwarded.load(Ordering::SeqCst),
        )
    };
    let expected = match name {
        "direct" => ((1, 0), (0, 0), (0, 0)),
        "http-denied" => ((0, 0), (0, 0), (1, 0)),
        "socks-denied" => ((0, 0), (1, 0), (1, 1)),
        _ => ((1, 0), (1, 1), (1, 1)),
    };
    ensure!(
        (counts(&target), counts(&inner), counts(&outer)) == expected,
        "unauthorized later hop or missing relay"
    );
    for e in [&mut outer, &mut inner, &mut target] {
        e.stop().await?;
    }
    Ok(
        json!({"mode":name,"authenticatedProxyRefusal":denied,"binaryBytesExact":!denied,"payloadBytes":if denied{0}else{PAYLOAD},"firstBinaryPrefixPreserved":!denied,"halfCloseAndFinalEof":!denied,"nextHopAdmissionExact":true,"allListenersAndRelaysJoined":true,"targetCounts":counts(&target),"innerCounts":counts(&inner),"outerCounts":counts(&outer),"prefixBytes":if denied{0}else{PREFIX.len()}}),
    )
}
fn validate(r: &Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1 && r["completed"] == true,
        "Route report version/completion"
    );
    let rows = r["cases"].as_array().context("Route cases absent")?;
    let names = [
        "direct",
        "http-socks",
        "socks-http",
        "http-denied",
        "socks-denied",
    ];
    ensure!(rows.len() == names.len(), "Route incomplete matrix");
    for (row, name) in rows.iter().zip(names) {
        let denied = name.ends_with("denied");
        ensure!(
            row["mode"] == name
                && row["authenticatedProxyRefusal"] == denied
                && row["binaryBytesExact"] == !denied
                && row["firstBinaryPrefixPreserved"] == !denied
                && row["halfCloseAndFinalEof"] == !denied
                && row["nextHopAdmissionExact"] == true
                && row["allListenersAndRelaysJoined"] == true
                && row["payloadBytes"] == if denied { 0 } else { PAYLOAD }
                && row["prefixBytes"] == if denied { 0 } else { PREFIX.len() },
            "Route exact payload/EOF/admission evidence missing"
        );
        let counts = match name {
            "direct" => json!([[1, 0], [0, 0], [0, 0]]),
            "http-denied" => json!([[0, 0], [0, 0], [1, 0]]),
            "socks-denied" => json!([[0, 0], [1, 0], [1, 1]]),
            _ => json!([[1, 0], [1, 1], [1, 1]]),
        };
        ensure!(
            json!([row["targetCounts"], row["innerCounts"], row["outerCounts"]]) == counts,
            "Route admission/relay counts inconsistent"
        );
    }
    ensure!(
        r["limits"].as_array().is_some_and(|a| !a.is_empty()),
        "Route limitations missing"
    );
    Ok(())
}

#[cfg(feature = "ssh-probe")]
pub(crate) struct ProxyChain {
    inner: Endpoint,
    outer: Endpoint,
}
#[cfg(feature = "ssh-probe")]
impl ProxyChain {
    pub(crate) async fn connect(target: SocketAddr) -> Result<(Self, pulse_route::BoxStream)> {
        ensure!(
            target.ip().is_loopback(),
            "proxy fixture destination must be owned loopback"
        );
        let inner = endpoint(Kind::Socks, Some(target)).await?;
        let outer = endpoint(Kind::Http, Some(inner.address)).await?;
        let route = Route::new(vec![
            Hop::http_connect(outer.address, Some(credential(false)?))?,
            Hop::socks5(inner.address, Some(credential(false)?))?,
        ])?;
        let chain = Self { inner, outer };
        let target = Destination::new(target.ip().to_string(), target.port())?;
        let stream = NetworkDialer::new(Duration::from_secs(3))?
            .dial(&route, &target)
            .await?;
        Ok((chain, stream))
    }
    pub(crate) async fn finish(&mut self) -> Result<()> {
        for endpoint in [&mut self.outer, &mut self.inner] {
            ensure!(
                endpoint.accepted.load(Ordering::SeqCst) == 1
                    && endpoint.forwarded.load(Ordering::SeqCst) == 1,
                "protocol routed proxy admission counters"
            );
            let task = endpoint
                .task
                .as_mut()
                .context("protocol relay task missing")?;
            tokio::time::timeout(Duration::from_secs(3), task).await???;
            endpoint.task = None;
            endpoint.stop().await?;
        }
        Ok(())
    }
}

pub fn run() -> Result<()> {
    let root = super::root()?;
    let path = root.join("target/probes/route.json");
    std::fs::create_dir_all(root.join("target/probes"))?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let cases = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut cases = vec![];
            for name in [
                "direct",
                "http-socks",
                "socks-http",
                "http-denied",
                "socks-denied",
            ] {
                cases.push(case(name).await?);
            }
            Ok::<_, anyhow::Error>(cases)
        })
        .await
    })??;
    let r = json!({"schemaVersion":1,"cases":cases,"completed":true,"limits":["Owned loopback HTTP CONNECT/SOCKS fixtures; not real enterprise proxies or cross-platform acceptance","No HTTPS-to-proxy TLS, ProxyCommand, SSH pooled routes, credentials UI or production RPC entry","No end-to-end slow-consumer performance claim"]});
    validate(&r)?;
    std::fs::write(path, serde_json::to_vec_pretty(&r)?)?;
    println!(
        "PASS direct and two authenticated proxy orders, exact binary half-close and refusal cleanup"
    );
    Ok(())
}
