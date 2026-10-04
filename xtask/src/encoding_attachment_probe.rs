//! Actual actor/decoder/lease/credit, then cross-language headless rendering.
use anyhow::{Result, ensure};
use pulse_app::attachment::{AttachmentConfig, attach, attach_v2};
use pulse_core::TermSize;
use pulse_ipc::{
    attach::Part,
    attach_v2::{decode_begin, decode_fragment},
    decode,
};
use pulse_term::session::{ChannelTransport, Lifecycle, StreamEnd, spawn_gbk_text_session};
use serde_json::json;
use std::time::Duration;
async fn case(history: bool) -> Result<serde_json::Value> {
    let size = TermSize::new(32, 8)?;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_gbk_text_session(size, 128, transport);
    let mut old = handle
        .diagnostic_subscribe(2000, 4 * 1024 * 1024, 1024 * 1024)
        .await?;
    let mut prefix = if history {
        b"row\r\n".repeat(15)
    } else {
        vec![]
    };
    prefix.push(0xd6);
    for piece in prefix.chunks(7) {
        driver.output.send(piece.to_vec()).await?;
        let _ = tokio::time::timeout(Duration::from_secs(1), old.output.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("source prefix record absent"))?;
    }
    let capture = handle.state.borrow().clone();
    ensure!(
        capture.parsed_bytes == prefix.len() as u64
            && capture.utf8_bytes == prefix.len() as u64 - 1,
        "pending GBK boundary"
    );
    ensure!(
        attach(&handle, 1, AttachmentConfig::default())
            .await
            .is_err(),
        "v1 accepted normalized mode"
    );
    ensure!(
        old.status.borrow().end == StreamEnd::Open,
        "v1 refusal changed old lease"
    );
    let generation = (1u64 << 60) + 7;
    let config = AttachmentConfig {
        fragment: 7,
        frames: 1,
        bytes: 64,
        ack_timeout: Duration::from_secs(2),
    };
    let (ack, mut wire, binding) = attach_v2(&handle, generation, config).await?;
    ensure!(
        old.status.borrow().end == StreamEnd::Replaced,
        "v2 capture did not commit replacement"
    );
    let mut frames = Vec::new();
    let mut begin = None;
    let mut restore_reached = false;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(frame) = wire.recv().await {
            let f = decode(&frame)?;
            let p = decode_fragment(f.payload)?;
            if p.part == Part::Begin {
                begin = Some(decode_begin(p.body)?.0);
            }
            let committed = p.part == Part::Commit;
            ack.acknowledge(generation, f.sequence)?;
            frames.push(frame);
            if committed {
                restore_reached = true;
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    ensure!(
        restore_reached
            && begin
                .is_some_and(|b| b.source_bytes == capture.parsed_bytes
                    && b.utf8_bytes == capture.utf8_bytes),
        "wrong captured coordinates"
    );
    driver
        .output
        .send(vec![0xd0, 0xce, 0xc4, b'!', b'\r', b'\n'])
        .await?;
    drop(driver);
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(frame) = wire.recv().await {
            let sequence = decode(&frame)?.sequence;
            ack.acknowledge(generation, sequence)?;
            frames.push(frame);
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let receipt = binding.join().await?;
    let (final_state, expected) = task.join_with_screen().await?;
    ensure!(
        final_state.lifecycle == Lifecycle::Ended && receipt.boundary == final_state,
        "v2 EOF receipt differs from actor"
    );
    ensure!(
        final_state.parsed_bytes == prefix.len() as u64 + 6
            && final_state.utf8_bytes == prefix.len() as u64 - 1 + 9,
        "source versus display final counters"
    );
    Ok(
        json!({"name":if history{"gbk-history-pending-reattach"}else{"gbk-pending-reattach"},"generation":generation.to_string(),"frames":frames,"expected":expected,"sourceBytes":final_state.parsed_bytes.to_string(),"utf8Bytes":final_state.utf8_bytes.to_string(),"sourceOnlyPrefix":true,"v1RefusalPreservedOldLease":true,"v2AtomicReplacement":true,"orderedEofAcked":true,"peakCreditBytes":receipt.peak_credit_bytes}),
    )
}
pub fn run() -> Result<()> {
    let root = super::root()?;
    let path = root.join("target/probes/encoding-attachment.json");
    std::fs::create_dir_all(root.join("target/probes"))?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let cases = runtime
        .block_on(async { Ok::<_, anyhow::Error>(vec![case(false).await?, case(true).await?]) })?;
    std::fs::write(
        path,
        serde_json::to_vec_pretty(
            &json!({"schemaVersion":2,"columns":32,"rows":8,"cases":cases,"limitations":["Generator ACKs after staging wire; headless callback ACK separately verified in TS","Classified test-only GBK, not production binary preclassification or real WebView encoding"]}),
        )?,
    )?;
    println!("Prepared 2 actual GBK actor/v2 capture/history/EOF cases");
    Ok(())
}
