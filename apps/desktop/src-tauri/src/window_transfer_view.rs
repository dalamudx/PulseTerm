//! One fixed peer capture delivered to the main WebView. No frontend session/owner selection.
use super::*;
pub(super) struct Current {
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    confirm: Option<oneshot::Sender<()>>,
    commit_sequence: Option<u32>,
    commit_acknowledged: bool,
    live: bool,
    marker: bool,
    data_end: Option<(u32, u64, bool)>,
}
impl Current {
    fn can_confirm(&self, owner: WindowToken, id: SessionId, generation: u64) -> bool {
        self.owner == owner
            && self.id == id
            && self.generation == generation
            && self.commit_sequence.is_some()
            && self.commit_acknowledged
            && self.confirm.is_some()
    }
}
fn current(state: &WindowSessions, owner: WindowToken, generation: u64) -> Result<SessionId> {
    state
        .0
        .transfer_view
        .lock()
        .map_err(error)?
        .as_ref()
        .filter(|c| {
            c.owner == owner && c.generation == generation && (c.confirm.is_some() || c.live)
        })
        .map(|c| c.id)
        .ok_or("session.transfer_stale".into())
}
struct Guard {
    state: WindowSessions,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.state.0.transfer_view.lock()
            && slot
                .as_ref()
                .is_some_and(|c| c.id == self.id && c.generation == self.generation)
        {
            *slot = None;
        }
        let _ = self
            .state
            .0
            .registry
            .detach(self.owner, self.id, self.generation);
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn probe_transfer_ack(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    sequence: u32,
) -> Result<()> {
    let (owner, _) = state.owned(&window, &nonce)?;
    let generation = parse_u64(&generation)?;
    let id = current(&state, owner, generation)?;
    acknowledge_for(&state, owner, id, generation, sequence)
}
pub(super) fn acknowledge_for(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    sequence: u32,
) -> Result<()> {
    if current(state, owner, generation)? != id {
        return Err("session.transfer_stale".into());
    }
    // Hold metadata through synchronous ACK admission: released credit can wake
    // the next-frame sender immediately on another runtime thread.
    let mut slot = state.0.transfer_view.lock().map_err(error)?;
    let current = slot
        .as_mut()
        .filter(|c| {
            c.owner == owner
                && c.id == id
                && c.generation == generation
                && (c.confirm.is_some() || c.live)
        })
        .ok_or("session.transfer_stale")?;
    state
        .0
        .registry
        .acknowledge(owner, id, generation, sequence)
        .map_err(error)?;
    if let Some((last, _, acknowledged)) = current.data_end.as_mut()
        && *last == sequence
    {
        *acknowledged = true;
    }
    if current.commit_sequence == Some(sequence) {
        current.commit_acknowledged = true;
    }
    Ok(())
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_transfer_confirm(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    rendered: RenderedProjection,
) -> Result<()> {
    let (owner, _) = state.owned(&window, &nonce)?;
    let generation = parse_u64(&generation)?;
    let id = current(&state, owner, generation)?;
    compare_for(&state, owner, id, generation, rendered).await?;
    state.owned(&window, &nonce)?;
    confirm_for(&state, owner, id, generation)
}
pub(super) async fn compare_for(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    rendered: RenderedProjection,
) -> Result<()> {
    if current(state, owner, generation)? != id {
        return Err("session.transfer_stale".into());
    }
    {
        let slot = state.0.transfer_view.lock().map_err(error)?;
        if !slot
            .as_ref()
            .is_some_and(|c| c.can_confirm(owner, id, generation))
        {
            return Err("session.transfer_commit_not_acknowledged".into());
        }
    }
    let rendered = RenderedScreen {
        boundary: SessionState {
            lifecycle: Lifecycle::Running,
            size: TermSize::new(rendered.columns, rendered.rows).map_err(error)?,
            revision: parse_counter(&rendered.revision)?,
            parsed_bytes: parse_counter(&rendered.parsed_bytes)?,
            utf8_bytes: parse_counter(&rendered.parsed_bytes)?,
            source_is_utf8: true,
        },
        digest: rendered.digest,
    };
    if state
        .0
        .registry
        .compare_visible(owner, id, generation, rendered)
        .await
        .map_err(|e| match e {
            pulse_app::registry::RegistryError::Busy => "session.transfer_compare_busy".to_string(),
            _ => "session.transfer_compare_rejected".to_string(),
        })?
        != VisibleComparison::Equal
    {
        return Err("session.transfer_screen_mismatch".into());
    }
    Ok(())
}
pub(super) fn confirm_for(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
) -> Result<()> {
    let mut slot = state.0.transfer_view.lock().map_err(error)?;
    let confirm = slot
        .as_mut()
        .filter(|c| c.owner == owner && c.id == id && c.generation == generation)
        .and_then(|c| c.confirm.take())
        .ok_or("session.transfer_stale")?;
    confirm
        .send(())
        .map_err(|_| "session.transfer_confirm_cancelled".to_string())
}
pub(super) fn confirmation_role(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
) -> Result<(bool, bool)> {
    let slot = state.0.transfer_view.lock().map_err(error)?;
    let c = slot
        .as_ref()
        .filter(|c| {
            c.owner == owner && c.id == id && c.generation == generation && c.confirm.is_some()
        })
        .ok_or("session.transfer_stale")?;
    Ok((c.live, c.marker))
}
pub(super) fn live_generation(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
) -> Result<u64> {
    let slot = state.0.transfer_view.lock().map_err(error)?;
    slot.as_ref()
        .filter(|c| c.owner == owner && c.id == id && c.live)
        .map(|c| c.generation)
        .ok_or("session.peer_no_live_view".into())
}
pub(super) fn live_data_acked(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
) -> Result<Option<u64>> {
    let slot = state.0.transfer_view.lock().map_err(error)?;
    let c = slot
        .as_ref()
        .filter(|c| c.owner == owner && c.id == id && c.generation == generation && c.live)
        .ok_or("session.transfer_stale")?;
    Ok(c.data_end
        .filter(|(_, _, acked)| *acked)
        .map(|(_, end, _)| end))
}

pub(super) async fn render(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    previous: u64,
    channel: &Channel<RawFrame>,
) -> Result<()> {
    render_inner(state, owner, id, previous, channel, None).await
}
pub(super) async fn render_live(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    channel: &Channel<RawFrame>,
    pages: &reload::PageTracker,
    page_nonce: &str,
) -> Result<()> {
    render_inner(state, owner, id, 0, channel, Some((pages, page_nonce))).await
}
async fn render_inner(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    previous: u64,
    channel: &Channel<RawFrame>,
    live: Option<(&reload::PageTracker, &str)>,
) -> Result<()> {
    // Reserve the shared sender lane BEFORE capture can replace a live lease.
    let _permit = state
        .0
        .transfer_view_slot
        .clone()
        .try_acquire_owned()
        .map_err(|_| "session.transfer_busy".to_string())?;
    if state.0.transfer_view.lock().map_err(error)?.is_some() {
        return Err("session.transfer_busy".into());
    }
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
    let _guard = Guard {
        state: state.clone(),
        owner,
        id,
        generation: view.generation,
    };
    if view.generation <= previous {
        return Err("session.transfer_generation".into());
    }
    let (tx, rx) = oneshot::channel();
    {
        let mut slot = state.0.transfer_view.lock().map_err(error)?;
        if slot.is_some() {
            return Err("session.transfer_busy".into());
        }
        *slot = Some(Current {
            owner,
            id,
            generation: view.generation,
            confirm: Some(tx),
            commit_sequence: None,
            commit_acknowledged: false,
            live: live.is_some(),
            marker: false,
            data_end: None,
        });
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let wire = view.wire.recv().await.ok_or("session.transfer_wire")?;
            let frame = pulse_ipc::decode(&wire).map_err(error)?;
            let part = if state.0.dual {
                pulse_ipc::attach_v2::decode_fragment(frame.payload)
                    .map_err(error)?
                    .part
            } else {
                pulse_ipc::attach::decode_fragment(frame.payload)
                    .map_err(error)?
                    .part
            };
            if part == pulse_ipc::attach::Part::Commit {
                let mut slot = state.0.transfer_view.lock().map_err(error)?;
                let current = slot
                    .as_mut()
                    .filter(|c| c.owner == owner && c.id == id && c.generation == view.generation)
                    .ok_or("session.transfer_stale")?;
                current.commit_sequence = Some(frame.sequence);
            }
            channel.send(RawFrame(wire)).map_err(error)?;
            if part == pulse_ipc::attach::Part::Commit {
                break;
            }
        }
        rx.await.map_err(error)?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "session.transfer_view_deadline".to_string())??;
    state
        .0
        .registry
        .check_binding(owner, id, view.generation)
        .map_err(error)?;
    if let Some((pages, page_nonce)) = live {
        tokio::time::timeout(Duration::from_secs(5),async{
            let navigation=pages.navigation_started();tokio::pin!(navigation);
            loop{let wire=tokio::select!{biased;result=&mut navigation=>{result?;break;},wire=view.wire.recv()=>wire};
                let Some(wire)=wire else{if pages.has_navigation_started(){pages.navigation_started().await?;break;}return Err("session.peer_live_wire_closed".to_string());};
                if pages.authorize_view(page_nonce).is_err(){if pages.has_navigation_started(){pages.navigation_started().await?;break;}return Err("session.peer_live_scope_revoked".to_string());}
                let frame=pulse_ipc::decode(&wire).map_err(error)?;
                let(part,offset,length)=if state.0.dual{let p=pulse_ipc::attach_v2::decode_fragment(frame.payload).map_err(error)?;(p.part,p.offset,p.body.len())}else{let p=pulse_ipc::attach::decode_fragment(frame.payload).map_err(error)?;(p.part,p.offset,p.body.len())};
                match part{
                    pulse_ipc::attach::Part::Data=>{let end=offset.checked_add(length as u64).ok_or("session.peer_data_overflow")?;let mut slot=state.0.transfer_view.lock().map_err(error)?;let c=slot.as_mut().filter(|c|c.owner==owner&&c.id==id&&c.generation==view.generation&&c.live).ok_or("session.transfer_stale")?;c.data_end=Some((frame.sequence,end,false));},
                    pulse_ipc::attach::Part::Marker=>{let(tx,rx)=oneshot::channel();{let mut slot=state.0.transfer_view.lock().map_err(error)?;let c=slot.as_mut().filter(|c|c.owner==owner&&c.id==id&&c.generation==view.generation&&c.live&&c.confirm.is_none()&&c.data_end.is_some_and(|(_,_,acked)|acked)).ok_or("session.peer_marker_without_data_ack")?;if c.marker{return Err("session.peer_repeated_live_marker".to_string());}c.marker=true;c.commit_sequence=Some(frame.sequence);c.commit_acknowledged=false;c.confirm=Some(tx);}channel.send(RawFrame(wire)).map_err(error)?;tokio::select!{result=rx=>{result.map_err(error)?;},result=&mut navigation=>{result?;break;}}continue;},
                    _=>return Err("session.peer_unsupported_live_part".to_string()),
                }
                channel.send(RawFrame(wire)).map_err(error)?;
            }
            Ok::<(),String>(())
        }).await.map_err(|_|"session.peer_live_navigation_deadline".to_string())??;
    } else {
        state
            .0
            .registry
            .detach(owner, id, view.generation)
            .map_err(error)?;
    }
    let receipt = tokio::time::timeout(Duration::from_secs(3), view.completion)
        .await
        .map_err(error)?
        .map_err(error)?;
    if !matches!(
        receipt,
        Err(pulse_app::attachment::AttachmentError::Detached)
    ) {
        return Err("session.transfer_unexpected_completion".into());
    }
    Ok(())
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pulse_term::session::{self, ChannelTransport};
    #[tokio::test]
    async fn failed_capture_order_detaches_its_lease_and_releases_sender_credit_without_closing_actor()
     {
        for dual in [false, true] {
            let state = WindowSessions::new(false, None, dual, false);
            let owner = state.0.registry.open_window().unwrap();
            let (transport, driver) = session::ChannelTransport::bounded_driver();
            let (handle, task) =
                session::spawn_session(TermSize::new(40, 12).unwrap(), 128, transport);
            let id = state.0.registry.register(owner, handle).unwrap();
            let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let count = sent.clone();
            let channel = Channel::<RawFrame>::new(move |_| {
                count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            });
            assert_eq!(
                render(&state, owner, id, u64::MAX, &channel)
                    .await
                    .unwrap_err(),
                "session.transfer_generation"
            );
            assert_eq!(sent.load(std::sync::atomic::Ordering::Relaxed), 0);
            assert_eq!(state.0.transfer_view_slot.available_permits(), 1);
            assert!(state.0.transfer_view.lock().unwrap().is_none());
            assert!(state.0.registry.check_binding(owner, id, 1).is_err());
            assert_eq!(
                state.0.registry.state(owner, id).unwrap().lifecycle,
                Lifecycle::Running
            );
            let next = state
                .0
                .attach_codec(
                    owner,
                    id,
                    AttachmentConfig {
                        fragment: 64,
                        frames: 1,
                        bytes: 128,
                        ack_timeout: Duration::from_secs(3),
                    },
                )
                .await
                .unwrap();
            assert_eq!(next.generation, 2);
            state
                .0
                .registry
                .check_binding(owner, id, next.generation)
                .unwrap();
            state.0.registry.close_session(owner, id).unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), task.join())
                    .await
                    .unwrap()
                    .unwrap()
                    .lifecycle,
                Lifecycle::Closed
            );
            drop((next, driver));
        }
    }

    #[tokio::test]
    async fn occupied_sender_lane_rejects_before_capture_without_replacing_or_spending_generation()
    {
        for dual in [false, true] {
            let state = WindowSessions::new(false, None, dual, false);
            let owner = state.0.registry.open_window().unwrap();
            let (transport, driver) = session::ChannelTransport::bounded_driver();
            let (handle, task) =
                session::spawn_session(TermSize::new(40, 12).unwrap(), 128, transport);
            let id = state.0.registry.register(owner, handle).unwrap();
            let config = AttachmentConfig {
                fragment: 64,
                frames: 1,
                bytes: 128,
                ack_timeout: Duration::from_secs(3),
            };
            let first = state.0.attach_codec(owner, id, config).await.unwrap();
            let permit = state
                .0
                .transfer_view_slot
                .clone()
                .try_acquire_owned()
                .unwrap();
            let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let observed = sent.clone();
            let channel = Channel::<RawFrame>::new(move |_| {
                observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            });
            assert_eq!(
                render(&state, owner, id, first.generation, &channel)
                    .await
                    .unwrap_err(),
                "session.transfer_busy"
            );
            assert_eq!(sent.load(std::sync::atomic::Ordering::Relaxed), 0);
            state
                .0
                .registry
                .check_binding(owner, id, first.generation)
                .unwrap();
            assert!(state.0.transfer_view.lock().unwrap().is_none());
            drop(permit);
            let next = state.0.attach_codec(owner, id, config).await.unwrap();
            assert_eq!(next.generation, first.generation + 1);
            state
                .0
                .registry
                .check_binding(owner, id, next.generation)
                .unwrap();
            let _ = tokio::time::timeout(Duration::from_secs(1), first.completion)
                .await
                .unwrap();
            state
                .0
                .registry
                .check_binding(owner, id, next.generation)
                .unwrap();
            state.0.registry.close_session(owner, id).unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), task.join())
                    .await
                    .unwrap()
                    .unwrap()
                    .lifecycle,
                Lifecycle::Closed
            );
            drop((next, driver));
        }
    }

    #[tokio::test]
    async fn live_data_credit_does_not_grant_duplicate_or_unacknowledged_marker_confirmation() {
        let state = WindowSessions::default();
        let owner = state.0.registry.open_window().unwrap();
        let foreign = state.0.registry.open_window().unwrap();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = session::spawn_session(TermSize::new(40, 12).unwrap(), 128, transport);
        let id = state.0.registry.register(owner, handle).unwrap();
        *state.0.transfer_view.lock().unwrap() = Some(Current {
            owner,
            id,
            generation: 17,
            confirm: None,
            commit_sequence: Some(2),
            commit_acknowledged: true,
            live: true,
            marker: false,
            data_end: Some((3, 42, true)),
        });
        assert_eq!(current(&state, owner, 17).unwrap(), id);
        assert!(confirmation_role(&state, owner, id, 17).is_err());
        {
            let slot = state.0.transfer_view.lock().unwrap();
            assert!(!slot.as_ref().unwrap().can_confirm(owner, id, 17));
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut slot = state.0.transfer_view.lock().unwrap();
            let c = slot.as_mut().unwrap();
            c.confirm = Some(tx);
            c.marker = true;
            c.commit_sequence = Some(4);
            c.commit_acknowledged = false;
            assert!(!c.can_confirm(owner, id, 17));
            c.commit_acknowledged = true;
            assert!(c.can_confirm(owner, id, 17));
            assert!(!c.can_confirm(foreign, id, 17));
            assert!(!c.can_confirm(owner, id, 18));
        }
        assert_eq!(
            confirmation_role(&state, owner, id, 17).unwrap(),
            (true, true)
        );
        confirm_for(&state, owner, id, 17).unwrap();
        rx.await.unwrap();
        assert!(confirm_for(&state, owner, id, 17).is_err());
        assert!(confirmation_role(&state, owner, id, 17).is_err());
        assert_eq!(current(&state, owner, 17).unwrap(), id);
        state.0.transfer_view.lock().unwrap().take();
        state.0.registry.close_session(owner, id).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        drop(driver);
    }

    #[tokio::test]
    async fn gate_denies_foreign_old_and_already_confirmed_without_touching_current_binding() {
        let state = WindowSessions::default();
        let owner = state.0.registry.open_window().unwrap();
        let foreign = state.0.registry.open_window().unwrap();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = session::spawn_session(TermSize::new(40, 12).unwrap(), 128, transport);
        let id = state.0.registry.register(owner, handle).unwrap();
        let cfg = AttachmentConfig {
            fragment: 64,
            frames: 1,
            bytes: 128,
            ack_timeout: Duration::from_secs(3),
        };
        let first = state.0.registry.attach(owner, id, cfg).await.unwrap();
        let (tx, _rx) = oneshot::channel();
        *state.0.transfer_view.lock().unwrap() = Some(Current {
            owner,
            id,
            generation: first.generation,
            confirm: Some(tx),
            commit_sequence: None,
            commit_acknowledged: false,
            live: false,
            marker: false,
            data_end: None,
        });
        let old_guard = Guard {
            state: state.clone(),
            owner,
            id,
            generation: first.generation,
        };
        assert_eq!(current(&state, owner, first.generation).unwrap(), id);
        assert!(current(&state, foreign, first.generation).is_err());
        assert!(current(&state, owner, first.generation + 1).is_err());
        let next = state.0.registry.attach(owner, id, cfg).await.unwrap();
        let (tx, _rx) = oneshot::channel();
        *state.0.transfer_view.lock().unwrap() = Some(Current {
            owner,
            id,
            generation: next.generation,
            confirm: Some(tx),
            commit_sequence: None,
            commit_acknowledged: false,
            live: false,
            marker: false,
            data_end: None,
        });
        drop(old_guard);
        state
            .0
            .registry
            .check_binding(owner, id, next.generation)
            .unwrap();
        assert_eq!(current(&state, owner, next.generation).unwrap(), id);
        {
            let mut slot = state.0.transfer_view.lock().unwrap();
            let c = slot.as_mut().unwrap();
            assert!(!c.can_confirm(owner, id, next.generation));
            c.commit_sequence = Some(2);
            assert!(!c.can_confirm(owner, id, next.generation));
            c.commit_acknowledged = true;
            assert!(c.can_confirm(owner, id, next.generation));
            assert!(!c.can_confirm(foreign, id, next.generation));
            assert!(!c.can_confirm(owner, id, next.generation + 1));
        }
        state
            .0
            .transfer_view
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .confirm
            .take();
        assert!(current(&state, owner, next.generation).is_err());
        state
            .0
            .registry
            .check_binding(owner, id, next.generation)
            .unwrap();
        state.0.registry.close_session(owner, id).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), task.join())
            .await
            .unwrap()
            .unwrap();
        drop(driver);
    }
}
