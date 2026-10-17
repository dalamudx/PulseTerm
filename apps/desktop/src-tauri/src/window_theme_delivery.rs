//! Fixed private sample only. Read-only projection; no source/path/session selection or input grant.
use super::*;
use pulse_app::registry::RegisteredThemeSnapshot;
#[cfg(all(test, feature = "settings-probe", unix))]
#[path = "window_theme_delivery_tests.rs"]
mod tests;
#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThemeDelivery {
    pub(super) ticket: String,
    pub(super) generation: String,
    pub(super) paint_json: String,
}
pub(super) struct Challenge {
    pub(super) owner: WindowToken,
    pub(super) id: SessionId,
    pub(super) generation: u64,
    pub(super) page: String,
    pub(super) target_epoch: Option<u64>,
    pub(super) ticket: uuid::Uuid,
    pub(super) snapshot: RegisteredThemeSnapshot,
    pub(super) phase: u8,
    pub(super) done: Option<oneshot::Sender<()>>,
    pub(super) retirement: Option<oneshot::Sender<()>>,
}
pub(super) fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err("theme.delivery.rejected".into())
    }
}
fn authorized<T>(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    page: &str,
    generation: u64,
    action: impl FnOnce(&mut Challenge) -> Result<T>,
) -> Result<T> {
    let active = state.0.active.lock().map_err(error)?;
    let run = active.as_ref().ok_or("session.forbidden")?;
    require(run.nonce == nonce && run.peer_owner == Some(owner) && run.owner != owner)?;
    let id = run.peer_id.ok_or("session.peer_not_registered")?;
    run.peer_pages.with_theme_page(page, || {
        state
            .0
            .registry
            .check_binding(owner, id, generation)
            .map_err(error)?;
        let mut slot = state.0.theme_delivery.lock().map_err(error)?;
        let c = slot.as_mut().ok_or("theme.delivery.disabled")?;
        require(
            c.target_epoch.is_none()
                && c.owner == owner
                && c.id == id
                && c.page == page
                && c.generation == generation,
        )?;
        action(c)
    })
}
fn read(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    page: &str,
    generation: u64,
) -> Result<ThemeDelivery> {
    authorized(state, owner, nonce, page, generation, |c| {
        read_challenge(state, owner, generation, c)
    })
}
pub(super) fn read_challenge(
    state: &WindowSessions,
    owner: WindowToken,
    generation: u64,
    c: &mut Challenge,
) -> Result<ThemeDelivery> {
    require(
        c.phase == 0
            && state
                .0
                .registry
                .view_theme_current(owner, c.id, generation, &c.snapshot)
                .map_err(error)?,
    )?;
    let paint_json = c
        .snapshot
        .theme()
        .document()
        .paint_data()
        .map_err(error)?
        .to_json()
        .map_err(error)?;
    c.phase = 1;
    Ok(ThemeDelivery {
        ticket: c.ticket.to_string(),
        generation: generation.to_string(),
        paint_json,
    })
}
fn advance(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    page: &str,
    generation: u64,
    ticket: &str,
    phase: u8,
) -> Result<()> {
    authorized(state, owner, nonce, page, generation, |c| {
        advance_challenge(state, owner, generation, c, ticket, phase)
    })
}
pub(super) fn advance_challenge(
    state: &WindowSessions,
    owner: WindowToken,
    generation: u64,
    c: &mut Challenge,
    ticket: &str,
    phase: u8,
) -> Result<()> {
    require(
        c.ticket.to_string() == ticket
            && c.phase == phase
            && state
                .0
                .registry
                .view_theme_current(owner, c.id, generation, &c.snapshot)
                .map_err(error)?,
    )?;
    c.phase += 1;
    if phase == 2 {
        c.done
            .take()
            .ok_or("theme.delivery.rejected")?
            .send(())
            .map_err(|_| "theme.delivery.receiver_closed".to_string())?;
    }
    Ok(())
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_peer_theme_read(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
    generation: String,
) -> Result<ThemeDelivery> {
    read(
        &state,
        state.caller(&window)?,
        &nonce,
        &page_nonce,
        parse_u64(&generation)?,
    )
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_peer_theme_confirm(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
    generation: String,
    ticket: String,
) -> Result<()> {
    advance(
        &state,
        state.caller(&window)?,
        &nonce,
        &page_nonce,
        parse_u64(&generation)?,
        &ticket,
        1,
    )
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_peer_theme_drawn(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
    generation: String,
    ticket: String,
) -> Result<()> {
    advance(
        &state,
        state.caller(&window)?,
        &nonce,
        &page_nonce,
        parse_u64(&generation)?,
        &ticket,
        2,
    )
}
#[cfg(feature = "settings-probe")]
pub(super) struct DeliveryGuard {
    pub(super) inner: std::sync::Weak<Inner>,
    pub(super) ticket: uuid::Uuid,
}
#[cfg(feature = "settings-probe")]
impl Drop for DeliveryGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade()
            && let Ok(mut slot) = inner.theme_delivery.lock()
            && slot.as_ref().is_some_and(|c| c.ticket == self.ticket)
        {
            slot.take();
        }
    }
}
/// Trusted fixture schedules exactly one actual-page draw and waits for its independent receipt.
/// Confirmation is instantaneous, not a lease or an atomic file→remote-DOM transaction.
#[cfg(feature = "settings-probe")]
pub(super) async fn deliver(
    state: &WindowSessions,
    window: &WebviewWindow,
    nonce: &str,
    page: &str,
    generation: u64,
) -> Result<()> {
    let (owner, id, pages) = peer_view::owned(state, window, nonce, page)?;
    let (done, receipt) = oneshot::channel();
    let ticket = uuid::Uuid::new_v4();
    pages.with_theme_page(page, || {
        let mut slot = state.0.theme_delivery.lock().map_err(error)?;
        require(slot.is_none())?;
        let snapshot = state
            .0
            .registry
            .read_current_view_theme(owner, id, generation)
            .map_err(error)?
            .ok_or("theme.delivery.stale")?;
        *slot = Some(Challenge {
            owner,
            id,
            generation,
            page: page.into(),
            target_epoch: None,
            ticket,
            snapshot,
            phase: 0,
            done: Some(done),
            retirement: None,
        });
        Ok(())
    })?;
    let _cleanup = DeliveryGuard {
        inner: Arc::downgrade(&state.0),
        ticket,
    };
    // Only backend-generated canonical UUIDs/counter are interpolated, never theme text.
    let result = async {
        window.eval(format!("window.__PULSE_PEER_THEME__({},{},{}).catch(()=>{{console.error('theme.delivery.failed');}})", serde_json::to_string(nonce).map_err(error)?, serde_json::to_string(page).map_err(error)?, serde_json::to_string(&generation.to_string()).map_err(error)?)).map_err(error)?;
        tokio::time::timeout(Duration::from_secs(1), receipt).await.map_err(|_| "theme.delivery.deadline".to_string())?.map_err(error)?;
        Ok(())
    }.await;
    if result.is_ok() {
        eprintln!("session.native_theme.peer_paint_checked");
    }
    result
}
