//! One real fixed PTY child, two trusted owner incarnations; not runtime GUI migration.
use super::*;
use pulse_app::{
    attachment::AttachmentConfig,
    registry::{RegisteredAttachment, SessionId, SessionRegistry, WindowToken},
};
use pulse_ipc::{
    attach::{Part, decode_begin, decode_fragment},
    decode,
};
use pulse_term::TerminalProbe;
struct Guard(SessionHandle);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.close();
    }
}
async fn bootstrap(
    r: &SessionRegistry,
    o: WindowToken,
    id: SessionId,
    b: &mut RegisteredAttachment,
    size: TermSize,
) -> Result<(TerminalProbe, u64)> {
    let mut replay = TerminalProbe::new(size, 128);
    let mut boundary = None;
    let mut history = vec![];
    let mut restore = vec![];
    loop {
        let w = tokio::time::timeout(Duration::from_secs(3), b.wire.recv())
            .await?
            .context("migration bootstrap absent")?;
        let frame = decode(&w)?;
        let p = decode_fragment(frame.payload)?;
        ensure!(p.generation == b.generation, "migration generation mixed");
        match p.part {
            Part::Begin => {
                ensure!(boundary.is_none(), "duplicate migration Begin");
                boundary = Some(decode_begin(p.body)?);
            }
            Part::History => {
                ensure!(
                    p.offset as usize == history.len()
                        && history.len() + p.body.len() <= pulse_ipc::attach::MAX_HISTORY,
                    "history offset/budget"
                );
                history.extend_from_slice(p.body);
            }
            Part::Restore => {
                ensure!(
                    p.offset as usize == restore.len()
                        && restore.len() + p.body.len() <= pulse_ipc::attach::MAX_RESTORE,
                    "restore offset/budget"
                );
                restore.extend_from_slice(p.body);
            }
            Part::Commit => {
                let (before, h, s) = boundary.context("migration missing Begin")?;
                ensure!(
                    h == history.len()
                        && s == restore.len()
                        && before.columns == size.columns()
                        && before.rows == size.rows(),
                    "migration capture lengths/geometry"
                );
                replay.feed(&history);
                replay.feed(&restore);
                replay.take_replies();
                r.acknowledge(o, id, b.generation, frame.sequence)?;
                return Ok((replay, before.parsed_bytes));
            }
            _ => anyhow::bail!("migration live before Commit"),
        };
        r.acknowledge(o, id, b.generation, frame.sequence)?;
    }
}
pub(super) fn validate(v: &serde_json::Value) -> Result<()> {
    let n = |k: &str| {
        v[k].as_u64()
            .context("migration evidence missing/noninteger")
    };
    let (source, target, capture, final_bytes, frames) = (
        n("sourceGeneration")?,
        n("targetGeneration")?,
        n("captureSourceBytes")?,
        n("finalSourceBytes")?,
        n("liveFrames")?,
    );
    ensure!(
        n("spawnedFixedChildren")? == 1
            && n("trustedOwnerIncarnations")? == 2
            && source > 0
            && target > source
            && capture > 0
            && capture < final_bytes
            && final_bytes < 1024 * 1024
            && n("nativeReadBytes")? == final_bytes
            && (2..=16384).contains(&frames)
            && n("nonzeroExitCode")? == 7,
        "native migration evidence invalid counters"
    );
    for key in [
        "sourceInputAndAckRejected",
        "sourceWindowCloseKeptSessionAlive",
        "targetCaptureAndFinalScreenExact",
        "targetUtf8InputValidatedByChild",
        "oldCompletionDidNotRetireNewGeneration",
        "orderedEofAcked",
        "directChildAndNativeWorkersJoined",
    ] {
        ensure!(v[key] == true, "native migration evidence incomplete {key}");
    }
    Ok(())
}

pub(super) async fn run() -> Result<serde_json::Value> {
    let size = TermSize::new(80, 24)?;
    let (handle, task) =
        pulse_app::spawn_local(command("portable-pty-helper-interactive")?, size, 128).await?;
    let _guard = Guard(handle.clone());
    visible(&handle, "PORTABLE_READY").await?;
    let r = SessionRegistry::new();
    let source = r.open_window()?;
    let target = r.open_window()?;
    let id = r.register(source, handle.clone())?;
    let cfg = AttachmentConfig {
        fragment: 128,
        frames: 1,
        bytes: 256,
        ack_timeout: Duration::from_secs(3),
    };
    let mut first = r.attach(source, id, cfg).await?;
    let (_, old_offset) = bootstrap(&r, source, id, &mut first, size).await?;
    let before = r.state(source, id)?;
    ensure!(
        old_offset == before.parsed_bytes,
        "initial native boundary mismatch"
    );
    r.transfer(source, target, id)?;
    ensure!(
        r.state(source, id).is_err()
            && r.acknowledge(source, id, first.generation, 0).is_err()
            && r.input_bound(source, id, first.generation, b"denied\r".to_vec())
                .await
                .is_err(),
        "original PTY owner still authorized"
    );
    r.close_window(source)?;
    ensure!(
        r.state(target, id)?.lifecycle == Lifecycle::Running,
        "old window closed migrated child"
    );
    let mut second = r.attach(target, id, cfg).await?;
    ensure!(
        second.generation > first.generation,
        "target reused migration generation"
    );
    let (mut replay, mut offset) = bootstrap(&r, target, id, &mut second, size).await?;
    ensure!(
        offset == old_offset && replay.screen() == handle.inspect().await?,
        "target restore changed initial native screen"
    );
    let old_completion = tokio::time::timeout(Duration::from_secs(3), first.completion).await??;
    ensure!(
        old_completion.is_err(),
        "old finite PTY binding unexpectedly completed as EOF"
    );
    r.check_binding(target, id, second.generation)?;
    let input = "literal ; $(input) 中\r";
    r.input_bound(target, id, second.generation, input.as_bytes().to_vec())
        .await?;
    let (mut ended, mut frames) = (false, 0);
    tokio::time::timeout(Duration::from_secs(8), async {
        while let Some(w) = second.wire.recv().await {
            let frame = decode(&w)?;
            let p = decode_fragment(frame.payload)?;
            ensure!(
                p.generation == second.generation,
                "old native frame reached new owner"
            );
            match p.part {
                Part::Data => {
                    ensure!(p.offset == offset, "native migration byte gap");
                    replay.feed(p.body);
                    replay.take_replies();
                    offset += p.body.len() as u64;
                }
                Part::Marker => ensure!(p.offset == offset, "migration marker gap"),
                Part::Checkpoint => {
                    ensure!(p.offset == offset, "migration flush gap");
                    let deadline = replay
                        .sync_deadline()
                        .context("checkpoint without synchronized update")?;
                    ensure!(
                        replay.expire_sync(deadline),
                        "checkpoint failed to expire synchronized update"
                    );
                }
                Part::End => {
                    ensure!(
                        !ended && p.offset == offset,
                        "native migration EOF mismatch"
                    );
                    replay.finish_input();
                    ended = true;
                }
                _ => anyhow::bail!("unexpected native migration transaction"),
            };
            r.acknowledge(target, id, second.generation, frame.sequence)?;
            frames += 1;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let receipt = second.completion.await??;
    let (final_state, screen, exit) =
        tokio::time::timeout(Duration::from_secs(3), task.join_with_screen()).await??;
    ensure!(
        ended
            && final_state.lifecycle == Lifecycle::Ended
            && exit.exit_code == 7
            && !exit.cancelled
            && offset == final_state.parsed_bytes
            && exit.read_bytes == offset
            && receipt.boundary == final_state
            && replay.screen() == screen,
        "native migrated child output/exit/EOF/renderer mismatch"
    );
    let text = screen
        .cells
        .iter()
        .flatten()
        .map(|c| c.text.as_str())
        .collect::<String>();
    ensure!(
        text.contains("PORTABLE_OK 中"),
        "migrated PTY did not receive target UTF8 input"
    );
    r.reap_closed(id)?;
    Ok(
        json!({"spawnedFixedChildren":1,"trustedOwnerIncarnations":2,"sourceGeneration":first.generation,"targetGeneration":second.generation,"captureSourceBytes":old_offset,"finalSourceBytes":offset,"nativeReadBytes":exit.read_bytes,"liveFrames":frames,"sourceInputAndAckRejected":true,"sourceWindowCloseKeptSessionAlive":true,"targetCaptureAndFinalScreenExact":true,"targetUtf8InputValidatedByChild":true,"oldCompletionDidNotRetireNewGeneration":true,"orderedEofAcked":true,"nonzeroExitCode":7,"directChildAndNativeWorkersJoined":true,"limits":["Trusted registry window tokens, not actual WebView cross-window migration/drag UI","One real child spawn through the same LocalSessionTask; no public PID identity API claim","Native input already admitted cannot be rolled back"]}),
    )
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn evidence() -> serde_json::Value {
        json!({"spawnedFixedChildren":1,"trustedOwnerIncarnations":2,"sourceGeneration":1,"targetGeneration":2,"captureSourceBytes":19,"finalSourceBytes":63,"nativeReadBytes":63,"liveFrames":3,"sourceInputAndAckRejected":true,"sourceWindowCloseKeptSessionAlive":true,"targetCaptureAndFinalScreenExact":true,"targetUtf8InputValidatedByChild":true,"oldCompletionDidNotRetireNewGeneration":true,"orderedEofAcked":true,"nonzeroExitCode":7,"directChildAndNativeWorkersJoined":true})
    }
    #[test]
    fn migration_report_rejects_missing_actions_and_false_receipts() {
        let v = evidence();
        assert!(validate(&v).is_ok());
        for key in [
            "sourceInputAndAckRejected",
            "sourceWindowCloseKeptSessionAlive",
            "targetCaptureAndFinalScreenExact",
            "targetUtf8InputValidatedByChild",
            "oldCompletionDidNotRetireNewGeneration",
            "orderedEofAcked",
            "directChildAndNativeWorkersJoined",
        ] {
            let mut wrong = v.clone();
            wrong.as_object_mut().unwrap().remove(key);
            assert!(validate(&wrong).is_err());
            wrong[key] = false.into();
            assert!(validate(&wrong).is_err());
        }
    }
    #[test]
    fn migration_report_rejects_two_children_and_mismatched_boundaries() {
        for (key, value) in [
            ("spawnedFixedChildren", 2),
            ("trustedOwnerIncarnations", 1),
            ("targetGeneration", 1),
            ("captureSourceBytes", 64),
            ("nativeReadBytes", 62),
            ("nonzeroExitCode", 0),
            ("liveFrames", 0),
        ] {
            let mut wrong = evidence();
            wrong[key] = value.into();
            assert!(validate(&wrong).is_err(), "{key}");
        }
        let mut wrong = evidence();
        wrong["targetGeneration"] = serde_json::Value::from(2.5);
        assert!(validate(&wrong).is_err());
    }
}
