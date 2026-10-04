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
    let (owner, id, _) = owned(&state, &window, &nonce, &page_nonce)?;
    let generation = parse_u64(&generation)?;
    transfer_view::compare_for(&state, owner, id, generation, rendered).await?;
    owned(&state, &window, &nonce, &page_nonce)?;
    transfer_view::confirm_for(&state, owner, id, generation)
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
    let _slot = state
        .0
        .peer_view_slot
        .clone()
        .try_acquire_owned()
        .map_err(error)?;
    let (owner, id, pages) = owned(&state, &window, &nonce, &page_nonce)?;
    if pages.view_finished() {
        return Err("session.peer_view_already_finished".into());
    }
    transfer_view::render(&state, owner, id, 0, &channel).await?;
    owned(&state, &window, &nonce, &page_nonce)?;
    pages.mark_captured(&page_nonce)
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
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pulse_term::session::{ChannelTransport, spawn_session};
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
        assert!(run.peer_view(peer, "run", &old).is_err());
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
