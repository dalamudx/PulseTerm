//! Fixed refreshed peer page reads its own actor through a nonce-scoped binary Channel.
use super::*;
fn owned(
    state: &WindowSessions,
    window: &WebviewWindow,
    nonce: &str,
    page_nonce: &str,
) -> Result<(WindowToken, SessionId, reload::PageTracker)> {
    let owner = state.caller(window)?;
    let active = state.0.active.lock().map_err(error)?;
    let run = active.as_ref().ok_or("session.forbidden")?;
    let (id, pages) = run.peer_view(owner, nonce, page_nonce)?;
    state.0.registry.state(owner, id).map_err(error)?;
    Ok((owner, id, pages))
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_peer_ack(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
    generation: String,
    sequence: u32,
) -> Result<()> {
    let (owner, id, _) = owned(&state, &window, &nonce, &page_nonce)?;
    transfer_view::acknowledge_for(&state, owner, id, parse_u64(&generation)?, sequence)
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_peer_confirm(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
    generation: String,
    rendered: RenderedProjection,
) -> Result<()> {
    let (owner, id, pages) = owned(&state, &window, &nonce, &page_nonce)?;
    let generation = parse_u64(&generation)?;
    let (live, marker) = transfer_view::confirmation_role(&state, owner, id, generation)?;
    transfer_view::compare_for(&state, owner, id, generation, rendered).await?;
    owned(&state, &window, &nonce, &page_nonce)?;
    transfer_view::confirm_for(&state, owner, id, generation)?;
    if live {
        if marker {
            pages.mark_live_display(&page_nonce)?;
        } else {
            pages.mark_captured(&page_nonce)?;
        }
    }
    Ok(())
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_peer_view(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
    channel: Channel<RawFrame>,
) -> Result<()> {
    let state = state.inner().clone();
    let (owner, id, pages) = owned(&state, &window, &nonce, &page_nonce)?;
    let initial = pages.is_initial_document();
    if !initial {
        pages.wait_navigation_joined().await?;
        pages.wait_refresh_capture().await?;
        owned(&state, &window, &nonce, &page_nonce)?;
    }
    {
        let (initial_claim, _slot, lane) = admit_view(&state, &pages, &page_nonce)?;
        if initial_claim != initial {
            return Err("session.peer_page_changed".into());
        }
        let result =
            transfer_view::render_live(&state, owner, id, &channel, &pages, &page_nonce, lane)
                .await;
        eprintln!(
            "session.peer_live.supervisor_returned initial={initial} ok={}",
            result.is_ok()
        );
        result?;
    }
    if initial {
        eprintln!("session.peer_live.peer_permit_released");
        pages.mark_navigation_joined()?;
        eprintln!("session.peer_live.navigation_joined");
    } else {
        owned(&state, &window, &nonce, &page_nonce)?;
        pages.mark_refreshed_joined(&page_nonce)?;
        eprintln!("session.peer_fresh_live.joined");
    }
    Ok(())
}
fn admit_view(
    state: &WindowSessions,
    pages: &reload::PageTracker,
    page_nonce: &str,
) -> Result<(
    bool,
    tokio::sync::OwnedSemaphorePermit,
    tokio::sync::OwnedSemaphorePermit,
)> {
    pages.authorize_view(page_nonce)?;
    let peer = state
        .0
        .peer_view_slot
        .clone()
        .try_acquire_owned()
        .map_err(|_| "session.peer_view_busy".to_string())?;
    let lane = transfer_view::reserve_lane(state)?;
    if pages.view_finished() {
        return Err("session.peer_view_already_finished".into());
    }
    let initial = pages.claim_view(page_nonce)?;
    Ok((initial, peer, lane))
}

#[tauri::command]
#[specta::specta]
pub(crate) fn probe_peer_done(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
) -> Result<()> {
    let (_, _, pages) = owned(&state, &window, &nonce, &page_nonce)?;
    pages.mark_rendered(&page_nonce)
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_peer_hold_ready(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
    generation: String,
) -> Result<()> {
    let (owner, id, pages) = owned(&state, &window, &nonce, &page_nonce)?;
    let generation = parse_u64(&generation)?;
    if transfer_view::live_generation(&state, owner, id)? != generation {
        return Err("session.peer_wrong_hold_generation".into());
    }
    state
        .0
        .registry
        .check_binding(owner, id, generation)
        .map_err(error)?;
    pages.mark_hold_ready(&page_nonce)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pulse_term::session::{ChannelTransport, spawn_session};
    use tauri::webview::PageLoadEvent;
    #[test]
    fn rejected_lane_or_nonce_does_not_spend_initial_or_refreshed_capture_claim() {
        for refreshed in [false, true] {
            let state = WindowSessions::default();
            let pages = reload::PageTracker::new();
            pages.record(PageLoadEvent::Started);
            pages.record(PageLoadEvent::Finished);
            let first = pages.injection().unwrap_or_default().0;
            assert!(pages.confirm_nonce(&first).is_ok());
            let nonce = if refreshed {
                assert!(pages.claim_view(&first).is_ok());
                assert!(pages.mark_captured(&first).is_ok());
                assert!(pages.mark_rendered(&first).is_ok());
                pages.record(PageLoadEvent::Started);
                pages.navigation_retired(Ok(1));
                assert!(pages.mark_navigation_joined().is_ok());
                pages.record(PageLoadEvent::Finished);
                let fresh = pages.injection().unwrap_or_default().0;
                assert!(pages.confirm_nonce(&first).is_err());
                assert!(pages.confirm_nonce(&fresh).is_ok());
                fresh
            } else {
                first
            };
            let wrong = uuid::Uuid::nil().to_string();
            assert!(admit_view(&state, &pages, &wrong).is_err());
            assert_eq!(state.0.peer_view_slot.available_permits(), 1);
            assert_eq!(state.0.transfer_view_slot.available_permits(), 1);
            let busy_peer = state
                .0
                .peer_view_slot
                .clone()
                .try_acquire_owned()
                .unwrap_or_else(|_| panic!("peer"));
            assert!(admit_view(&state, &pages, &nonce).is_err());
            drop(busy_peer);
            let busy_lane = transfer_view::reserve_lane(&state).unwrap_or_else(|_| panic!("lane"));
            assert!(admit_view(&state, &pages, &nonce).is_err());
            assert_eq!(state.0.peer_view_slot.available_permits(), 1);
            drop(busy_lane);
            let (initial, peer, lane) =
                admit_view(&state, &pages, &nonce).unwrap_or_else(|_| panic!("admission"));
            assert_eq!(initial, !refreshed);
            assert!(admit_view(&state, &pages, &nonce).is_err());
            drop((peer, lane));
            assert!(admit_view(&state, &pages, &nonce).is_err());
            assert_eq!(state.0.peer_view_slot.available_permits(), 1);
            assert_eq!(state.0.transfer_view_slot.available_permits(), 1);
        }
    }

    #[tokio::test]
    async fn trusted_started_retirement_ignores_old_run_and_recreated_window_but_keeps_peer_actor_alive()
     {
        let state = WindowSessions::default();
        let main = state.0.registry.open_window().unwrap();
        let peer = state.0.registry.open_window().unwrap();
        let foreign = state.0.registry.open_window().unwrap();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = spawn_session(TermSize::new(40, 12).unwrap(), 128, transport);
        let id = state.0.registry.register(peer, handle).unwrap();
        let view = state
            .0
            .registry
            .attach(
                peer,
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
        let mut run = Active::new(
            main,
            "current".into(),
            id,
            Arc::new(crate::viewer_visibility::VisibilityGate::default()),
        );
        run.bind_peer(peer).unwrap();
        run.peer_id = Some(id);
        *state.0.active.lock().unwrap() = Some(run);
        state
            .0
            .windows
            .lock()
            .unwrap()
            .insert("session-peer".into(), peer);
        assert!(state.retire_peer_page("session-peer", "old-run").is_err());
        state
            .0
            .registry
            .check_binding(peer, id, view.generation)
            .unwrap();
        state
            .0
            .windows
            .lock()
            .unwrap()
            .insert("session-peer".into(), foreign);
        assert!(state.retire_peer_page("session-peer", "current").is_err());
        state
            .0
            .registry
            .check_binding(peer, id, view.generation)
            .unwrap();
        state
            .0
            .windows
            .lock()
            .unwrap()
            .insert("session-peer".into(), peer);
        assert_eq!(
            state.retire_peer_page("session-peer", "current").unwrap(),
            1
        );
        assert!(
            state
                .0
                .registry
                .check_binding(peer, id, view.generation)
                .is_err()
        );
        assert_eq!(
            state.retire_peer_page("session-peer", "current").unwrap(),
            0
        );
        assert_eq!(
            state.0.registry.state(peer, id).unwrap().lifecycle,
            Lifecycle::Running
        );
        state.0.registry.close_session(peer, id).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        drop((view, driver));
    }

    #[tokio::test]
    async fn pending_readonly_authorization_cannot_complete_after_navigation_revokes_its_epoch() {
        let registry = SessionRegistry::new();
        let main = registry.open_window().unwrap();
        let peer = registry.open_window().unwrap();
        let foreign = registry.open_window().unwrap();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = spawn_session(TermSize::new(40, 12).unwrap(), 128, transport);
        let id = registry.register(peer, handle).unwrap();
        let mut run = Active::new(
            main,
            "run".into(),
            id,
            Arc::new(crate::viewer_visibility::VisibilityGate::default()),
        );
        run.bind_peer(peer).unwrap();
        run.peer_id = Some(id);
        let pages = run.peer_pages.clone();
        pages.record(tauri::webview::PageLoadEvent::Started);
        pages.record(tauri::webview::PageLoadEvent::Finished);
        let old = pages.injection().unwrap().0;
        pages.confirm_nonce(&old).unwrap();
        assert!(run.peer_view(peer, "run", &old).is_ok());
        pages.record(tauri::webview::PageLoadEvent::Started);
        pages.record(tauri::webview::PageLoadEvent::Finished);
        let current = pages.injection().unwrap().0;
        assert!(pages.confirm_nonce(&old).is_err());
        pages.confirm_nonce(&current).unwrap();
        assert!(run.peer_view(foreign, "run", &current).is_err());
        assert!(run.peer_view(main, "run", &current).is_err());
        assert!(run.peer_view(peer, "wrong-run", &current).is_err());
        assert!(run.peer_view(peer, "run", &old).is_err());
        let (admitted, pending) = run.peer_view(peer, "run", &current).unwrap();
        assert_eq!(admitted, id);
        // Represents compare/capture yielding after initial permission admission.
        pages.record(tauri::webview::PageLoadEvent::Started);
        registry.retire_window_views(peer).unwrap();
        tokio::task::yield_now().await;
        assert!(run.peer_view(peer, "run", &current).is_err());
        assert!(pending.mark_captured(&current).is_err());
        assert!(pending.mark_rendered(&current).is_err());
        assert!(!pending.view_finished());
        assert_eq!(
            registry.state(peer, id).unwrap().lifecycle,
            Lifecycle::Running
        );
        registry.close_session(peer, id).unwrap();
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
    async fn no_peer_run_default_never_inherits_a_page_capability() {
        let registry = SessionRegistry::new();
        let owner = registry.open_window().unwrap();
        let foreign = registry.open_window().unwrap();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = spawn_session(TermSize::new(40, 12).unwrap(), 128, transport);
        let id = registry.register(owner, handle).unwrap();
        let run = Active::new(
            owner,
            "run".into(),
            id,
            Arc::new(crate::viewer_visibility::VisibilityGate::default()),
        );
        assert!(run.peer.is_none());
        assert!(run.peer_id.is_none());
        assert!(run.peer_owner.is_none());
        assert!(
            run.peer_view(foreign, "run", &uuid::Uuid::new_v4().to_string())
                .is_err()
        );
        assert!(run.peer_pages.injection().is_err());
        registry.close_session(owner, id).unwrap();
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
}
