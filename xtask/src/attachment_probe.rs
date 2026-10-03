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
    fs::create_dir_all("target/probes")?;
    fs::write(
        "target/probes/attachment.json",
        serde_json::to_vec_pretty(&json!({"columns":columns,"rows":rows,"cases":cases}))?,
    )?;
    println!(
        "Prepared 3 binary attachment cases: primary/alternate, history colours/soft wraps, UTF-8/CSI tails"
    );
    Ok(())
}
