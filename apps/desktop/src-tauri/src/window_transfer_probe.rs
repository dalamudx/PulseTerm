//! Fixed backend-only peer→main ownership test; wire consumed by Rust, not DOM.
use super::*;
use pulse_app::registry::RegisteredAttachment;
use pulse_ipc::{FrameKind, attach::Part};
use pulse_term::TerminalProbe;
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err("session.peer_transfer_invalid".into())
    }
}
pub(super) async fn capture(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    handle: &SessionHandle,
) -> Result<RegisteredAttachment> {
    let expected = state.0.registry.state(owner, id).map_err(error)?;
    require(
        expected.source_is_utf8
            && expected.parsed_bytes == expected.utf8_bytes
            && expected.lifecycle == Lifecycle::Running,
    )?;
    let mut view = state
        .0
        .attach_codec(
            owner,
            id,
            AttachmentConfig {
                fragment: 8192,
                frames: 1,
                bytes: 8192 + if state.0.dual { 28 } else { 20 },
                ack_timeout: Duration::from_secs(3),
            },
        )
        .await
        .map_err(error)?;
    let generation = view.generation;
    let mut begin = None;
    let mut history = Vec::new();
    let mut restore = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let wire = view.wire.recv().await.ok_or("session.peer_transfer_wire")?;
            let frame = pulse_ipc::decode(&wire).map_err(error)?;
            require(frame.kind == FrameKind::Attachment)?;
            let (part, wire_generation, offset, body) = if state.0.dual {
                let p = pulse_ipc::attach_v2::decode_fragment(frame.payload).map_err(error)?;
                (p.part, p.generation, p.offset, p.body)
            } else {
                let p = pulse_ipc::attach::decode_fragment(frame.payload).map_err(error)?;
                (p.part, p.generation, p.offset, p.body)
            };
            require(wire_generation == generation)?;
            match part {
                Part::Begin => {
                    require(begin.is_none())?;
                    let (h, s) = if state.0.dual {
                        let (b, h, s) = pulse_ipc::attach_v2::decode_begin(body).map_err(error)?;
                        require(
                            b.columns == expected.size.columns()
                                && b.rows == expected.size.rows()
                                && b.revision == expected.revision
                                && b.source_bytes == expected.parsed_bytes
                                && b.utf8_bytes == expected.utf8_bytes,
                        )?;
                        (h, s)
                    } else {
                        let (b, h, s) = pulse_ipc::attach::decode_begin(body).map_err(error)?;
                        require(
                            b.columns == expected.size.columns()
                                && b.rows == expected.size.rows()
                                && b.revision == expected.revision
                                && b.parsed_bytes == expected.parsed_bytes,
                        )?;
                        (h, s)
                    };
                    begin = Some((h, s));
                }
                Part::History => {
                    require(
                        begin.is_some()
                            && offset == history.len() as u64
                            && history.len() + body.len() <= pulse_ipc::attach::MAX_HISTORY,
                    )?;
                    history.extend_from_slice(body);
                }
                Part::Restore => {
                    require(
                        begin.is_some()
                            && offset == restore.len() as u64
                            && restore.len() + body.len() <= pulse_ipc::attach::MAX_RESTORE,
                    )?;
                    restore.extend_from_slice(body);
                }
                Part::Commit => {
                    require(begin == Some((history.len(), restore.len())) && !restore.is_empty())?;
                    let mut replay = TerminalProbe::new(expected.size, 128);
                    replay.feed(&history);
                    replay.feed(&restore);
                    replay.take_replies();
                    require(
                        replay.screen() == handle.inspect().await.map_err(error)?
                            && state.0.registry.state(owner, id).map_err(error)? == expected,
                    )?;
                    state
                        .0
                        .registry
                        .acknowledge(owner, id, generation, frame.sequence)
                        .map_err(error)?;
                    return Ok::<(), String>(());
                }
                _ => return Err("session.peer_transfer_unexpected_live".into()),
            }
            state
                .0
                .registry
                .acknowledge(owner, id, generation, frame.sequence)
                .map_err(error)?;
        }
    })
    .await
    .map_err(|_| "session.peer_transfer_capture_deadline".to_string())??;
    Ok(view)
}
pub(super) async fn handoff(
    state: &WindowSessions,
    target: &WebviewWindow,
    source: &WebviewWindow,
    target_owner: WindowToken,
    source_owner: WindowToken,
    id: SessionId,
    handle: &SessionHandle,
) -> Result<u64> {
    require(state.caller(target)? == target_owner && state.caller(source)? == source_owner)?;
    eprintln!("session.peer_transfer.source_capture");
    let old = capture(state, source_owner, id, handle).await?;
    eprintln!("session.peer_transfer.source_committed");
    require(state.0.registry.state(target_owner, id).is_err())?;
    state
        .0
        .registry
        .transfer(source_owner, target_owner, id)
        .map_err(error)?;
    require(
        state.0.registry.state(source_owner, id).is_err()
            && state
                .0
                .registry
                .acknowledge(source_owner, id, old.generation, 0)
                .is_err()
            && state
                .0
                .registry
                .input_bound(source_owner, id, old.generation, b"denied\n".to_vec())
                .await
                .is_err(),
    )?;
    eprintln!("session.peer_transfer.target_capture");
    let next = capture(state, target_owner, id, handle).await?;
    eprintln!("session.peer_transfer.target_committed");
    require(next.generation > old.generation)?;
    let old_receipt = tokio::time::timeout(Duration::from_secs(3), old.completion)
        .await
        .map_err(error)?
        .map_err(error)?;
    require(matches!(
        old_receipt,
        Err(pulse_app::attachment::AttachmentError::Detached
            | pulse_app::attachment::AttachmentError::Invalidated(
                pulse_term::session::StreamEnd::Replaced
            ))
    ))?;
    state
        .0
        .registry
        .check_binding(target_owner, id, next.generation)
        .map_err(error)?;
    // Move only ownership: session/PTY are still live. Release this Rust-only view;
    // actual target-side input and another exact capture follow source Destroyed.
    state
        .0
        .registry
        .detach(target_owner, id, next.generation)
        .map_err(error)?;
    let completion = tokio::time::timeout(Duration::from_secs(3), next.completion)
        .await
        .map_err(error)?
        .map_err(error)?;
    require(matches!(
        completion,
        Err(pulse_app::attachment::AttachmentError::Detached)
    ))?;
    Ok(next.generation)
}
pub(super) async fn after_source_destroyed(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    handle: &SessionHandle,
    previous: u64,
    channel: &Channel<RawFrame>,
) -> Result<()> {
    eprintln!("session.peer_transfer.source_destroyed");
    let before = state.0.registry.state(owner, id).map_err(error)?;
    let binding = capture(state, owner, id, handle).await?;
    require(binding.generation > previous)?;
    state
        .0
        .registry
        .input_bound(owner, id, binding.generation, b"hidden\n".to_vec())
        .await
        .map_err(error)?;
    state
        .0
        .registry
        .detach(owner, id, binding.generation)
        .map_err(error)?;
    let result = tokio::time::timeout(Duration::from_secs(3), binding.completion)
        .await
        .map_err(error)?
        .map_err(error)?;
    require(matches!(
        result,
        Err(pulse_app::attachment::AttachmentError::Detached)
    ))?;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let screen = handle.inspect().await.map_err(error)?;
            let text = screen
                .cells
                .iter()
                .flatten()
                .map(|c| c.text.as_str())
                .collect::<String>();
            if text.contains("HIDDEN_SEED 中文🌍") {
                require(
                    state
                        .0
                        .registry
                        .state(owner, id)
                        .map_err(error)?
                        .parsed_bytes
                        > before.parsed_bytes,
                )?;
                return Ok::<(), String>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "session.peer_transfer_input_deadline".to_string())??;
    eprintln!("session.peer_transfer.after_input_capture");
    transfer_view::render(state, owner, id, binding.generation, channel).await?;
    Ok(())
}
