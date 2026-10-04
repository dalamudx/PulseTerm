//! Revoking the view cannot close a busy SSH actor. Private fixture only.
use super::*;
pub(super) async fn run(timeout: bool, routed: bool) -> Result<serde_json::Value> {
    let mut endpoint = endpoint(None).await?;
    let (connection, mut proxies) = connect_path(&endpoint, routed).await?;
    let mut control = connection.channel_open_session().await?;
    let primary = connection.channel_open_session().await?;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let size = TermSize::new(40, 12)?;
    let (handle, actor) = spawn_gbk_text_session(size, 128, transport);
    let _guard = ActorGuard(handle.clone());
    let r = SessionRegistry::new();
    let owner = r.open_window()?;
    let id = r.register(owner, handle.clone())?;
    let mut binding = r
        .attach_v2(
            owner,
            id,
            AttachmentConfig {
                fragment: 8192,
                frames: 1,
                bytes: 8220,
                ack_timeout: Duration::from_secs(3),
            },
        )
        .await?;
    loop {
        let w = binding
            .wire
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("capture absent"))?;
        let f = decode(&w)?;
        let commit = decode_fragment(f.payload)?.part == Part::Commit;
        r.acknowledge(owner, id, binding.generation, f.sequence)?;
        if commit {
            break;
        }
    }
    primary.exec(true, EXEC).await?;
    let (mut rx, _write) = primary.split();
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let reader = tokio::spawn(async move {
        let DriverEndpoint {
            output,
            input: _input,
            mut stop,
            failed,
        } = driver;
        let (mut eof, mut closed) = (false, false);
        let result=async{loop{let message=tokio::select!{biased;_=stop.changed()=>anyhow::bail!("SSH actor stopped"),m=rx.wait()=>m};match message{Some(ChannelMsg::Data{data})=>{ensure!(!eof&&data.len()<=8192,"SSH output cap");count.fetch_add(data.len(),Ordering::SeqCst);tokio::select!{biased;_=stop.changed()=>anyhow::bail!("SSH actor stopped pending send"),result=output.send(data.to_vec())=>result?,};},Some(ChannelMsg::Eof)=>eof=true,Some(ChannelMsg::Close)=>{closed=true;break;},None=>break,Some(ChannelMsg::ExtendedData{..})=>anyhow::bail!("unexpected stderr"),_=>{}}}ensure!(eof&&closed,"remote EOF/Close absent");Ok::<_,anyhow::Error>(())}.await;
        if result.is_err() {
            failed.send_replace(true);
        }
        drop(output);
        result
    });
    let _jobs = Jobs(vec![reader.abort_handle()]);
    let first = binding
        .wire
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("live frame absent"))?;
    ensure!(
        decode_fragment(decode(&first)?.payload)?.part == Part::Data,
        "no first Data"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = (
        handle.state.borrow().parsed_bytes,
        reads.load(Ordering::SeqCst),
        endpoint.metrics.produced.load(Ordering::SeqCst),
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    ensure!(
        before
            == (
                handle.state.borrow().parsed_bytes,
                reads.load(Ordering::SeqCst),
                endpoint.metrics.produced.load(Ordering::SeqCst)
            )
            && before.2 < TOTAL,
        "remote not paused on unacked frame"
    );
    tokio::time::timeout(Duration::from_millis(300), handle.diagnostic_digest()).await??;
    ensure!(
        handle.state.borrow().lifecycle == Lifecycle::Running,
        "actor not alive on pause"
    );
    if !timeout {
        r.detach(owner, id, binding.generation)?;
    }
    let failure = tokio::time::timeout(Duration::from_secs(5), binding.completion).await??;
    ensure!(
        matches!(
            (timeout, failure),
            (
                true,
                Err(pulse_app::attachment::AttachmentError::AckTimeout)
            ) | (false, Err(pulse_app::attachment::AttachmentError::Detached))
        ),
        "view revocation reason mismatch"
    );
    drop(binding.wire);
    ensure!(
        r.check_binding(owner, id, binding.generation).is_err(),
        "revoked view still active"
    );
    tokio::time::timeout(
        Duration::from_secs(3),
        echoed(&mut control, b"after-release"),
    )
    .await??;
    reader.await??;
    let (final_state, screen) = actor.join_with_screen().await?;
    ensure!(
        final_state.lifecycle == Lifecycle::Ended
            && final_state.parsed_bytes == TOTAL as u64
            && final_state.utf8_bytes == (TOTAL / 6 * 7) as u64
            && reads.load(Ordering::SeqCst) == TOTAL
            && endpoint.metrics.produced.load(Ordering::SeqCst) == TOTAL
            && endpoint.metrics.producer_done.load(Ordering::SeqCst),
        "detach/timeout closed or lost SSH actor"
    );
    let mut reference = TerminalProbe::new(size, 128);
    let block = "中\r\nab".repeat(BLOCK / 6);
    for _ in 0..BLOCKS {
        reference.feed(block.as_bytes());
    }
    reference.finish_input();
    ensure!(screen == reference.screen(), "unbound actor screen differs");
    r.reap_closed(id)?;
    control.eof().await?;
    drop(control);
    connection
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(connection);
    finish_proxies(&mut proxies).await?;
    drained(&endpoint).await?;
    stop(&mut endpoint).await?;
    Ok(
        json!({"proxyOrder":proxy_order(routed),"proxyRelaysJoined":true,"kind":if timeout{"ack-timeout"}else{"detach"},"sourceBytes":final_state.parsed_bytes,"utf8Bytes":final_state.utf8_bytes,"actorControlResponsive":true,"viewRevokedOnly":true,"sharedControlRecovered":true,"remoteResumed":true,"actorEndedNormally":true,"fullActorScreenExact":true,"connectionsListenerAndProducerJoined":true}),
    )
}
pub(super) fn validate(r: &serde_json::Value, timeout: bool, routed: bool) -> Result<()> {
    ensure!(
        r["proxyOrder"] == proxy_order(routed) && r["proxyRelaysJoined"] == true,
        "SSH release requested route/relay evidence"
    );
    ensure!(
        r["kind"] == if timeout { "ack-timeout" } else { "detach" }
            && r["sourceBytes"] == TOTAL
            && r["utf8Bytes"] == TOTAL / 6 * 7,
        "SSH release coordinates/kind"
    );
    for key in [
        "actorControlResponsive",
        "viewRevokedOnly",
        "sharedControlRecovered",
        "remoteResumed",
        "actorEndedNormally",
        "fullActorScreenExact",
        "connectionsListenerAndProducerJoined",
    ] {
        ensure!(r[key] == true, "SSH view-release stage missing");
    }
    Ok(())
}
