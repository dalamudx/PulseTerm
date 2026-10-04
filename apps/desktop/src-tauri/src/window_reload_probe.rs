//! Real fixed peer WebView reload with Rust-only recapture. No production navigation hook.
use super::*;
use tauri::webview::PageLoadEvent;
use tokio::sync::watch;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Pages {
    started: u8,
    finished: u8,
    invalid: bool,
    authenticated: u8,
    old_nonce_rejected: bool,
    rendered: bool,
    captured: bool,
}
#[derive(Clone, Default)]
struct Nonces {
    current: Option<uuid::Uuid>,
    previous: Option<uuid::Uuid>,
}
impl Pages {
    fn ready(self, count: u8) -> bool {
        !self.invalid
            && self.started == count
            && self.finished == count
            && self.authenticated == count
            && (count == 1 || self.old_nonce_rejected)
    }
}
#[derive(Clone)]
pub(super) struct PageTracker {
    events: watch::Sender<Pages>,
    nonce: Arc<Mutex<Nonces>>,
}
impl PageTracker {
    pub(super) fn new() -> Self {
        let (tx, _) = watch::channel(Pages::default());
        Self {
            events: tx,
            nonce: Arc::new(Mutex::new(Nonces::default())),
        }
    }
    pub(super) fn record(&self, event: PageLoadEvent) {
        let Ok(mut nonces) = self.nonce.lock() else {
            self.events.send_modify(|p| p.invalid = true);
            return;
        };
        self.events.send_modify(|p| {
            if p.invalid {
                return;
            }
            match event {
                PageLoadEvent::Started => {
                    if p.started != p.finished || p.started >= 2 {
                        p.invalid = true;
                    } else {
                        if let Some(old) = nonces.current.take() {
                            nonces.previous = Some(old);
                        }
                        p.started += 1;
                    }
                }
                PageLoadEvent::Finished => {
                    if p.started != p.finished + 1 {
                        p.invalid = true;
                    } else {
                        nonces.current = Some(uuid::Uuid::new_v4());
                        p.finished += 1;
                    }
                }
            }
            if p.invalid {
                nonces.current = None;
            }
        });
    }
    // Backend-only material for current-document eval. Never expose a nonce getter RPC.
    pub(super) fn injection(&self) -> Result<(String, Option<String>)> {
        let n = self.nonce.lock().map_err(error)?;
        let p = self.snapshot();
        require(!p.invalid && p.started == p.finished)?;
        Ok((
            n.current.ok_or("session.peer_page_not_ready")?.to_string(),
            n.previous.map(|v| v.to_string()),
        ))
    }
    fn revoke_nonce(&self) -> Result<()> {
        let mut n = self.nonce.lock().map_err(error)?;
        n.previous = n.current.take();
        Ok(())
    }
    pub(super) fn confirm_nonce(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let n = self.nonce.lock().map_err(error)?;
        if n.current != Some(parsed) {
            if n.previous == Some(parsed) {
                self.events.send_modify(|p| p.old_nonce_rejected = true);
            }
            return Err("session.peer_page_nonce_rejected".into());
        }
        let mut accepted = false;
        self.events.send_modify(|p| {
            if !p.invalid
                && p.started == p.finished
                && p.authenticated + 1 == p.finished
                && (p.finished == 1 || p.old_nonce_rejected)
            {
                p.authenticated = p.finished;
                accepted = true;
            }
        });
        require(accepted)
    }

    pub(super) fn authorize_view(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let n = self.nonce.lock().map_err(error)?;
        require(n.current == Some(parsed) && self.snapshot().ready(2))
    }
    pub(super) fn view_finished(&self) -> bool {
        self.snapshot().captured
    }
    pub(super) fn mark_captured(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let nonce = self.nonce.lock().map_err(error)?;
        require(nonce.current == Some(parsed))?;
        let mut accepted = false;
        self.events.send_modify(|p| {
            if p.ready(2) && !p.captured && !p.rendered {
                p.captured = true;
                accepted = true;
            }
        });
        require(accepted)
    }
    pub(super) fn mark_rendered(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let nonce = self.nonce.lock().map_err(error)?;
        require(nonce.current == Some(parsed))?;
        let mut accepted = false;
        self.events.send_modify(|p| {
            if p.ready(2) && p.captured && !p.rendered {
                p.rendered = true;
                accepted = true;
            }
        });
        require(accepted)
    }

    fn snapshot(&self) -> Pages {
        *self.events.borrow()
    }
}
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err("session.peer_reload_invalid".into())
    }
}
pub(super) async fn verify(
    state: &WindowSessions,
    window: &WebviewWindow,
    owner: WindowToken,
    id: SessionId,
    handle: &SessionHandle,
    pages: &PageTracker,
) -> Result<()> {
    require(pages.snapshot().ready(1) && state.caller(window)? == owner)?;
    let old = transfer::capture(state, owner, id, handle).await?;
    let before = state.0.registry.state(owner, id).map_err(error)?;
    // Peer JS has never been granted access to this session: its only script probes
    // forbidden main-session controls. Revoke its page nonce before retiring views;
    // new nonce is injected only to the new local document after Finished.
    let old_nonce = pages.injection()?.0;
    pages.revoke_nonce()?;
    require(state.0.registry.retire_window_views(owner).map_err(error)? == 1)?;
    require(
        state
            .0
            .registry
            .acknowledge(owner, id, old.generation, 0)
            .is_err()
            && state
                .0
                .registry
                .input_bound(owner, id, old.generation, b"denied\n".to_vec())
                .await
                .is_err(),
    )?;
    let mut changed = pages.events.subscribe();
    eprintln!("session.peer_reload.requested");
    window.reload().map_err(error)?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let p = *changed.borrow();
            require(!p.invalid)?;
            if p.ready(2) && p.rendered {
                return Ok::<(), String>(());
            }
            changed.changed().await.map_err(error)?;
        }
    })
    .await
    .map_err(|_| "session.peer_reload_page_deadline".to_string())??;
    require(
        state.caller(window)? == owner
            && state.0.registry.state(owner, id).map_err(error)? == before
            && handle.state.borrow().lifecycle == Lifecycle::Running
            && window
                .app_handle()
                .get_webview_window(window.label())
                .is_some(),
    )?;
    require(pages.confirm_nonce(&old_nonce).is_err() && pages.injection()?.0 != old_nonce)?;
    let fresh = transfer::capture(state, owner, id, handle).await?;
    require(fresh.generation > old.generation)?;
    let old_result = tokio::time::timeout(Duration::from_secs(3), old.completion)
        .await
        .map_err(error)?
        .map_err(error)?;
    require(matches!(
        old_result,
        Err(pulse_app::attachment::AttachmentError::Detached)
    ))?;
    state
        .0
        .registry
        .check_binding(owner, id, fresh.generation)
        .map_err(error)?;
    state
        .0
        .registry
        .detach(owner, id, fresh.generation)
        .map_err(error)?;
    let result = tokio::time::timeout(Duration::from_secs(3), fresh.completion)
        .await
        .map_err(error)?
        .map_err(error)?;
    require(matches!(
        result,
        Err(pulse_app::attachment::AttachmentError::Detached)
    ))?;
    eprintln!("session.peer_reload.recaptured");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readonly_view_requires_authenticated_fresh_page_and_rendered_receipt_is_single_use() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let old = p.injection().unwrap_or_default().0;
        assert!(p.authorize_view(&old).is_err());
        assert!(p.confirm_nonce(&old).is_ok());
        assert!(p.authorize_view(&old).is_err());
        assert!(p.mark_rendered(&old).is_err());
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let current = p.injection().unwrap_or_default().0;
        assert!(p.authorize_view(&current).is_err());
        assert!(p.confirm_nonce(&old).is_err());
        assert!(p.confirm_nonce(&current).is_ok());
        assert!(p.authorize_view(&current).is_ok());
        assert!(!p.view_finished());
        assert!(p.mark_rendered(&old).is_err());
        assert!(p.mark_rendered(&current).is_err());
        assert!(p.mark_captured(&old).is_err());
        assert!(p.mark_captured(&current).is_ok());
        assert!(p.mark_captured(&current).is_err());
        assert!(p.mark_rendered(&current).is_ok());
        assert!(p.view_finished());
        assert!(p.mark_rendered(&current).is_err());
        assert!(p.revoke_nonce().is_ok());
        assert!(p.authorize_view(&current).is_err());
        assert!(p.mark_rendered(&current).is_err());
    }

    #[test]
    fn navigation_nonce_is_revoked_and_duplicate_or_old_page_confirmation_cannot_reauthorize() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        assert!(p.injection().is_err());
        p.record(PageLoadEvent::Finished);
        let old = p.injection().ok().map(|n| n.0).unwrap_or_default();
        assert!(!old.is_empty());
        assert!(p.confirm_nonce(&old).is_ok());
        assert!(p.snapshot().ready(1));
        assert!(p.confirm_nonce(&old).is_err());
        assert!(p.revoke_nonce().is_ok());
        assert!(p.injection().is_err());
        p.record(PageLoadEvent::Started);
        assert!(p.confirm_nonce(&old).is_err());
        p.record(PageLoadEvent::Finished);
        let (current, previous) = p.injection().unwrap_or_default();
        assert_ne!(current, old);
        assert_eq!(previous.as_deref(), Some(old.as_str()));
        assert!(p.confirm_nonce(&current).is_ok());
        assert!(p.snapshot().ready(2));
        assert!(p.confirm_nonce(&old).is_err());
        assert!(p.confirm_nonce(&current).is_err());
    }
    #[test]
    fn loaded_page_is_not_ready_without_actual_old_nonce_rejection_and_current_confirmation() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let old = p.injection().unwrap_or_default().0;
        assert!(p.confirm_nonce(&old).is_ok());
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let fresh = p.injection().unwrap_or_default().0;
        assert!(!p.snapshot().ready(2));
        assert!(p.confirm_nonce(&fresh).is_err());
        assert!(p.confirm_nonce(&uuid::Uuid::new_v4().to_string()).is_err());
        assert!(!p.snapshot().old_nonce_rejected);
        assert!(p.confirm_nonce(&old).is_err());
        assert!(p.confirm_nonce(&fresh).is_ok());
        assert!(p.snapshot().ready(2));
        p.record(PageLoadEvent::Started);
        assert!(p.confirm_nonce(&fresh).is_err());
        assert!(p.injection().is_err());
    }

    #[test]
    fn two_real_page_cycles_are_required_and_early_finish_is_not_ready() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        assert_eq!(p.snapshot().finished, 0);
        p.record(PageLoadEvent::Finished);
        assert_eq!(
            p.snapshot(),
            Pages {
                started: 1,
                finished: 1,
                invalid: false,
                authenticated: 0,
                old_nonce_rejected: false,
                rendered: false,
                captured: false,
            }
        );
        p.record(PageLoadEvent::Started);
        assert_eq!(p.snapshot().finished, 1);
        p.record(PageLoadEvent::Finished);
        assert_eq!(
            p.snapshot(),
            Pages {
                started: 2,
                finished: 2,
                invalid: false,
                authenticated: 0,
                old_nonce_rejected: false,
                rendered: false,
                captured: false,
            }
        );
        p.record(PageLoadEvent::Started);
        assert!(p.snapshot().invalid);
    }
    #[test]
    fn duplicated_or_out_of_order_callbacks_never_fabricate_fresh_page() {
        for bad in [
            vec![PageLoadEvent::Finished],
            vec![PageLoadEvent::Started, PageLoadEvent::Started],
            vec![
                PageLoadEvent::Started,
                PageLoadEvent::Finished,
                PageLoadEvent::Finished,
            ],
        ] {
            let p = PageTracker::new();
            for e in bad {
                p.record(e);
            }
            assert!(p.snapshot().invalid);
            p.record(PageLoadEvent::Started);
            p.record(PageLoadEvent::Finished);
            assert!(p.snapshot().invalid);
            assert!(p.snapshot().finished < 2);
        }
    }
}
