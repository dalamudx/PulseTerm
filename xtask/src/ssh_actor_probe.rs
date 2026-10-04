//! Classified fixed GBK bytes on an already authenticated owned SSH channel.
//! Not a production SSH transport, binary classifier or network-pressure benchmark.
#[cfg(test)]
#[path = "ssh_actor_probe_tests.rs"]
mod tests;
use anyhow::{Result, ensure};
use pulse_app::{attachment::AttachmentConfig, registry::SessionRegistry};
use pulse_core::TermSize;
use pulse_ipc::{
    attach::Part,
    attach_v2::{decode_begin, decode_fragment},
    decode,
};
use pulse_term::{
    TerminalProbe,
    session::{
        ChannelTransport, DriverEndpoint, Lifecycle, SessionHandle, StreamEnd, TransportWrite,
        spawn_gbk_text_session,
    },
};
use russh::{Channel, ChannelMsg, client};
use serde_json::{Value, json};
use std::time::Duration;
async fn echo(
    channel: &mut Channel<client::Msg>,
    driver: &DriverEndpoint,
    bytes: &[u8],
) -> Result<()> {
    channel.data(bytes).await?;
    let mut received = Vec::new();
    while received.len() < bytes.len() {
        match tokio::time::timeout(Duration::from_secs(3), channel.wait()).await? {
            Some(ChannelMsg::Data { data }) => {
                ensure!(
                    received.len() + data.len() <= bytes.len(),
                    "unexpected extra SSH data"
                );
                received.extend_from_slice(&data);
                driver.output.send(data.to_vec()).await?;
            }
            Some(ChannelMsg::Close | ChannelMsg::Eof) | None => anyhow::bail!("premature SSH EOF"),
            _ => {}
        }
    }
    ensure!(received == bytes, "SSH changed original GBK bytes");
    Ok(())
}
async fn at(handle: &SessionHandle, source: u64) -> Result<()> {
    let mut state = handle.state.clone();
    tokio::time::timeout(Duration::from_secs(3), async {
        while state.borrow().parsed_bytes < source {
            state.changed().await?;
        }
        ensure!(
            state.borrow().parsed_bytes == source,
            "actor source boundary mismatch"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    Ok(())
}
async fn commit(
    r: &SessionRegistry,
    o: pulse_app::registry::WindowToken,
    id: pulse_app::registry::SessionId,
    b: &mut pulse_app::registry::RegisteredAttachment,
) -> Result<()> {
    loop {
        let w = tokio::time::timeout(Duration::from_secs(3), b.wire.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("missing capture"))?;
        let f = decode(&w)?;
        let p = decode_fragment(f.payload)?;
        if p.part == Part::Begin {
            let boundary = decode_begin(p.body)?.0;
            ensure!(
                boundary.source_bytes == 1 && boundary.utf8_bytes == 0,
                "GBK lead capture dual coordinate mismatch"
            );
        }
        r.acknowledge(o, id, b.generation, f.sequence)?;
        if p.part == Part::Commit {
            return Ok(());
        }
    }
}
pub(crate) async fn run(mut channel: Channel<client::Msg>) -> Result<Value> {
    let size = TermSize::new(40, 12)?;
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_gbk_text_session(size, 128, transport);
    let r = SessionRegistry::new();
    let owner = r.open_window()?;
    let id = r.register(owner, handle.clone())?;
    let _guard = CloseOnDrop(handle.clone());
    let old = handle
        .diagnostic_subscribe(2000, 4 * 1024 * 1024, 1024 * 1024)
        .await?;
    echo(&mut channel, &driver, &[0xd6]).await?;
    at(&handle, 1).await?;
    ensure!(
        handle.state.borrow().utf8_bytes == 0,
        "pending GBK lead rendered"
    );
    ensure!(
        r.attach(owner, id, AttachmentConfig::default())
            .await
            .is_err(),
        "v1 accepted GBK SSH actor"
    );
    let config = AttachmentConfig {
        fragment: 7,
        frames: 1,
        bytes: 64,
        ack_timeout: Duration::from_secs(3),
    };
    let mut binding = r.attach_v2(owner, id, config).await?;
    ensure!(
        old.status.borrow().end == StreamEnd::Replaced,
        "old lease not replaced"
    );
    commit(&r, owner, id, &mut binding).await?;
    echo(&mut channel, &driver, &[0xd0]).await?;
    at(&handle, 2).await?;
    r.input_bound(owner, id, binding.generation, "中文".as_bytes().to_vec())
        .await?;
    let Some(TransportWrite::Input(input)) =
        tokio::time::timeout(Duration::from_secs(3), driver.input.recv()).await?
    else {
        anyhow::bail!("missing bound SSH input");
    };
    ensure!(input == [0xd6, 0xd0, 0xce, 0xc4], "input reverse encoding");
    echo(&mut channel, &driver, &input).await?;
    at(&handle, 6).await?;
    echo(&mut channel, &driver, b"\x1b[6n").await?;
    at(&handle, 10).await?;
    let Some(TransportWrite::Reply(query)) =
        tokio::time::timeout(Duration::from_secs(3), driver.input.recv()).await?
    else {
        anyhow::bail!("missing backend query reply");
    };
    ensure!(query == b"\x1b[1;7R", "backend-owned query reply");
    echo(&mut channel, &driver, &query).await?;
    at(&handle, 16).await?;
    channel.eof().await?;
    let (mut eof, mut closed) = (false, false);
    while let Some(message) = tokio::time::timeout(Duration::from_secs(3), channel.wait()).await? {
        match message {
            ChannelMsg::Eof => eof = true,
            ChannelMsg::Close => {
                closed = true;
                break;
            }
            ChannelMsg::Data { .. } | ChannelMsg::ExtendedData { .. } => {
                anyhow::bail!("unexpected SSH tail")
            }
            _ => {}
        }
    }
    ensure!(eof && closed, "SSH EOF/Close missing");
    drop(driver);
    let (mut source, mut utf8) = (1u64, 0u64);
    let mut ends = 0;
    while let Some(w) = tokio::time::timeout(Duration::from_secs(3), binding.wire.recv()).await? {
        let f = decode(&w)?;
        let p = decode_fragment(f.payload)?;
        ensure!(p.source_bytes >= source, "wire raw source regression");
        if p.part == Part::Data {
            ensure!(p.offset == utf8, "wire UTF8 gap");
            utf8 += p.body.len() as u64;
        }
        if p.part == Part::End {
            ensure!(
                p.source_bytes == 16 && p.offset == 19,
                "wire EOF mixed coordinates"
            );
            ends += 1;
        }
        source = p.source_bytes;
        r.acknowledge(owner, id, binding.generation, f.sequence)?;
    }
    let receipt = binding.completion.await??;
    let (final_state, screen) = task.join_with_screen().await?;
    ensure!(
        ends == 1
            && source == 16
            && utf8 == 19
            && receipt.boundary == final_state
            && final_state.lifecycle == Lifecycle::Ended
            && final_state.parsed_bytes == 16
            && final_state.utf8_bytes == 19,
        "actor/SSH/wire final coordinates"
    );
    let mut reference = TerminalProbe::new(size, 128);
    reference.feed("中中文\x1b[6n\x1b[1;7R".as_bytes());
    reference.finish_input();
    ensure!(
        screen == reference.screen(),
        "normalized SSH actor screen mismatch"
    );
    r.reap_closed(id)?;
    Ok(
        json!({"sourceBytes":16,"utf8Bytes":19,"pendingCaptureSource":1,"pendingCaptureUtf8":0,"v1Refused":true,"oldLeaseReplaced":true,"boundInputReverseEncoded":true,"backendQueryReplyOnSsh":true,"rawSshEchoExact":true,"binaryV2OffsetsExact":true,"actorScreenExact":true,"sshEofAndClose":true,"orderedEofAcked":true,"peakCreditBytes":receipt.peak_credit_bytes,"limits":["Fixed classified GBK over controlled two-hop russh echo, not a real device","Finite async driver stages are bounded, not end-to-end SSH slow-consumer acceptance","No production SSH adapter, DOM or binary preclassifier"]}),
    )
}
struct CloseOnDrop(SessionHandle);
impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.0.close();
    }
}
pub(crate) fn validate(r: &Value) -> Result<()> {
    ensure!(
        r["sourceBytes"] == 16
            && r["utf8Bytes"] == 19
            && r["pendingCaptureSource"] == 1
            && r["pendingCaptureUtf8"] == 0
            && r["peakCreditBytes"]
                .as_u64()
                .is_some_and(|n| (1..=64).contains(&n)),
        "SSH actor coordinates/credit missing"
    );
    for key in [
        "v1Refused",
        "oldLeaseReplaced",
        "boundInputReverseEncoded",
        "backendQueryReplyOnSsh",
        "rawSshEchoExact",
        "binaryV2OffsetsExact",
        "actorScreenExact",
        "sshEofAndClose",
        "orderedEofAcked",
    ] {
        ensure!(r[key] == true, "SSH actor evidence missing");
    }
    ensure!(
        r["limits"].as_array().is_some_and(|x| !x.is_empty()),
        "SSH actor limitations missing"
    );
    Ok(())
}
