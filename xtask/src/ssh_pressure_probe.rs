//! Controlled lossless ACK pressure. Shared connection stalls are a release LIMIT, not success.
#[path = "ssh_pressure_cancel.rs"]
mod cancel;
#[cfg(test)]
#[path = "ssh_pressure_tests.rs"]
mod tests;
use super::*;
use pulse_app::{attachment::AttachmentConfig, registry::SessionRegistry};
use pulse_core::TermSize;
use pulse_ipc::{attach::Part, attach_v2::decode_fragment, decode};
use pulse_term::{
    TerminalProbe,
    session::{ChannelTransport, DriverEndpoint, Lifecycle, SessionHandle, spawn_gbk_text_session},
};
pub(super) const EXEC: &[u8] = b"pulse-fixed-gbk-pressure";
const BLOCK: usize = 8190;
const BLOCKS: usize = 512;
const TOTAL: usize = BLOCK * BLOCKS;
struct ProducerGuard(Arc<Metrics>);
impl Drop for ProducerGuard {
    fn drop(&mut self) {
        self.0.producing.fetch_sub(1, Ordering::SeqCst);
    }
}
pub(super) async fn produce(
    writer: russh::ChannelWriteHalf<server::Msg>,
    metrics: Arc<Metrics>,
) -> std::result::Result<(), russh::Error> {
    metrics.producing.fetch_add(1, Ordering::SeqCst);
    let _guard = ProducerGuard(metrics.clone());
    let block = [0xd6, 0xd0, b'\r', b'\n', b'a', b'b'].repeat(BLOCK / 6);
    for _ in 0..BLOCKS {
        writer.data(block.as_slice()).await?;
        metrics.produced.fetch_add(block.len(), Ordering::SeqCst);
    }
    writer.eof().await?;
    writer.close().await?;
    metrics.producer_done.store(true, Ordering::SeqCst);
    Ok(())
}

struct ActorGuard(SessionHandle);
impl Drop for ActorGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}
struct Jobs(Vec<tokio::task::AbortHandle>);
impl Drop for Jobs {
    fn drop(&mut self) {
        for job in &self.0 {
            job.abort();
        }
    }
}
fn proxy_order(routed: bool) -> serde_json::Value {
    if routed {
        json!(["http-connect", "socks5"])
    } else {
        json!([])
    }
}
async fn finish_proxies(proxies: &mut Option<super::super::route_probe::ProxyChain>) -> Result<()> {
    if let Some(proxies) = proxies.as_mut() {
        proxies.finish().await?;
    }
    *proxies = None;
    Ok(())
}
async fn connect_pressure(e: &Endpoint) -> Result<client::Handle<Pin>> {
    Ok(connect_path(e, false).await?.0)
}
async fn connect_path(
    e: &Endpoint,
    routed: bool,
) -> Result<(
    client::Handle<Pin>,
    Option<super::super::route_probe::ProxyChain>,
)> {
    let cfg = Arc::new(client::Config {
        window_size: 32768,
        maximum_packet_size: 8192,
        channel_buffer_size: 2,
        inactivity_timeout: Some(Duration::from_secs(15)),
        ..Default::default()
    });
    let pin = Pin {
        key: e.key.clone(),
        rejected: Arc::new(AtomicBool::new(false)),
    };
    let (mut c, proxies) = if routed {
        let (proxies, stream) = super::super::route_probe::ProxyChain::connect(e.address).await?;
        (
            client::connect_stream(cfg, stream, pin).await?,
            Some(proxies),
        )
    } else {
        (client::connect(cfg, e.address, pin).await?, None)
    };
    ensure!(
        c.authenticate_password(USER, PASS).await?.success(),
        "pressure auth"
    );
    Ok((c, proxies))
}
async fn echoed(channel: &mut Channel<client::Msg>, text: &[u8]) -> Result<()> {
    channel.data(text).await?;
    let mut bytes = Vec::new();
    while bytes.len() < text.len() {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                ensure!(
                    bytes.len() + data.len() <= text.len(),
                    "control reply bound"
                );
                bytes.extend_from_slice(&data);
            }
            Some(ChannelMsg::Eof | ChannelMsg::Close) | None => {
                anyhow::bail!("control channel closed")
            }
            _ => {}
        }
    }
    ensure!(bytes == text, "control echo mismatch");
    Ok(())
}
pub(super) async fn probe(routed: bool) -> Result<serde_json::Value> {
    let mut endpoint = endpoint(None).await?;
    let (connection, mut proxies) = connect_path(&endpoint, routed).await?;
    let mut control = connection.channel_open_session().await?;
    echoed(&mut control, b"before").await?;
    let primary = connection.channel_open_session().await?;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (size, history) = (TermSize::new(40, 12)?, 128);
    let (handle, actor) = spawn_gbk_text_session(size, history, transport);
    let _actor_guard = ActorGuard(handle.clone());
    let registry = SessionRegistry::new();
    let owner = registry.open_window()?;
    let id = registry.register(owner, handle.clone())?;
    let bootstrap_projection = handle.diagnostic_digest().await?;
    let mut binding = registry
        .attach_v2(
            owner,
            id,
            AttachmentConfig {
                fragment: 8192,
                frames: 1,
                bytes: 8220,
                ack_timeout: Duration::from_secs(15),
            },
        )
        .await?;
    let sent = endpoint.metrics.clone();
    let read = Arc::new(AtomicUsize::new(0));
    primary.exec(true, EXEC).await?;
    let (read_half, _write_half) = primary.split();
    let reader_count = read.clone();
    let reader = tokio::spawn(async move {
        let mut rx = read_half;
        let DriverEndpoint {
            output,
            input: _input,
            mut stop,
            failed,
        } = driver;
        let (mut eof, mut closed) = (false, false);
        let result=async{loop{let message=tokio::select!{biased;_=stop.changed()=>{anyhow::bail!("actor cancelled SSH reader");},m=rx.wait()=>m};match message{Some(ChannelMsg::Data{data})=>{ensure!(!eof&&data.len()<=8192,"unexpected SSH data/after EOF");reader_count.fetch_add(data.len(),Ordering::SeqCst);tokio::select!{biased;_=stop.changed()=>anyhow::bail!("actor cancelled blocked output"),r=output.send(data.to_vec())=>r?,};},Some(ChannelMsg::Eof)=>eof=true,Some(ChannelMsg::Close)=>{closed=true;break;},None=>break,Some(ChannelMsg::ExtendedData{..})=>anyhow::bail!("unexpected stderr"),_=>{}}}ensure!(eof&&closed,"real SSH EOF/Close absent");Ok::<_,anyhow::Error>(())}.await;
        if result.is_err() {
            failed.send_replace(true);
        }
        drop(output);
        result
    });
    let _jobs = Jobs(vec![reader.abort_handle()]);
    // Hold bootstrap credit while the REAL SSH producer starts, not a local
    // echo queue. The capture remains empty while normalized output advances.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let bootstrap_paused = (
        handle.state.borrow().parsed_bytes,
        read.load(Ordering::SeqCst),
        sent.produced.load(Ordering::SeqCst),
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    ensure!(
        bootstrap_paused
            == (
                handle.state.borrow().parsed_bytes,
                read.load(Ordering::SeqCst),
                sent.produced.load(Ordering::SeqCst)
            )
            && bootstrap_paused.0 > bootstrap_projection.boundary.parsed_bytes
            && bootstrap_paused.2 < TOTAL,
        "bootstrap credit did not stop actor/reader/remote producer"
    );
    ensure!(
        registry.compare_bootstrap(
            owner,
            id,
            binding.generation,
            pulse_app::registry::RenderedScreen {
                boundary: bootstrap_projection.boundary,
                digest: bootstrap_projection.digest
            }
        )? == pulse_app::registry::VisibleComparison::Equal,
        "snapshot confirmation required remote output to stay idle"
    );
    loop {
        let wire = binding
            .wire
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("capture lost"))?;
        let f = decode(&wire)?;
        let commit = decode_fragment(f.payload)?.part == Part::Commit;
        registry.acknowledge(owner, id, binding.generation, f.sequence)?;
        if commit {
            break;
        }
    }

    let first = binding
        .wire
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("live data missing"))?;
    ensure!(
        decode_fragment(decode(&first)?.payload)?.part == Part::Data,
        "first live record not Data"
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    let paused = (
        handle.state.borrow().parsed_bytes,
        read.load(Ordering::SeqCst),
        sent.produced.load(Ordering::SeqCst),
        endpoint.metrics.bytes.load(Ordering::SeqCst),
    );
    let mut request = Box::pin(echoed(&mut control, b"during"));
    let same_connection = tokio::time::timeout(Duration::from_millis(350), &mut request).await;
    ensure!(
        same_connection.is_err(),
        "shared connection became responsive: review known limitation probe"
    );
    // Separate TCP/session proves the service itself is responsive while the first connection is blocked.
    let isolated = connect_pressure(&endpoint).await?;
    let mut separate = isolated.channel_open_session().await?;
    tokio::time::timeout(
        Duration::from_millis(500),
        echoed(&mut separate, b"isolated"),
    )
    .await??;
    separate.eof().await?;
    drop(separate);
    isolated
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(isolated);
    tokio::time::sleep(Duration::from_millis(350)).await;
    let stable = (
        handle.state.borrow().parsed_bytes,
        read.load(Ordering::SeqCst),
        sent.produced.load(Ordering::SeqCst),
    );
    ensure!(
        stable == (paused.0, paused.1, paused.2)
            && paused.0 > 0
            && paused.1 <= 1024 * 1024
            && paused.2 < TOTAL,
        "ACK pause did not stop actor/SSH reader/producer: paused={paused:?} stable={stable:?} total={TOTAL}"
    );
    ensure!(
        !sent.producer_done.load(Ordering::SeqCst),
        "remote producer finished despite paused ACK"
    );
    registry.check_binding(owner, id, binding.generation)?;
    tokio::time::timeout(Duration::from_millis(300), handle.diagnostic_digest()).await??;
    // ACK then byte-validate EVERY live normalized byte, including the deliberately held first frame.
    let mut utf8 = 0u64;
    let mut source = 0u64;
    let mut eof_frames = 0;
    let pattern = "中\r\nab".as_bytes();
    let mut wire = Some(first);
    while let Some(bytes) = wire {
        let f = decode(&bytes)?;
        let p = decode_fragment(f.payload)?;
        ensure!(p.source_bytes >= source, "raw regression");
        if p.part == Part::Data {
            ensure!(p.offset == utf8, "UTF8 loss/reorder");
            for (i, b) in p.body.iter().enumerate() {
                ensure!(
                    *b == pattern[((utf8 as usize) + i) % pattern.len()],
                    "SSH normalized corruption"
                );
            }
            utf8 += p.body.len() as u64;
        }
        if p.part == Part::End {
            ensure!(
                p.offset == utf8 && p.source_bytes == TOTAL as u64,
                "ordered EOF coordinates"
            );
            eof_frames += 1;
        }
        source = p.source_bytes;
        registry.acknowledge(owner, id, binding.generation, f.sequence)?;
        wire = tokio::time::timeout(Duration::from_secs(10), binding.wire.recv()).await?;
    }
    let receipt = binding.completion.await??;
    ensure!(
        sent.producer_done.load(Ordering::SeqCst) && sent.produced.load(Ordering::SeqCst) == TOTAL,
        "remote producer completion missing"
    );
    reader.await??;
    tokio::time::timeout(Duration::from_secs(3), &mut request).await??;
    drop(request);
    control.eof().await?;
    drop(control);
    let (final_state, screen) = actor.join_with_screen().await?;
    ensure!(
        source == TOTAL as u64
            && utf8 == (TOTAL / 6 * 7) as u64
            && eof_frames == 1
            && final_state == receipt.boundary
            && final_state.lifecycle == Lifecycle::Ended,
        "final actor/wire state"
    );
    let mut reference = TerminalProbe::new(size, history);
    let block = "中\r\nab".repeat(BLOCK / 6);
    for _ in 0..BLOCKS {
        reference.feed(block.as_bytes());
    }
    reference.finish_input();
    ensure!(
        screen == reference.screen(),
        "bulk SSH actor screen mismatch"
    );
    registry.reap_closed(id)?;
    connection
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(connection);
    finish_proxies(&mut proxies).await?;
    drained(&endpoint).await?;
    let remote_total = endpoint.metrics.bytes.load(Ordering::SeqCst);
    ensure!(remote_total == 20, "remote control byte accounting");
    stop(&mut endpoint).await?;
    let detached = cancel::run(false, routed).await?;
    let timed_out = cancel::run(true, routed).await?;
    Ok(
        json!({"schemaVersion":1,"proxyOrder":proxy_order(routed),"proxyRelaysJoined":true,"detached":detached,"timedOut":timed_out,"actorControlResponsive":true,"bootstrapWhileRemotePausedChecked":true,"bootstrapPausedActorSource":bootstrap_paused.0,"bootstrapPausedBridgeRead":bootstrap_paused.1,"bootstrapPausedRemoteProduced":bootstrap_paused.2,"sourceBytes":source,"utf8Bytes":utf8,"pausedActorSource":paused.0,"pausedBridgeRead":paused.1,"pausedRemoteProduced":paused.2,"pausedRemoteControlReceived":paused.3,"receiveWindow":32768,"receiveBufferMessages":2,"maxPacket":8192,"peakCreditBytes":receipt.peak_credit_bytes,"rawAndNormalizedBytesExact":true,"actorAndReaderAndRemoteProducerStopped":true,"oldConnectionControlBlocked":true,"separateConnectionResponsive":true,"sameConnectionControlRecoveredAfterAck":true,"actorScreenExact":true,"realSshEofAndClose":true,"orderedEofAcked":true,"allConnectionsAndListenerJoined":true,"limits":["Controlled loopback russh, not network-device or all SSH-server acceptance","Shared SSH connection control is blocked by a full per-channel queue: production multiplexing gate remains","No production SSH transport/route reuse or binary/Zmodem classification"]}),
    )
}
pub(super) fn validate(r: &serde_json::Value, routed: bool) -> Result<()> {
    ensure!(
        r["proxyOrder"] == proxy_order(routed) && r["proxyRelaysJoined"] == true,
        "SSH pressure requested route/relay evidence"
    );
    ensure!(
        r["schemaVersion"] == 1
            && r["sourceBytes"] == TOTAL
            && r["utf8Bytes"] == TOTAL / 6 * 7
            && r["receiveWindow"] == 32768
            && r["receiveBufferMessages"] == 2
            && r["maxPacket"] == 8192
            && r["peakCreditBytes"]
                .as_u64()
                .is_some_and(|n| (1..=8220).contains(&n)),
        "SSH pressure coordinates/bounds"
    );
    ensure!(
        r["bootstrapPausedActorSource"]
            .as_u64()
            .is_some_and(|n| n > 0 && n < TOTAL as u64)
            && r["bootstrapPausedBridgeRead"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= 1024 * 1024)
            && r["bootstrapPausedRemoteProduced"]
                .as_u64()
                .is_some_and(|n| n > 0 && n < TOTAL as u64),
        "bootstrap pause coordinates missing or out of bounds"
    );
    for key in [
        "bootstrapWhileRemotePausedChecked",
        "actorControlResponsive",
        "rawAndNormalizedBytesExact",
        "actorAndReaderAndRemoteProducerStopped",
        "oldConnectionControlBlocked",
        "separateConnectionResponsive",
        "sameConnectionControlRecoveredAfterAck",
        "actorScreenExact",
        "realSshEofAndClose",
        "orderedEofAcked",
        "allConnectionsAndListenerJoined",
    ] {
        ensure!(r[key] == true, "SSH pressure observation missing");
    }
    ensure!(
        r["pausedActorSource"]
            .as_u64()
            .is_some_and(|n| n > 0 && n < TOTAL as u64)
            && r["pausedBridgeRead"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= 1024 * 1024)
            && r["pausedRemoteProduced"]
                .as_u64()
                .is_some_and(|n| n > 0 && n < TOTAL as u64)
            && r["limits"].as_array().is_some_and(|x| !x.is_empty()),
        "pause metadata/limitations"
    );
    cancel::validate(&r["detached"], false, routed)?;
    cancel::validate(&r["timedOut"], true, routed)?;
    Ok(())
}
