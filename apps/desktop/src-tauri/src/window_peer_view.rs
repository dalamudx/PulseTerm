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
    let run = active
        .as_ref()
        .filter(|r| r.nonce == nonce && r.peer_owner == Some(owner) && r.owner != owner)
        .ok_or("session.forbidden")?;
    run.peer_pages.authorize_view(page_nonce)?;
    let id = run.peer_id.ok_or("session.peer_not_registered")?;
    state.0.registry.state(owner, id).map_err(error)?;
    Ok((owner, id, run.peer_pages.clone()))
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
