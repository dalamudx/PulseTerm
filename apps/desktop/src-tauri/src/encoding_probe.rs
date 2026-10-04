//! Fixed, classified GBK fixture only. Real actor/registry/Channel, no remote transport.
#[cfg(test)]
#[path = "encoding_probe_tests.rs"]
mod tests;
use super::*;
#[cfg(feature = "encoding-probe")]
use pulse_ipc::{attach::Part, attach_v2::decode_fragment, decode};
#[cfg(feature = "encoding-probe")]
use pulse_term::session::{ChannelTransport, StreamEnd, TransportWrite, spawn_gbk_text_session};
#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EncodingEvidence {
    pub generation: String,
    pub source_bytes: String,
    pub utf8_bytes: String,
    pub revision: String,
    pub digest: String,
    pub frames: u32,
    pub peak_credit_bytes: u32,
    pub replacements: u8,
    pub reverse_input_checked: bool,
    pub source_only_marker: bool,
    pub dom_rows_checked: u32,
    pub history_checked: bool,
    pub callback_ack_checked: bool,
}
impl EncodingEvidence {
    pub(crate) fn validate(&self) -> Result<()> {
        parse_u64(&self.generation)?;
        parse_u64(&self.revision)?;
        if parse_counter(&self.source_bytes)? != 89
            || parse_counter(&self.utf8_bytes)? != 91
            || self.digest.len() != 16
            || !self
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.frames < 4
            || self.frames > 16384
            || self.peak_credit_bytes == 0
            || self.peak_credit_bytes > 64
            || self.replacements != 1
            || !self.reverse_input_checked
            || !self.source_only_marker
            || self.dom_rows_checked != 12
            || !self.history_checked
            || !self.callback_ack_checked
        {
            return Err("encoding.invalid_evidence".into());
        }
        Ok(())
    }
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_encoding_mode(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
) -> Result<bool> {
    state.caller(&window)?;
    Ok(state.0.encoding && cfg!(feature = "encoding-probe"))
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_encoding_input(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    text: String,
) -> Result<()> {
    if !state.0.encoding
        || !cfg!(feature = "encoding-probe")
        || !matches!(text.as_str(), "中文" | "确认")
    {
        return Err("encoding.fixed_input_only".into());
    }
    let (o, id) = state.owned(&window, &nonce)?;
    state
        .0
        .registry
        .input_bound(o, id, parse_u64(&generation)?, text.into_bytes())
        .await
        .map_err(error)
}
#[cfg(feature = "encoding-probe")]
async fn input(driver: &mut pulse_term::session::DriverEndpoint, expected: &[u8]) -> Result<()> {
    match tokio::time::timeout(Duration::from_secs(5), driver.input.recv())
        .await
        .map_err(|_| "encoding.input_deadline".to_string())?
    {
        Some(TransportWrite::Input(bytes)) if bytes == expected => Ok(()),
        _ => Err("encoding.reverse_input_failed".into()),
    }
}
#[cfg(feature = "encoding-probe")]
async fn bootstrap(
    binding: &mut pulse_app::registry::RegisteredAttachment,
    channel: &Channel<RawFrame>,
) -> Result<()> {
    loop {
        let wire = tokio::time::timeout(Duration::from_secs(5), binding.wire.recv())
            .await
            .map_err(error)?
            .ok_or("encoding.capture_incomplete")?;
        let commit = decode_fragment(decode(&wire).map_err(error)?.payload)
            .map_err(error)?
            .part
            == Part::Commit;
        channel.send(RawFrame(wire)).map_err(error)?;
        if commit {
            return Ok(());
        }
    }
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_encoding(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    channel: Channel<RawFrame>,
) -> Result<EncodingEvidence> {
    #[cfg(feature = "encoding-probe")]
    {
        run(window, state, nonce, channel).await
    }
    #[cfg(not(feature = "encoding-probe"))]
    {
        let _ = (window, state, nonce, channel);
        Err("encoding.validation_feature_required".into())
    }
}
#[cfg(feature = "encoding-probe")]
async fn run(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    channel: Channel<RawFrame>,
) -> Result<EncodingEvidence> {
    if !state.0.encoding {
        return Err("encoding.mode_required".into());
    }
    let nonce = uuid::Uuid::parse_str(&nonce).map_err(error)?.to_string();
    let state = state.inner().clone();
    let owner = state.caller(&window)?;
    let _slot = state.0.slot.clone().try_acquire_owned().map_err(error)?;
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let (handle, task) =
        spawn_gbk_text_session(TermSize::new(40, 12).map_err(error)?, 128, transport);
    let id = state
        .0
        .registry
        .register(owner, handle.clone())
        .map_err(error)?;
    let _guard = RunGuard {
        state: state.clone(),
        owner,
        id,
        handle: handle.clone(),
    };
    *state.0.active.lock().map_err(error)? = Some(Active {
        owner,
        nonce,
        id,
        peer: None,
        visibility: Arc::new(crate::viewer_visibility::VisibilityGate::default()),
    });
    let mut old = handle
        .diagnostic_subscribe(1, 4 * 1024 * 1024, 1024 * 1024)
        .await
        .map_err(error)?;
    let mut prefix = b"row\r\n".repeat(15);
    prefix.push(0xd6);
    driver.output.send(prefix).await.map_err(error)?;
    tokio::time::timeout(Duration::from_secs(5), old.output.recv())
        .await
        .map_err(error)?
        .ok_or("encoding.prefix_missing")?;
    let config = AttachmentConfig {
        fragment: 7,
        frames: 1,
        bytes: 64,
        ack_timeout: Duration::from_secs(5),
    };
    let mut first = state
        .0
        .registry
        .attach_v2(owner, id, config)
        .await
        .map_err(error)?;
    if old.status.borrow().end != StreamEnd::Replaced {
        return Err("encoding.old_lease_not_replaced".into());
    }
    bootstrap(&mut first, &channel).await?;
    input(&mut driver, &[0xd6, 0xd0, 0xce, 0xc4]).await?;
    let mut next = state
        .0
        .registry
        .attach_v2(owner, id, config)
        .await
        .map_err(error)?;
    if next.generation <= first.generation
        || state
            .0
            .registry
            .acknowledge(owner, id, first.generation, 0)
            .is_ok()
    {
        return Err("encoding.stale_ack_admitted".into());
    }
    if first.completion.await.map_err(error)?.is_ok() {
        return Err("encoding.old_binding_not_revoked".into());
    }
    bootstrap(&mut next, &channel).await?;
    input(&mut driver, &[0xc8, 0xb7, 0xc8, 0xcf]).await?;
    // Complete the old lead, then expose a source-only lead in the LIVE queue.
    driver.output.send(vec![0xd0]).await.map_err(error)?;
    let mut observed = handle.state.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        while observed.borrow().parsed_bytes != 77 {
            observed.changed().await.map_err(error)?;
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(error)??;
    driver.output.send(vec![0xce]).await.map_err(error)?;
    let mut marker = false;
    loop {
        let wire = tokio::time::timeout(Duration::from_secs(5), next.wire.recv())
            .await
            .map_err(error)?
            .ok_or("encoding.marker_missing")?;
        let f = decode(&wire).map_err(error)?;
        let p = decode_fragment(f.payload).map_err(error)?;
        if p.part == Part::Marker {
            marker = true;
        }
        channel.send(RawFrame(wire)).map_err(error)?;
        if marker {
            break;
        }
    }
    driver
        .output
        .send(vec![
            0xc4, b'\r', b'\n', b'G', b'B', b'K', b'_', b'O', b'K', b'\r', b'\n',
        ])
        .await
        .map_err(error)?;
    drop(driver);
    while let Some(wire) = tokio::time::timeout(Duration::from_secs(5), next.wire.recv())
        .await
        .map_err(error)?
    {
        channel.send(RawFrame(wire)).map_err(error)?;
    }
    let receipt = next.completion.await.map_err(error)?.map_err(error)?;
    let (final_state, screen) =
        tokio::time::timeout(Duration::from_secs(5), task.join_with_screen())
            .await
            .map_err(error)?
            .map_err(error)?;
    if final_state.lifecycle != Lifecycle::Ended
        || final_state != receipt.boundary
        || final_state.parsed_bytes != 89
        || final_state.utf8_bytes != 91
    {
        return Err("encoding.final_boundary_mismatch".into());
    }
    Ok(EncodingEvidence {
        generation: next.generation.to_string(),
        source_bytes: final_state.parsed_bytes.to_string(),
        utf8_bytes: final_state.utf8_bytes.to_string(),
        revision: final_state.revision.to_string(),
        digest: pulse_term::reconciliation::screen_digest(&screen),
        frames: u32::try_from(receipt.frames).map_err(error)?,
        peak_credit_bytes: u32::try_from(receipt.peak_credit_bytes).map_err(error)?,
        replacements: 1,
        reverse_input_checked: true,
        source_only_marker: marker,
        dom_rows_checked: 0,
        history_checked: false,
        callback_ack_checked: false,
    })
}
