use anyhow::{Context, Result, ensure};
use pulse_core::TermSize;
use pulse_pty::PtyCommand;
use pulse_term::session::{Lifecycle, MAX_RESTORE_BYTES, SessionHandle};
use serde_json::json;
use std::time::{Duration, Instant};
async fn visible(handle: &SessionHandle, marker: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let screen = handle.inspect().await?;
            let text = screen
                .cells
                .into_iter()
                .map(|r| r.into_iter().map(|c| c.text).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            if text.contains(marker) {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("visible marker deadline")??;
    Ok(())
}
fn command(mode: &str) -> Result<PtyCommand> {
    let mut command = PtyCommand::new(std::env::current_exe()?);
    command.args = vec![mode.into(), "literal ; $(argv)".into()];
    command.cwd = Some(super::root()?.join("target/probes"));
    command
        .env
        .push(("PULSE_PTY_TEST".into(), "explicit-value".into()));
    command
        .env
        .push(("PULSE_PTY_PORTABLE".into(), "explicit-value".into()));
    Ok(command)
}
async fn wire_streaming() -> Result<serde_json::Value> {
    use pulse_app::{attachment::AttachmentConfig, registry::SessionRegistry};
    use pulse_ipc::attach::{Part, decode_fragment};
    let size = TermSize::new(80, 24)?;
    let (session, task) =
        pulse_app::spawn_local(command("portable-pty-helper-stream")?, size, 128).await?;
    visible(&session, "PORTABLE_STREAM_READY").await?;
    let config = AttachmentConfig {
        frames: 1,
        fragment: 8192,
        bytes: 9000,
        ack_timeout: Duration::from_secs(5),
    };
    let registry = SessionRegistry::new();
    let window = registry.open_window()?;
    let id = registry.register(window, session.clone())?;
    let binding = registry.attach(window, id, config).await?;
    let mut wire = binding.wire;
    let mut replay = pulse_term::TerminalProbe::new(size, 128);
    let mut sequence = 0;
    let mut offset = None;
    let mut pause = None;
    let mut end = false;
    tokio::time::timeout(Duration::from_secs(12), async {
        while let Some(bytes) = wire.recv().await {
            let frame = pulse_ipc::decode(&bytes)?;
            ensure!(frame.sequence == sequence, "wire reorder");
            sequence += 1;
            let part = decode_fragment(frame.payload)?;
            match part.part {
                Part::Begin => {
                    offset = Some(u64::from_le_bytes(part.body[12..20].try_into()?));
                }
                Part::History | Part::Restore => {
                    replay.feed(part.body);
                    replay.take_replies();
                }
                Part::Commit => {
                    registry
                        .input(window, id, "literal ; $(input) 中\n".as_bytes().to_vec())
                        .await?;
                }
                Part::Data => {
                    ensure!(Some(part.offset) == offset, "wire duplicate/loss");
                    offset = Some(part.offset + part.body.len() as u64);
                    replay.feed(part.body);
                    replay.take_replies();
                    if pause.is_none() {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        let stopped = session.state.borrow().parsed_bytes;
                        tokio::time::sleep(Duration::from_millis(80)).await;
                        ensure!(
                            session.state.borrow().parsed_bytes == stopped,
                            "ACK pressure did not reach native parser"
                        );
                        session.inspect().await?;
                        pause = Some(stopped);
                    }
                }
                Part::RepairBegin | Part::RepairBytes | Part::RepairCommit => {
                    anyhow::bail!("unsolicited repair")
                }
                Part::Checkpoint | Part::Marker => {
                    ensure!(Some(part.offset) == offset, "wire checkpoint bytes");
                }
                Part::End => {
                    ensure!(Some(part.offset) == offset, "wire end offset");
                    replay.finish_input();
                    end = true;
                }
            }
            registry.acknowledge(window, id, part.generation, frame.sequence)?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("native wire deadline")??;
    let receipt = binding.completion.await??;
    let (state, exit) = tokio::time::timeout(Duration::from_secs(3), task.join()).await??;
    ensure!(
        end && state.lifecycle == Lifecycle::Ended && exit.exit_code == 0,
        "wire final native status"
    );
    ensure!(
        Some(state.parsed_bytes) == offset && receipt.boundary == state,
        "wire final backend boundary"
    );
    ensure!(
        receipt.peak_credit_bytes <= config.bytes,
        "wire credit bound"
    );
    let text = replay.screen().cells[0]
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    ensure!(
        text.starts_with("PORTABLE_STREAM_DONE"),
        "wire final screen"
    );
    Ok(
        json!({"registryOwnerGenerationAuthorization":true,"ackToNativeBackpressure":true,"pausedParsedBytes":pause,"wireFrames":receipt.frames,"creditPeakBytes":receipt.peak_credit_bytes,"creditLimitBytes":config.bytes,"orderedEofAck":true,"finalParsedBytes":offset,"renderer":"Rust validator; not frontend callbacks","platform":std::env::consts::OS}),
    )
}
async fn streaming() -> Result<serde_json::Value> {
    use pulse_term::session::{MAX_HISTORY_BYTES, StreamEnd, StreamRecord};
    let size = TermSize::new(80, 24)?;
    let (handle, task) =
        pulse_app::spawn_local(command("portable-pty-helper-stream")?, size, 128).await?;
    visible(&handle, "PORTABLE_STREAM_READY").await?;
    let old = handle
        .diagnostic_subscribe(20, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .map_err(|e| anyhow::anyhow!("native first lease: {e}"))?;
    handle
        .input("literal ; $(input) 中\n".as_bytes().to_vec())
        .await?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while old.output.len() != pulse_term::session::TRANSPORT_QUEUE_CAPACITY {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("native lease never filled")?;
    let paused = handle.state.borrow().parsed_bytes;
    tokio::time::sleep(Duration::from_millis(80)).await;
    ensure!(
        handle.state.borrow().parsed_bytes == paused,
        "full view did not stop parsing"
    );
    ensure!(
        handle.state.borrow().lifecycle == Lifecycle::Running,
        "native actor unexpectedly ended"
    );
    handle.inspect().await?; // Control remains live under stream pressure.
    let mut view = handle
        .diagnostic_subscribe(20, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await
        .map_err(|e| anyhow::anyhow!("native reattach: {e}"))?;
    ensure!(
        view.lease_id > old.lease_id && old.status.borrow().end == StreamEnd::Replaced,
        "lease replacement failed"
    );
    ensure!(
        view.bootstrap.boundary.parsed_bytes == paused,
        "capture drifted while frozen"
    );
    let mut replay = pulse_term::TerminalProbe::new(size, 128);
    replay.feed(&view.bootstrap.history.bytes);
    replay.feed(&view.bootstrap.restore);
    let mut offset = view.bootstrap.boundary.parsed_bytes;
    let mut chunks = 0;
    tokio::time::timeout(Duration::from_secs(8), async {
        while let Some(record) = view.output.recv().await {
            match record {
                StreamRecord::Data {
                    start,
                    bytes,
                    boundary,
                } => {
                    ensure!(
                        start == offset && boundary.parsed_bytes == start + bytes.len() as u64,
                        "lost or duplicate native tail"
                    );
                    ensure!(boundary.size == size, "unexpected native geometry");
                    replay.feed(&bytes);
                    replay.take_replies();
                    offset = boundary.parsed_bytes;
                    chunks += 1;
                }
                StreamRecord::Repair { .. } => anyhow::bail!("unexpected unsolicited repair"),
                StreamRecord::Fence(boundary) | StreamRecord::Marker(boundary) => {
                    ensure!(boundary.parsed_bytes == offset, "fence manufactured bytes");
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("native live consumption deadline")??;
    replay.finish_input();
    let (final_state, exit) = tokio::time::timeout(Duration::from_secs(3), task.join())
        .await
        .context("native live join deadline")??;
    ensure!(
        exit.exit_code == 0 && final_state.lifecycle == Lifecycle::Ended,
        "native stream did not exit cleanly"
    );
    ensure!(
        offset == final_state.parsed_bytes
            && view.status.borrow().end == StreamEnd::Session(Lifecycle::Ended),
        "native terminal boundary lost"
    );
    let text = replay.screen().cells[0]
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    ensure!(
        text.starts_with("PORTABLE_STREAM_DONE"),
        "stream replay lost final reset/marker"
    );
    Ok(
        json!({"atomicBootstrapTail":true,"pausedParsedBytes":paused,"boundedParserBackpressure":true,"controlUnderPressure":true,"replacementGeneration":view.lease_id,"continuousByteOffsets":true,"liveChunks":chunks,"finalParsedBytes":offset,"eofAndDirectChildJoin":true,"platform":std::env::consts::OS}),
    )
}
async fn probe() -> Result<serde_json::Value> {
    let size = TermSize::new(80, 24)?;
    let mode = if cfg!(unix) {
        "pty-helper-interactive"
    } else {
        "portable-pty-helper-interactive"
    };
    let (handle, task) = pulse_app::spawn_local(command(mode)?, size, 128).await?;
    visible(
        &handle,
        if cfg!(unix) {
            "READY"
        } else {
            "PORTABLE_READY"
        },
    )
    .await?;
    handle.resize(TermSize::new(91, 37)?).await?;
    let input = if cfg!(unix) {
        "input with ; $(literal)\n"
    } else {
        "literal ; $(input) 中\r"
    };
    handle.input(input.as_bytes().to_vec()).await?;
    let (state, exit) = tokio::time::timeout(Duration::from_secs(8), task.join())
        .await
        .context("native join timeout")??;
    ensure!(
        state.lifecycle == Lifecycle::Ended,
        "actor did not finalize EOF: {:?}",
        state.lifecycle
    );
    ensure!(exit.exit_code == 7, "native exit lost");
    ensure!(
        exit.resize_count == 1 && state.size == TermSize::new(91, 37)?,
        "resize not acknowledged"
    );
    ensure!(
        exit.written_bytes >= input.len() as u64,
        "input not fully written"
    );
    let (held, task) =
        pulse_app::spawn_local(command("portable-pty-helper-hold")?, size, 0).await?;
    visible(&held, "PORTABLE_HOLD").await?;
    let restore = held.diagnostic_restore(MAX_RESTORE_BYTES).await?;
    let mut replay = pulse_term::TerminalProbe::new(restore.boundary.size, 0);
    replay.feed(&restore.bytes);
    ensure!(
        replay.screen() == held.inspect().await?,
        "native actor diagnostic restore failed"
    );
    let start = Instant::now();
    held.close();
    let (state, exit) = tokio::time::timeout(Duration::from_secs(3), task.join())
        .await
        .context("native cancellation timeout")??;
    ensure!(
        state.lifecycle == Lifecycle::Closed && exit.cancelled,
        "cancel not recorded"
    );
    ensure!(start.elapsed() < Duration::from_secs(2), "cancel too slow");
    let (dropped, task) =
        pulse_app::spawn_local(command("portable-pty-helper-hold")?, size, 0).await?;
    visible(&dropped, "PORTABLE_HOLD").await?;
    drop(task);
    let mut status = dropped.state.clone();
    tokio::time::timeout(Duration::from_secs(2), async {
        while status.borrow().lifecycle == Lifecycle::Running {
            status.changed().await?;
        }
        Ok::<_, tokio::sync::watch::error::RecvError>(())
    })
    .await??;
    ensure!(
        status.borrow().lifecycle == Lifecycle::Closed,
        "receipt drop did not cancel actor"
    );
    let stream = streaming().await?;
    let wire = wire_streaming().await?;
    Ok(
        json!({"wire":wire,"stream":stream,"schemaVersion":3,"platform":std::env::consts::OS,"actorNativeInputOutput":true,"queryAndChildResizeValidated":cfg!(unix),"nativeResizeAcknowledged":true,"nonzeroExit":7,"eofFinalized":true,"boundedDiagnosticRestore":true,"directChildCancelAndJoin":true,"receiptDropClosesActor":true,"queueDepth":pulse_term::session::TRANSPORT_QUEUE_CAPACITY,"chunkBytes":pulse_term::session::MAX_CHUNK,"limits":["Direct child only, not process-tree supervision","Windows canonical helper; ConPTY execution requires Windows CI","One authorized registry binding; runtime WebView/window lifecycle adapter remains unimplemented","Native resize pauses parsing until ACK; queued pre-resize output may still reflow under new size; not a cross-window attach boundary"]}),
    )
}
pub fn run() -> Result<()> {
    std::fs::create_dir_all(super::root()?.join("target/probes"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let report = runtime.block_on(probe())?;
    std::fs::write(
        super::root()?.join("target/probes/local-session.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "PASS native actor {}: input/output, resize ACK, EOF, bounded restore and cancellation",
        std::env::consts::OS
    );
    Ok(())
}
