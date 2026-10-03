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
    Ok(
        json!({"schemaVersion":1,"platform":std::env::consts::OS,"actorNativeInputOutput":true,"queryAndChildResizeValidated":cfg!(unix),"nativeResizeAcknowledged":true,"nonzeroExit":7,"eofFinalized":true,"boundedDiagnosticRestore":true,"directChildCancelAndJoin":true,"receiptDropClosesActor":true,"queueDepth":pulse_term::session::TRANSPORT_QUEUE_CAPACITY,"chunkBytes":pulse_term::session::MAX_CHUNK,"limits":["Direct child only, not process-tree supervision","Windows canonical helper; ConPTY execution requires Windows CI","No view subscription or frozen-tail lease yet","Native resize pauses parsing until ACK; queued pre-resize output may still reflow under new size; not a cross-window attach boundary"]}),
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
