#[path = "local_session_transfer.rs"]
mod transfer;
use anyhow::{Context, Result, ensure};
use pulse_core::TermSize;
use pulse_pty::PtyCommand;
use pulse_term::session::{Lifecycle, MAX_RESTORE_BYTES, SessionHandle};
use serde_json::json;
use std::time::{Duration, Instant};
fn line_input(line: &str) -> Vec<u8> {
    let mut bytes = line.as_bytes().to_vec();
    bytes.push(if cfg!(windows) { b'\r' } else { b'\n' });
    bytes
}
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
pub(crate) fn command(mode: &str) -> Result<PtyCommand> {
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
pub(crate) fn detached_helper() -> Result<()> {
    use std::io::{BufRead, Write};
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(2);
    });
    let mut out = std::io::stdout().lock();
    writeln!(out, "DETACHED_READY\r")?;
    out.flush()?;
    for line in std::io::stdin().lock().lines() {
        match line?.as_str() {
            "first" => {
                write!(out, "\x1b[1;1HDETACHED_FIRST 中文🌍\r\n")?;
            }
            "next" => {
                write!(out, "\x1b[2;1HDETACHED_NEXT 中文🌍\r\n")?;
            }
            "exit" => {
                write!(out, "\x1b[3;1HDETACHED_EXIT\r\n")?;
                out.flush()?;
                return Ok(());
            }
            _ => anyhow::bail!("invalid detached helper command"),
        };
        out.flush()?;
    }
    anyhow::bail!("detached helper unexpected stdin EOF")
}
async fn detached_session() -> Result<serde_json::Value> {
    use pulse_app::{attachment::AttachmentConfig, registry::SessionRegistry};
    use pulse_ipc::attach::{Part, decode_fragment};
    let size = TermSize::new(80, 24)?;
    let (session, task) =
        pulse_app::spawn_local(command("local-session-helper-detached")?, size, 128).await?;
    let registry = SessionRegistry::new();
    let owner = registry.open_window()?;
    let id = registry.register(owner, session.clone())?;
    visible(&session, "DETACHED_READY").await?;
    let config = AttachmentConfig {
        frames: 1,
        fragment: 8192,
        bytes: 9000,
        ack_timeout: Duration::from_secs(3),
    };
    let mut old = registry.attach(owner, id, config).await?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(bytes) = old.wire.recv().await {
            let frame = pulse_ipc::decode(&bytes)?;
            let part = decode_fragment(frame.payload)?;
            registry.acknowledge(owner, id, part.generation, frame.sequence)?;
            if part.part == Part::Commit {
                return Ok::<_, anyhow::Error>(());
            }
        }
        anyhow::bail!("missing initial Commit")
    })
    .await??;
    registry.detach(owner, id, old.generation)?;
    ensure!(
        matches!(
            tokio::time::timeout(Duration::from_secs(1), old.completion).await??,
            Err(pulse_app::attachment::AttachmentError::Detached)
        ),
        "detach receipt"
    );
    let before = registry.state(owner, id)?.parsed_bytes;
    ensure!(
        registry
            .input_bound(owner, id, old.generation, line_input("exit"))
            .await
            .is_err(),
        "old view input accepted"
    );
    registry.input(owner, id, line_input("first")).await?;
    visible(&session, "DETACHED_FIRST").await?;
    let held = Instant::now();
    tokio::time::sleep(Duration::from_secs(6)).await;
    let detached_ms = held.elapsed().as_millis();
    ensure!(
        registry.state(owner, id)?.lifecycle == Lifecycle::Running,
        "detached view timed out session"
    );
    registry.input(owner, id, line_input("next")).await?;
    visible(&session, "DETACHED_NEXT").await?;
    let captured = registry.state(owner, id)?;
    ensure!(
        captured.parsed_bytes > before,
        "detached output did not parse"
    );
    let mut fresh = registry.attach(owner, id, config).await?;
    ensure!(
        fresh.generation > old.generation,
        "generation did not advance"
    );
    ensure!(
        registry.check_binding(owner, id, old.generation).is_err(),
        "old generation revived"
    );
    let mut replay = pulse_term::TerminalProbe::new(size, 128);
    let mut offset = None;
    let mut sequence = 0;
    let mut committed = false;
    let mut ended = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(bytes) = fresh.wire.recv().await {
            let frame = pulse_ipc::decode(&bytes)?;
            ensure!(frame.sequence == sequence, "detached wire reorder");
            sequence += 1;
            let part = decode_fragment(frame.payload)?;
            ensure!(part.generation == fresh.generation, "wire generation");
            match part.part {
                Part::Begin => {
                    offset = Some(u64::from_le_bytes(part.body[12..20].try_into()?));
                    ensure!(
                        offset == Some(captured.parsed_bytes),
                        "capture boundary moved unexpectedly"
                    );
                }
                Part::History | Part::Restore => {
                    replay.feed(part.body);
                    replay.take_replies();
                }
                Part::Commit => {
                    ensure!(
                        replay.screen().cells == session.inspect().await?.cells,
                        "detached hidden output lost on recovery"
                    );
                    committed = true;
                    registry
                        .input_bound(owner, id, fresh.generation, line_input("exit"))
                        .await?;
                }
                Part::Data => {
                    ensure!(Some(part.offset) == offset, "detached tail gap");
                    offset = Some(part.offset + part.body.len() as u64);
                    replay.feed(part.body);
                    replay.take_replies();
                }
                Part::Checkpoint | Part::Marker => {
                    ensure!(Some(part.offset) == offset, "detached checkpoint")
                }
                Part::End => {
                    ensure!(Some(part.offset) == offset, "detached EOF offset");
                    replay.finish_input();
                    ended = true;
                }
                Part::RepairBegin | Part::RepairBytes | Part::RepairCommit => {
                    anyhow::bail!("unexpected detached repair")
                }
            };
            registry.acknowledge(owner, id, part.generation, frame.sequence)?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let receipt = fresh.completion.await??;
    let (state, exit) = tokio::time::timeout(Duration::from_secs(3), task.join()).await??;
    ensure!(
        committed && ended && state.lifecycle == Lifecycle::Ended && exit.exit_code == 0,
        "detached EOF/native join"
    );
    ensure!(
        Some(state.parsed_bytes) == offset && receipt.boundary == state,
        "detached final boundary"
    );
    let text = replay
        .screen()
        .cells
        .iter()
        .flatten()
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    ensure!(text.contains("DETACHED_EXIT"), "detached final screen");
    Ok(
        json!({"detachedMs":detached_ms,"sessionSurvivedWithoutView":true,"hiddenOutputParsed":true,"oldInputRejected":true,"exactReattachAfterGuiProbeTimeout":true,"newGeneration":fresh.generation,"orderedEofAck":true,"directChildJoin":true,"renderer":"Rust validator; not a GUI parked recovery test","platform":std::env::consts::OS}),
    )
}

async fn wire_streaming() -> Result<serde_json::Value> {
    wire_streaming_command(command("portable-pty-helper-stream")?, false).await
}
pub(crate) async fn wire_streaming_command(
    native_command: PtyCommand,
    expect_shell_hints: bool,
) -> Result<serde_json::Value> {
    use pulse_app::{attachment::AttachmentConfig, registry::SessionRegistry};
    use pulse_ipc::attach::{Part, decode_fragment};
    let size = TermSize::new(80, 24)?;
    let (session, task) = pulse_app::spawn_local(native_command, size, 128).await?;
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
    let mut burst_bytes = 0usize;
    let mut shell_hint_count = None;
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
                    let sampled = registry
                        .take_shell_markers(window, id, binding.generation)
                        .await?;
                    let expected = if expect_shell_hints {
                        vec![
                            pulse_term::shell_markers::ShellMarker::PromptStart,
                            pulse_term::shell_markers::ShellMarker::PromptEnd,
                            pulse_term::shell_markers::ShellMarker::CommandStart,
                        ]
                    } else {
                        Vec::new()
                    };
                    ensure!(
                        sampled.hints.markers == expected && sampled.hints.dropped == 0,
                        "wire shell hint loss/invention"
                    );
                    shell_hint_count = Some(sampled.hints.markers.len());
                    registry
                        .input(window, id, line_input("literal ; $(input) 中"))
                        .await?;
                }
                Part::Data => {
                    ensure!(Some(part.offset) == offset, "wire duplicate/loss");
                    burst_bytes += part.body.iter().filter(|byte| **byte == b'X').count();
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
    let supervised =
        tokio::time::timeout(Duration::from_secs(3), task.join_with_receipt()).await??;
    let final_hints = supervised.actor.shell_markers;
    let expected_final = if expect_shell_hints {
        vec![pulse_term::shell_markers::ShellMarker::CommandEnd {
            exit_status: Some(7),
        }]
    } else {
        Vec::new()
    };
    ensure!(
        final_hints.markers == expected_final && final_hints.dropped == 0,
        "wire final hints lost/invented"
    );
    let advisory_exit_status = if expect_shell_hints { Some(7u8) } else { None };
    let state = supervised.actor.state;
    let exit = supervised.native;
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
    #[cfg(unix)]
    ensure!(
        burst_bytes == 2 * 1024 * 1024,
        "wire burst byte loss/duplication"
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
        json!({"shellHintsChecked":shell_hint_count.is_some(),"shellHintCount":shell_hint_count,"finalShellHintsChecked":true,"finalShellHintCount":final_hints.markers.len(),"advisoryExitStatus":advisory_exit_status,"nativeExitCode":exit.exit_code,"registryOwnerGenerationAuthorization":true,"ackToNativeBackpressure":true,"pausedParsedBytes":pause,"wireFrames":receipt.frames,"creditPeakBytes":receipt.peak_credit_bytes,"creditLimitBytes":config.bytes,"orderedEofAck":true,"finalParsedBytes":offset,"burstBytes":burst_bytes,"burstExactChecked":cfg!(unix),"renderer":"Rust validator; not frontend callbacks","platform":std::env::consts::OS}),
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
    handle.input(line_input("literal ; $(input) 中")).await?;
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
    ensure!(
        held.state.borrow().lifecycle == Lifecycle::Running,
        "hold helper exited before cancellation"
    );
    let start = Instant::now();
    held.close();
    let (state, exit) = tokio::time::timeout(Duration::from_secs(3), task.join())
        .await
        .context("native cancellation timeout")??;
    ensure!(
        matches!(state.lifecycle, Lifecycle::Closed | Lifecycle::Ended),
        "close did not terminate actor: {:?}, native_cancelled={}, native_exit={}",
        state.lifecycle,
        exit.cancelled,
        exit.exit_code
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
    let detached = detached_session().await?;
    let transferred = transfer::run().await?;
    Ok(
        json!({"transferred":transferred,"detached":detached,"wire":wire,"stream":stream,"schemaVersion":5,"platform":std::env::consts::OS,"actorNativeInputOutput":true,"queryAndChildResizeValidated":cfg!(unix),"nativeResizeAcknowledged":true,"nonzeroExit":7,"eofFinalized":true,"boundedDiagnosticRestore":true,"directChildCancelAndJoin":true,"cancelActorLifecycle":format!("{:?}",state.lifecycle),"nativeCancellationObserved":exit.cancelled,"receiptDropClosesActor":true,"queueDepth":pulse_term::session::TRANSPORT_QUEUE_CAPACITY,"chunkBytes":pulse_term::session::MAX_CHUNK,"limits":["Direct child only, not process-tree supervision","Windows canonical helper; ConPTY execution requires Windows CI","One authorized registry binding; runtime WebView/window lifecycle adapter remains unimplemented","Native resize pauses parsing until ACK; queued pre-resize output may still reflow under new size; not a cross-window attach boundary"]}),
    )
}
pub fn run() -> Result<()> {
    std::fs::create_dir_all(super::root()?.join("target/probes"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let report = runtime.block_on(probe())?;
    transfer::validate(&report["transferred"])?;
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
