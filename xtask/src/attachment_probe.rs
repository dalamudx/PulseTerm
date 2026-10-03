//! Shared binary receiver + real headless xterm rendering; not WebView/PTY attach.
use anyhow::{Result, ensure};
use pulse_core::TermSize;
use pulse_ipc::{
    attach::{AttachmentSender, Boundary},
    decode,
};
use pulse_term::TerminalProbe;
use serde_json::json;
use std::fs;
async fn actor_case(columns: u16, rows: u16) -> Result<serde_json::Value> {
    use pulse_term::session::{
        ChannelTransport, Lifecycle, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES, StreamRecord,
        spawn_session,
    };
    use tokio::time::{Duration, timeout};
    let size = TermSize::new(columns, rows)?;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(size, 128, transport);
    let mut prefix = Vec::new();
    for row in 0..20 {
        prefix.extend_from_slice(format!("row-{row:02}\r\n").as_bytes());
    }
    prefix.extend_from_slice(b"main\x1b[?1049halt\xf0\x9f");
    driver.output.send(prefix.clone()).await?;
    timeout(Duration::from_secs(2), async {
        let mut state = handle.state.clone();
        while state.borrow().parsed_bytes < prefix.len() as u64 {
            state.changed().await?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let mut stream = handle
        .diagnostic_subscribe(10, MAX_HISTORY_BYTES, MAX_RESTORE_BYTES)
        .await?;
    let mut source = TerminalProbe::new(size, 128);
    source.feed(&prefix);
    let history = source.recent_history_projection(stream.bootstrap.history.retained_rows);
    let retained = stream.bootstrap.history.retained_rows;
    let total = stream.bootstrap.history.total_rows;
    let boundary = |state: &pulse_term::session::SessionState| Boundary {
        columns: state.size.columns(),
        rows: state.size.rows(),
        revision: state.revision,
        parsed_bytes: state.parsed_bytes,
    };
    let generation = 0x100000000000007;
    let mut sender = AttachmentSender::new(
        generation,
        boundary(&stream.bootstrap.boundary),
        stream.bootstrap.history.bytes,
        stream.bootstrap.restore,
        7,
        1,
        64,
    )?;
    let mut frames = Vec::new();
    while let Some(frame) = sender.next_frame()? {
        sender.acknowledge(generation, decode(&frame)?.sequence)?;
        frames.push(frame);
    }
    let mut suffix = Vec::new();
    for bytes in [
        b"\x98\x80!\x1b[?1049lZ".as_slice(),
        b"\x1b[?2026hSYNC".as_slice(),
    ] {
        driver.output.send(bytes.to_vec()).await?;
        suffix.extend_from_slice(bytes);
        let record = timeout(Duration::from_secs(2), stream.output.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("actor tail disappeared"))?;
        let StreamRecord::Data {
            start,
            bytes,
            boundary: state,
        } = record
        else {
            anyhow::bail!("actor Data expected");
        };
        ensure!(
            start == state.parsed_bytes - bytes.len() as u64,
            "actor byte cursor inconsistent"
        );
        for chunk in bytes.chunks(2) {
            let frame = sender.data(chunk)?;
            sender.acknowledge(generation, decode(&frame)?.sequence)?;
            frames.push(frame);
        }
    }
    let record = timeout(Duration::from_secs(2), stream.output.recv())
        .await?
        .ok_or_else(|| anyhow::anyhow!("missing sync fence"))?;
    let StreamRecord::Fence(state) = record else {
        anyhow::bail!("expected sync timeout fence");
    };
    let frame = sender.checkpoint(boundary(&state))?;
    sender.acknowledge(generation, decode(&frame)?.sequence)?;
    frames.push(frame);
    let expected = handle.inspect().await?;
    let digest = handle.diagnostic_digest().await?;
    ensure!(digest.boundary == state, "fence/capture boundary drift");
    let checkpoints = vec![
        json!({"revision":state.revision.to_string(),"parsedBytes":state.parsed_bytes.to_string(),"expected":expected}),
    ];
    handle.close();
    ensure!(
        task.join().await?.lifecycle == Lifecycle::Closed,
        "actor not closed"
    );
    Ok(
        json!({"name":"actor-atomic-bootstrap-live-fence","prefix":prefix,"suffix":suffix,"frames":frames,"history":history,"expected":expected,"retainedRows":retained,"totalRows":total,"checkpoints":checkpoints}),
    )
}
pub fn run() -> Result<()> {
    let columns = 32;
    let rows = 8;
    let mut cases = Vec::new();
    for (name, special, suffix) in [
        ("primary-utf8-tail", "MAIN 😀中", vec![b'!']),
        (
            "alt-partial-csi",
            "MAIN\x1b[?1049hALT\x1b[31",
            b"m!\x1b[?1049lZ".to_vec(),
        ),
        ("primary-soft-history", "MAIN", b"\r\nNEXT".to_vec()),
    ] {
        let mut source = TerminalProbe::new(TermSize::new(columns, rows)?, 128);
        let mut prefix = Vec::new();
        for row in 0..20 {
            let text = if name == "primary-soft-history" {
                format!("\x1b[32mlong-{row:02}-{}\x1b[0m\r\n", "q".repeat(61))
            } else {
                format!("\x1b[38;2;1;2;3mrow-{row:02} 中\x1b[0m\r\n")
            };
            prefix.extend_from_slice(text.as_bytes());
        }
        prefix.extend_from_slice(special.as_bytes());
        if name == "primary-utf8-tail" {
            prefix.extend_from_slice(&[0xf0, 0x9f]);
        }
        source.feed(&prefix);
        let history = source
            .recent_history_bounded(10, 4 * 1024 * 1024)
            .map_err(|gap| anyhow::anyhow!("{name}: history {gap:?}"))?;
        let restore = source
            .restore_screen_bounded(1024 * 1024)
            .map_err(|gap| anyhow::anyhow!("{name}: restore {gap:?}"))?;
        ensure!(
            restore.approximations.is_empty(),
            "known approximation: {name}"
        );
        let expected_history = source.recent_history_projection(history.retained_rows);
        let suffix = if name == "primary-utf8-tail" {
            vec![0x98, 0x80, b'!']
        } else {
            suffix
        };
        let mut sender = AttachmentSender::new(
            0x100000000000007,
            Boundary {
                columns,
                rows,
                revision: 5,
                parsed_bytes: prefix.len() as u64,
            },
            history.bytes,
            restore.bytes,
            7,
            1,
            64,
        )?;
        let mut frames = Vec::new();
        while let Some(wire) = sender.next_frame()? {
            let sequence = decode(&wire)?.sequence;
            frames.push(wire);
            sender.acknowledge(0x100000000000007, sequence)?;
        }
        for part in suffix.chunks(2) {
            let wire = sender.data(part)?;
            let sequence = decode(&wire)?.sequence;
            frames.push(wire);
            sender.acknowledge(0x100000000000007, sequence)?;
        }
        source.feed(&suffix);
        cases.push(json!({"name":name,"prefix":prefix,"suffix":suffix,"frames":frames,"history":expected_history,"expected":source.screen(),"retainedRows":history.retained_rows,"totalRows":history.total_rows}));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    cases.push(runtime.block_on(actor_case(columns, rows))?);
    fs::create_dir_all("target/probes")?;
    fs::write(
        "target/probes/attachment.json",
        serde_json::to_vec_pretty(&json!({"columns":columns,"rows":rows,"cases":cases}))?,
    )?;
    println!(
        "Prepared 4 binary attachment cases: primary/alternate, history colours/soft wraps, UTF-8/CSI tails"
    );
    Ok(())
}
