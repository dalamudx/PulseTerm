//! Fixed migrated main view only; claim is injected by the trusted backend, never a path.
use super::theme_delivery::{Challenge, ThemeDelivery, advance_challenge, read_challenge, require};
use super::*;
use std::sync::atomic::Ordering;
#[cfg(all(test, feature = "settings-probe", unix))]
#[path = "window_theme_target_tests.rs"]
mod tests;

/// Trusted main navigation callback. Revocation only; session and windows stay alive.
pub(crate) fn retire_main_document(state: &WindowSessions) {
    let Ok(mut capture) = state.0.transfer_view.lock() else {
        return;
    };
    if let Some(current) = capture.as_mut() {
        transfer_view::retire_theme_target(current);
    }
    if let Ok(mut slot) = state.0.theme_delivery.lock() {
        let epoch = &state.0.theme_target_epoch;
        let next = epoch.load(Ordering::SeqCst).saturating_add(1);
        epoch.store(next, Ordering::SeqCst);
        if slot.as_ref().is_some_and(|c| c.target_epoch.is_some()) {
            slot.take();
        }
    }
}
fn authorized<T>(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    generation: u64,
    claim: &str,
    action: impl FnOnce(&mut Challenge) -> Result<T>,
) -> Result<T> {
    let active = state.0.active.lock().map_err(error)?;
    let run = active.as_ref().ok_or("session.forbidden")?;
    require(run.nonce == nonce && run.owner == owner && run.peer_owner != Some(owner))?;
    let id = run.peer_id.ok_or("session.peer_not_registered")?;
    transfer_view::with_theme_target(state, owner, id, generation, || {
        let mut slot = state.0.theme_delivery.lock().map_err(error)?;
        let c = slot.as_mut().ok_or("theme.delivery.disabled")?;
        let epoch = state.0.theme_target_epoch.load(Ordering::SeqCst);
        require(
            epoch != u64::MAX
                && c.target_epoch == Some(epoch)
                && c.owner == owner
                && c.id == id
                && c.generation == generation
                && c.page == claim
                && c.ticket.to_string() == claim,
        )?;
        action(c)
    })
}
fn read(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    generation: u64,
    claim: &str,
) -> Result<ThemeDelivery> {
    authorized(state, owner, nonce, generation, claim, |c| {
        read_challenge(state, owner, generation, c)
    })
}
fn advance(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    generation: u64,
    claim: &str,
    phase: u8,
) -> Result<()> {
    authorized(state, owner, nonce, generation, claim, |c| {
        advance_challenge(state, owner, generation, c, claim, phase)
    })
}
pub(super) fn failed(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    generation: u64,
    claim: &str,
    stage: u8,
) -> Result<()> {
    require(stage <= 5)?;
    authorized(state, owner, nonce, generation, claim, |c| {
        require(c.phase <= 3 && c.done.is_some())?;
        c.done.take(); // Fail-only: closes the active wait, never advances draw acceptance.
        eprintln!("session.native_palette.failed stage={stage}");
        Ok(())
    })
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_target_theme_failed(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    claim: String,
    stage: u8,
) -> Result<()> {
    failed(
        &state,
        state.caller(&window)?,
        &nonce,
        parse_u64(&generation)?,
        &claim,
        stage,
    )
}
/// Factory availability only; caller must be the registered local main window.
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_target_theme_mode(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
) -> Result<bool> {
    state.caller(&window)?;
    require(window.label() == "main")?;
    Ok(cfg!(feature = "settings-probe"))
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_target_theme_ready(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
) -> Result<()> {
    let owner = state.caller(&window)?;
    let active = state.0.active.lock().map_err(error)?;
    let run = active
        .as_ref()
        .filter(|r| r.owner == owner && r.nonce == nonce)
        .ok_or("session.forbidden")?;
    let id = run.peer_id.ok_or("session.peer_not_registered")?;
    transfer_view::ready_theme_for(&state, owner, id, parse_u64(&generation)?)
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_target_theme_read(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    claim: String,
) -> Result<ThemeDelivery> {
    read(
        &state,
        state.caller(&window)?,
        &nonce,
        parse_u64(&generation)?,
        &claim,
    )
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_target_theme_confirm(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    claim: String,
) -> Result<()> {
    advance(
        &state,
        state.caller(&window)?,
        &nonce,
        parse_u64(&generation)?,
        &claim,
        1,
    )
}
async fn wait_revoked(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    generation: u64,
    claim: &str,
) -> Result<()> {
    let (retire, retired) = oneshot::channel();
    let notice = authorized(state, owner, nonce, generation, claim, |c| {
        require(c.phase == 2)?;
        let notice = state
            .0
            .registry
            .observe_view_theme_revocation(owner, c.id, generation, &c.snapshot)
            .map_err(error)?;
        c.phase = 3;
        c.retirement = Some(retire);
        c.done
            .take()
            .ok_or("theme.delivery.rejected")?
            .send(())
            .map_err(|_| "theme.delivery.receiver_closed".to_string())?;
        Ok(notice)
    })?;
    tokio::select! {
        biased;
        _ = retired => return Err("theme.delivery.retired".into()),
        _ = notice.wait() => {}
    }
    authorized(state, owner, nonce, generation, claim, |c| {
        require(
            c.phase == 3
                && !state
                    .0
                    .registry
                    .view_theme_current(owner, c.id, generation, &c.snapshot)
                    .map_err(error)?,
        )?;
        c.phase = 4;
        c.retirement.take();
        Ok(())
    })
}
fn cleaned(
    state: &WindowSessions,
    owner: WindowToken,
    nonce: &str,
    generation: u64,
    claim: &str,
) -> Result<()> {
    authorized(state, owner, nonce, generation, claim, |c| {
        require(
            c.phase == 4
                && !state
                    .0
                    .registry
                    .view_theme_current(owner, c.id, generation, &c.snapshot)
                    .map_err(error)?,
        )?;
        c.phase = 5;
        c.done
            .take()
            .ok_or("theme.delivery.rejected")?
            .send(())
            .map_err(|_| "theme.delivery.receiver_closed".to_string())
    })
}
/// Single read-only, content-free observation. It cannot select a source or authorize repaint.
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_target_theme_wait_revoked(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    claim: String,
) -> Result<()> {
    wait_revoked(
        &state,
        state.caller(&window)?,
        &nonce,
        parse_u64(&generation)?,
        &claim,
    )
    .await
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_target_theme_drawn(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    claim: String,
) -> Result<()> {
    cleaned(
        &state,
        state.caller(&window)?,
        &nonce,
        parse_u64(&generation)?,
        &claim,
    )
}
#[cfg(feature = "settings-probe")]
pub(super) async fn deliver(
    state: &WindowSessions,
    window: &WebviewWindow,
    nonce: &str,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
) -> Result<()> {
    require(state.owned(window, nonce)?.0 == owner)?;
    let (done, receipt) = oneshot::channel();
    let ticket = uuid::Uuid::new_v4();
    {
        let active = state.0.active.lock().map_err(error)?;
        let run = active.as_ref().ok_or("session.forbidden")?;
        require(run.owner == owner && run.nonce == nonce && run.peer_id == Some(id))?;
        transfer_view::with_theme_target(state, owner, id, generation, || {
            let mut slot = state.0.theme_delivery.lock().map_err(error)?;
            require(slot.is_none())?;
            let epoch = state.0.theme_target_epoch.load(Ordering::SeqCst);
            require(epoch != u64::MAX)?;
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
                page: ticket.to_string(),
                target_epoch: Some(epoch),
                ticket,
                snapshot,
                phase: 0,
                done: Some(done),
                retirement: None,
            });
            Ok(())
        })?;
    }
    let _cleanup = theme_delivery::DeliveryGuard {
        inner: Arc::downgrade(&state.0),
        ticket,
    };
    // Fixed native-generated identifiers only, never theme contents or user code.
    window.eval(format!("window.__PULSE_TARGET_THEME__({},{},{}).catch(()=>{{console.error('theme.target.failed');}})", serde_json::to_string(nonce).map_err(error)?, serde_json::to_string(&generation.to_string()).map_err(error)?, serde_json::to_string(&ticket.to_string()).map_err(error)?)).map_err(error)?;
    tokio::time::timeout(Duration::from_secs(1), async {
        receipt
            .await
            .map_err(|_| "theme.target.failed".to_string())?;
        let (done, cleanup) = oneshot::channel();
        authorized(state, owner, nonce, generation, &ticket.to_string(), |c| {
            require(c.phase == 3 && c.done.is_none())?;
            c.done = Some(done);
            state
                .0
                .registry
                .invalidate_view_theme(owner, id, generation)
                .map_err(error)
        })?;
        cleanup.await.map_err(error)
    })
    .await
    .map_err(|_| "theme.target.deadline".to_string())??;
    eprintln!("session.native_theme.target_paint_checked");
    Ok(())
}
