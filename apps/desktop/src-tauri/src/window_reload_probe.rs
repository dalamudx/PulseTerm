//! Real fixed peer WebView reload with Rust-only recapture. No production navigation hook.
use super::*;
use tauri::webview::PageLoadEvent;
use tokio::sync::watch;
// Native Finished can precede the React effect installing the fixed peer hook.
// No timers/retries/nonce getter: either already installed or one document-local resolver.
pub(super) const WAIT_BOOTSTRAP: &str = "await new Promise(resolve=>{if(typeof window.__PULSE_PEER_VIEW__==='function'){resolve();}else{window.__PULSE_PEER_VIEW_READY__=resolve;}});";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Pages {
    started: u8,
    finished: u8,
    invalid: bool,
    authenticated: u8,
    old_nonce_rejected: bool,
    rendered: bool,
    captured: bool,
    navigation_retired: bool,
    initial_display: bool,
    bootstrap_advanced: bool,
    claimed: bool,
    live_display: bool,
    navigation_joined: bool,
    hold_ready: bool,
    pending_ack: bool,
    refresh_allowed: bool,
    refreshed_live_display: bool,
    refreshed_bootstrap_advanced: bool,
    refreshed_joined: bool,
    shell_hints_checked: bool,
    initial_shell_hints_checked: bool,
}
#[derive(Clone, Default)]
struct Nonces {
    current: Option<uuid::Uuid>,
    previous: Option<uuid::Uuid>,
}
impl Pages {
    fn readonly_ready(self) -> bool {
        self.ready(1) || self.ready(2)
    }
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
                        p.captured = false;
                        p.rendered = false;
                        p.claimed = false;
                        p.shell_hints_checked = false;
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
    #[cfg(test)]
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

    // Only the fixed login-validation adapter uses this: keep navigation blocked while
    // a current, already authenticated page generation is activated in the registry.
    // The caller must still verify native WindowToken and local origin before entering.
    #[cfg(feature = "login-probe")]
    pub(super) fn with_ready_page<T>(
        &self,
        value: &str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let n = self.nonce.lock().map_err(error)?;
        require(n.current == Some(parsed) && self.snapshot().readonly_ready())?;
        action()
    }

    pub(super) fn authorize_view(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let n = self.nonce.lock().map_err(error)?;
        require(n.current == Some(parsed) && self.snapshot().readonly_ready())
    }
    /// Short synchronous metadata callback; navigation cannot change the nonce mid-check.
    /// No IO/await or callback into this tracker. Lock order: active → nonce → delivery → registry.
    pub(super) fn with_theme_page<T>(
        &self,
        value: &str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        let nonce = self.nonce.lock().map_err(error)?;
        let p = self.snapshot();
        require(
            parsed.to_string() == value
                && nonce.current == Some(parsed)
                && p.readonly_ready()
                && p.captured
                && p.rendered,
        )?;
        action()
    }
    pub(super) fn claim_view(&self, value: &str) -> Result<bool> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let nonce = self.nonce.lock().map_err(error)?;
        require(nonce.current == Some(parsed))?;
        let mut initial = None;
        self.events.send_modify(|p| {
            if p.readonly_ready() && !p.claimed {
                p.claimed = true;
                initial = Some(p.finished == 1);
            }
        });
        initial.ok_or("session.peer_duplicate_capture".into())
    }
    pub(super) fn mark_live_display(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let nonce = self.nonce.lock().map_err(error)?;
        require(nonce.current == Some(parsed))?;
        let mut accepted = false;
        self.events.send_modify(|p| {
            if p.rendered && p.captured {
                if p.ready(1) && !p.live_display {
                    p.live_display = true;
                    accepted = true;
                } else if p.ready(2) && p.refreshed_bootstrap_advanced && !p.refreshed_live_display
                {
                    p.refreshed_live_display = true;
                    accepted = true;
                }
            }
        });
        require(accepted)
    }
    pub(super) fn mark_hold_ready(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        require(parsed.to_string() == value)?;
        let n = self.nonce.lock().map_err(error)?;
        require(n.current == Some(parsed))?;
        let mut accepted = false;
        self.events.send_modify(|p| {
            if p.ready(1) && p.live_display && !p.hold_ready {
                p.hold_ready = true;
                accepted = true;
            }
        });
        require(accepted)
    }
    fn pending_ack_observed(&self) -> Result<()> {
        let mut accepted = false;
        self.events.send_modify(|p| {
            if p.ready(1) && p.hold_ready && p.live_display && !p.pending_ack {
                p.pending_ack = true;
                accepted = true;
            }
        });
        require(accepted)
    }
    fn allow_refresh_capture(&self) -> Result<()> {
        let mut accepted = false;
        self.events.send_modify(|p| {
            if !p.invalid
                && p.started == 2
                && p.navigation_joined
                && p.pending_ack
                && !p.refresh_allowed
            {
                p.refresh_allowed = true;
                accepted = true;
            }
        });
        require(accepted)
    }
    pub(super) fn mark_bootstrap_advanced(&self, value: &str) -> Result<()> {
        self.authorize_view(value)?;
        let mut accepted = false;
        self.events.send_modify(|p| {
            if p.ready(1) && p.claimed && !p.captured && !p.rendered && !p.bootstrap_advanced {
                p.bootstrap_advanced = true;
                accepted = true;
            } else if p.ready(2)
                && p.claimed
                && !p.captured
                && !p.rendered
                && !p.refreshed_bootstrap_advanced
            {
                p.refreshed_bootstrap_advanced = true;
                accepted = true;
            }
        });
        require(accepted)
    }
    pub(super) fn is_initial_document(&self) -> bool {
        self.snapshot().ready(1)
    }
    pub(super) fn mark_refreshed_joined(&self, value: &str) -> Result<()> {
        self.authorize_view(value)?;
        let mut accepted = false;
        self.events.send_modify(|p| {
            if p.ready(2)
                && p.captured
                && p.rendered
                && p.refreshed_live_display
                && !p.refreshed_joined
            {
                p.refreshed_joined = true;
                accepted = true;
            }
        });
        require(accepted)
    }
    pub(super) async fn wait_refreshed_joined(&self) -> Result<()> {
        let mut events = self.events.subscribe();
        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let p = *events.borrow();
                require(p.ready(2))?;
                if p.refreshed_joined {
                    return Ok::<(), String>(());
                }
                events.changed().await.map_err(error)?;
            }
        })
        .await
        .map_err(|_| "session.peer_fresh_view_join_deadline".to_string())?
    }

    pub(super) async fn wait_initial_display(&self, value: &str) -> Result<()> {
        let mut events = self.events.subscribe();
        loop {
            self.authorize_view(value)?;
            let p = *events.borrow();
            require(p.readonly_ready())?;
            if p.captured && p.rendered {
                return Ok(());
            }
            events.changed().await.map_err(error)?;
        }
    }
    pub(super) async fn wait_refresh_capture(&self) -> Result<()> {
        let mut events = self.events.subscribe();
        tokio::time::timeout(Duration::from_millis(1500), async {
            loop {
                let p = *events.borrow();
                require(!p.invalid)?;
                if p.refresh_allowed {
                    return Ok::<(), String>(());
                }
                events.changed().await.map_err(error)?;
            }
        })
        .await
        .map_err(|_| "session.peer_refresh_drain_deadline".to_string())?
    }

    pub(super) fn has_navigation_started(&self) -> bool {
        let p = self.snapshot();
        !p.invalid && p.started == 2
    }
    pub(super) async fn navigation_started(&self) -> Result<()> {
        let mut events = self.events.subscribe();
        loop {
            let p = *events.borrow();
            require(!p.invalid)?;
            if p.started == 2 && p.navigation_retired {
                return Ok(());
            }
            events.changed().await.map_err(error)?;
        }
    }
    pub(super) fn mark_navigation_joined(&self) -> Result<()> {
        let mut accepted = false;
        self.events.send_modify(|p| {
            if !p.invalid && p.started == 2 && p.navigation_retired && !p.navigation_joined {
                p.navigation_joined = true;
                accepted = true;
            }
        });
        require(accepted)
    }
    pub(super) async fn wait_navigation_joined(&self) -> Result<()> {
        let mut events = self.events.subscribe();
        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let p = *events.borrow();
                require(!p.invalid)?;
                if p.navigation_joined {
                    return Ok::<(), String>(());
                }
                events.changed().await.map_err(error)?;
            }
        })
        .await
        .map_err(|_| "session.peer_old_view_join_deadline".to_string())?
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
            if p.readonly_ready() && !p.captured && !p.rendered {
                p.captured = true;
                accepted = true;
            }
        });
        require(accepted)
    }
    pub(super) fn initial_page(&self, value: &str) -> Result<bool> {
        self.authorize_view(value)?;
        Ok(self.snapshot().finished == 1)
    }
    pub(super) fn shell_hints_checked(&self) -> bool {
        let p = self.snapshot();
        !p.invalid && p.initial_shell_hints_checked && p.shell_hints_checked && p.finished == 2
    }
    pub(super) fn require_shell_hints(&self, value: &str) -> Result<()> {
        self.authorize_view(value)?;
        require(self.snapshot().shell_hints_checked)
    }
    pub(super) fn authorize_shell_hints(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        let nonce = self.nonce.lock().map_err(error)?;
        let p = self.snapshot();
        require(
            nonce.current == Some(parsed)
                && parsed.to_string() == value
                && p.readonly_ready()
                && p.captured
                && !p.rendered
                && !p.shell_hints_checked,
        )
    }
    pub(super) fn mark_shell_hints(&self, value: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(value).map_err(error)?;
        let nonce = self.nonce.lock().map_err(error)?;
        require(nonce.current == Some(parsed) && parsed.to_string() == value)?;
        let mut accepted = false;
        self.events.send_modify(|p| {
            if p.readonly_ready() && p.captured && !p.shell_hints_checked && !p.rendered {
                p.shell_hints_checked = true;
                if p.finished == 1 {
                    p.initial_shell_hints_checked = true;
                }
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
            if p.readonly_ready() && p.captured && !p.rendered {
                p.rendered = true;
                if p.finished == 1 {
                    p.initial_display = true;
                }
                accepted = true;
            }
        });
        require(accepted)
    }

    pub(super) fn navigation_retired(&self, result: Result<usize>) {
        self.events.send_modify(|p| {
            if p.started == 1 && p.finished == 0 && !p.invalid {
                if result != Ok(0) {
                    p.invalid = true;
                }
            } else if p.started == 2
                && p.finished == 1
                && !p.invalid
                && !p.navigation_retired
                && result == Ok(1)
            {
                p.navigation_retired = true;
            } else {
                p.invalid = true;
            }
        });
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
#[cfg(feature = "settings-probe")]
pub(super) type SettingsReceipt = settings_probe::Fixture;
#[cfg(not(feature = "settings-probe"))]
pub(super) struct SettingsReceipt;
pub(super) async fn verify(
    state: &WindowSessions,
    window: &WebviewWindow,
    owner: WindowToken,
    id: SessionId,
    handle: &SessionHandle,
    pages: &PageTracker,
) -> Result<SettingsReceipt> {
    require(pages.snapshot().ready(1) && state.caller(window)? == owner)?;
    let run_nonce = {
        let active = state.0.active.lock().map_err(error)?;
        active
            .as_ref()
            .filter(|r| r.peer_owner == Some(owner) && r.peer_id == Some(id))
            .ok_or("session.forbidden")?
            .nonce
            .clone()
    };
    let page_nonce = pages.injection()?.0;
    let mut first_page = pages.events.subscribe();
    window
        .eval(format!(
            "void window.__PULSE_PEER_VIEW__('{run_nonce}','{page_nonce}',undefined,true).catch(console.error)"
        ))
        .map_err(error)?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let p = *first_page.borrow();
            require(!p.invalid)?;
            if p.ready(1) && p.rendered && p.initial_display && p.bootstrap_advanced {
                return Ok::<(), String>(());
            }
            first_page.changed().await.map_err(error)?;
        }
    })
    .await
    .map_err(|_| "session.peer_initial_display_deadline".to_string())??;
    // Initial and reloaded page compare the same actor. Keep the existing quota.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let old_generation = transfer_view::live_generation(state, owner, id)?;
    #[cfg(feature = "settings-probe")]
    let settings = settings_probe::prepare(state, window, id, old_generation, pages).await?;
    // First hidden output was parsed before Begin reached the DOM. It now
    // needs to arrive as ordered Data and ACK after the captured Commit.
    require(pages.snapshot().bootstrap_advanced)?;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let projected = handle.inspect().await.map_err(error)?;
            let current = state.0.registry.state(owner, id).map_err(error)?;
            let text = projected
                .cells
                .iter()
                .flatten()
                .map(|c| c.text.as_str())
                .collect::<String>();
            if text.contains("HIDDEN_SEED 中文🌍")
                && transfer_view::live_data_acked(state, owner, id, old_generation)?
                    .is_some_and(|end| end >= current.parsed_bytes)
            {
                return Ok::<(), String>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "session.peer_live_data_ack_deadline".to_string())??;
    state
        .0
        .registry
        .request_marker(owner, id, old_generation)
        .await
        .map_err(error)?;
    let mut live = pages.events.subscribe();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let p = *live.borrow();
            require(!p.invalid)?;
            if p.ready(1) && p.live_display {
                return Ok::<(), String>(());
            }
            live.changed().await.map_err(error)?;
        }
    })
    .await
    .map_err(|_| "session.peer_live_marker_deadline".to_string())??;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let nonce_for_hold = pages.injection()?.0;
    window.eval(format!("void window.__PULSE_PEER_HOLD_ACK__('{run_nonce}','{nonce_for_hold}','{old_generation}').catch(console.error)")).map_err(error)?;
    let mut hold = pages.events.subscribe();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let p = *hold.borrow();
            require(!p.invalid)?;
            if p.hold_ready {
                return Ok::<(), String>(());
            }
            hold.changed().await.map_err(error)?;
        }
    })
    .await
    .map_err(|_| "session.peer_ack_hold_setup_deadline".to_string())??;
    state
        .0
        .registry
        .input_bound(owner, id, old_generation, b"hidden\n".to_vec())
        .await
        .map_err(error)?;
    let pending = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Some(pending) =
                transfer_view::live_data_pending(state, owner, id, old_generation)?
            {
                return Ok::<_, String>(pending);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "session.peer_no_pending_data_ack".to_string())??;
    tokio::time::sleep(Duration::from_millis(100)).await;
    require(transfer_view::live_data_pending(state, owner, id, old_generation)? == Some(pending))?;
    pages.pending_ack_observed()?;
    let before_pending_drain = state.0.registry.state(owner, id).map_err(error)?;
    // No explicit teardown here: actual Started must revoke nonce and retire this
    // still-open subscription. Finished/newdoc DOM alone cannot prove teardown.
    let old_nonce = pages.injection()?.0;
    state
        .0
        .registry
        .check_binding(owner, id, old_generation)
        .map_err(error)?;
    let mut changed = pages.events.subscribe();
    eprintln!("session.peer_reload.requested");
    window.reload().map_err(error)?;
    // Old relay must retire; live output need not settle before new capture.
    pages.wait_navigation_joined().await?;
    let expected = state.0.registry.state(owner, id).map_err(error)?;
    require(expected.parsed_bytes >= before_pending_drain.parsed_bytes)?;
    pages.allow_refresh_capture()?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let p = *changed.borrow();
            require(!p.invalid)?;
            if p.ready(2)
                && p.rendered
                && p.navigation_retired
                && p.navigation_joined
                && p.live_display
                && p.pending_ack
                && p.refresh_allowed
            {
                return Ok::<(), String>(());
            }
            changed.changed().await.map_err(error)?;
        }
    })
    .await
    .map_err(|_| "session.peer_reload_page_deadline".to_string())??;
    require(
        state.caller(window)? == owner
            && state
                .0
                .registry
                .state(owner, id)
                .map_err(error)?
                .parsed_bytes
                >= expected.parsed_bytes
            && handle.state.borrow().lifecycle == Lifecycle::Running
            && window
                .app_handle()
                .get_webview_window(window.label())
                .is_some(),
    )?;
    require(
        state
            .0
            .registry
            .acknowledge(owner, id, old_generation, 0)
            .is_err()
            && state
                .0
                .registry
                .input_bound(owner, id, old_generation, b"denied\n".to_vec())
                .await
                .is_err(),
    )?;
    require(pages.confirm_nonce(&old_nonce).is_err() && pages.injection()?.0 != old_nonce)?;
    let refreshed_generation = transfer_view::live_generation(state, owner, id)?;
    require(refreshed_generation > old_generation)?;
    #[cfg(feature = "settings-probe")]
    let settings = settings
        .refreshed(state, window, id, refreshed_generation, pages)
        .await?;
    let seed_count = handle
        .inspect()
        .await
        .map_err(error)?
        .cells
        .iter()
        .flatten()
        .map(|c| c.text.as_str())
        .collect::<String>()
        .matches("HIDDEN_SEED 中文🌍")
        .count();
    tokio::time::sleep(Duration::from_millis(500)).await;
    state
        .0
        .registry
        .input_bound(owner, id, refreshed_generation, b"hidden\n".to_vec())
        .await
        .map_err(error)?;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let projected = handle.inspect().await.map_err(error)?;
            let current = state.0.registry.state(owner, id).map_err(error)?;
            let count = projected
                .cells
                .iter()
                .flatten()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .matches("HIDDEN_SEED 中文🌍")
                .count();
            if current.parsed_bytes > expected.parsed_bytes
                && count > seed_count
                && transfer_view::live_data_acked(state, owner, id, refreshed_generation)?
                    .is_some_and(|end| end >= current.parsed_bytes)
            {
                return Ok::<(), String>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "session.peer_fresh_live_data_ack_deadline".to_string())??;
    state
        .0
        .registry
        .request_marker(owner, id, refreshed_generation)
        .await
        .map_err(error)?;
    let mut events = pages.events.subscribe();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let p = *events.borrow();
            require(p.ready(2))?;
            if p.refreshed_live_display {
                return Ok::<(), String>(());
            }
            events.changed().await.map_err(error)?;
        }
    })
    .await
    .map_err(|_| "session.peer_fresh_live_marker_deadline".to_string())??;
    state
        .0
        .registry
        .detach(owner, id, refreshed_generation)
        .map_err(error)?;
    pages.wait_refreshed_joined().await?;
    require(state.0.registry.state(owner, id).map_err(error)?.lifecycle == Lifecycle::Running)?;

    let fresh = transfer::capture(state, owner, id, handle).await?;
    require(fresh.generation > old_generation)?;
    require(pages.snapshot().navigation_joined)?;
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
    #[cfg(feature = "settings-probe")]
    return Ok(settings);
    #[cfg(not(feature = "settings-probe"))]
    Ok(SettingsReceipt)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn refreshed_display_and_join_require_its_own_done_and_marker_not_first_page_receipts() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let old = p.injection().unwrap_or_default().0;
        assert!(p.confirm_nonce(&old).is_ok());
        assert!(p.claim_view(&old).is_ok());
        assert!(p.mark_captured(&old).is_ok());
        assert!(p.mark_rendered(&old).is_ok());
        assert!(p.mark_live_display(&old).is_ok());
        assert!(p.mark_refreshed_joined(&old).is_err());
        p.record(PageLoadEvent::Started);
        p.navigation_retired(Ok(1));
        assert!(p.mark_navigation_joined().is_ok());
        p.record(PageLoadEvent::Finished);
        let current = p.injection().unwrap_or_default().0;
        assert!(p.confirm_nonce(&old).is_err());
        assert!(p.confirm_nonce(&current).is_ok());
        assert_eq!(p.claim_view(&current), Ok(false));
        assert!(!p.is_initial_document());
        assert!(p.mark_live_display(&current).is_err());
        assert!(p.mark_bootstrap_advanced(&old).is_err());
        assert!(p.mark_bootstrap_advanced(&current).is_ok());
        assert!(p.mark_bootstrap_advanced(&current).is_err());
        assert!(p.mark_captured(&current).is_ok());
        let waiter_pages = p.clone();
        let waiter_nonce = current.clone();
        let wait =
            tokio::spawn(async move { waiter_pages.wait_initial_display(&waiter_nonce).await });
        tokio::task::yield_now().await;
        assert!(!wait.is_finished());
        assert!(p.mark_rendered(&current).is_ok());
        assert!(matches!(wait.await, Ok(Ok(()))));
        assert!(p.mark_refreshed_joined(&current).is_err());
        assert!(!p.snapshot().refreshed_live_display);
        assert!(p.mark_live_display(&old).is_err());
        assert!(p.mark_live_display(&current).is_ok());
        assert!(p.mark_live_display(&current).is_err());
        assert!(p.mark_refreshed_joined(&old).is_err());
        assert!(p.mark_refreshed_joined(&current).is_ok());
        assert!(p.mark_refreshed_joined(&current).is_err());
        assert!(p.wait_refreshed_joined().await.is_ok());
    }

    #[test]
    fn pending_ack_retirement_requires_live_hold_and_oldjoin_before_allowing_new_capture() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let old = p.injection().unwrap_or_default().0;
        assert!(p.confirm_nonce(&old).is_ok());
        assert!(p.mark_hold_ready(&old).is_err());
        assert!(p.pending_ack_observed().is_err());
        assert!(p.mark_captured(&old).is_ok());
        assert!(p.mark_rendered(&old).is_ok());
        assert!(p.mark_live_display(&old).is_ok());
        assert!(p.mark_hold_ready(&old).is_ok());
        assert!(p.mark_hold_ready(&old).is_err());
        assert!(p.pending_ack_observed().is_ok());
        assert!(p.pending_ack_observed().is_err());
        assert!(p.allow_refresh_capture().is_err());
        p.record(PageLoadEvent::Started);
        p.navigation_retired(Ok(1));
        assert!(p.mark_hold_ready(&old).is_err());
        assert!(p.allow_refresh_capture().is_err());
        assert!(p.mark_navigation_joined().is_ok());
        assert!(p.allow_refresh_capture().is_ok());
        assert!(p.allow_refresh_capture().is_err());
        assert!(p.snapshot().refresh_allowed);
    }

    #[tokio::test]
    async fn initial_display_barrier_does_not_release_on_capture_alone_or_reauthorize_after_navigation()
     {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let nonce = p.injection().unwrap_or_default().0;
        assert!(p.confirm_nonce(&nonce).is_ok());
        assert!(p.mark_captured(&nonce).is_ok());
        let waiting_pages = p.clone();
        let waiting_nonce = nonce.clone();
        let waiting =
            tokio::spawn(async move { waiting_pages.wait_initial_display(&waiting_nonce).await });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        assert!(p.mark_rendered(&nonce).is_ok());
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), waiting).await,
            Ok(Ok(Ok(())))
        ));
        p.record(PageLoadEvent::Started);
        assert!(p.wait_initial_display(&nonce).await.is_err());
    }

    #[test]
    fn capture_claim_is_single_flight_per_authenticated_document_and_cannot_replay_after_started() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let old = p.injection().unwrap_or_default().0;
        assert!(p.claim_view(&old).is_err());
        assert!(p.confirm_nonce(&old).is_ok());
        assert_eq!(p.claim_view(&old), Ok(true));
        assert!(p.claim_view(&old).is_err());
        assert!(p.mark_live_display(&old).is_err());
        assert!(p.mark_captured(&old).is_ok());
        assert!(p.mark_rendered(&old).is_ok());
        assert!(p.mark_live_display(&old).is_ok());
        assert!(p.mark_live_display(&old).is_err());
        assert!(p.mark_navigation_joined().is_err());
        p.record(PageLoadEvent::Started);
        p.navigation_retired(Ok(1));
        assert!(p.claim_view(&old).is_err());
        assert!(p.mark_live_display(&old).is_err());
        p.record(PageLoadEvent::Finished);
        let fresh = p.injection().unwrap_or_default().0;
        assert!(p.claim_view(&fresh).is_err());
        assert!(p.confirm_nonce(&old).is_err());
        assert!(p.confirm_nonce(&fresh).is_ok());
        assert_eq!(p.claim_view(&fresh), Ok(false));
        assert!(p.claim_view(&fresh).is_err());
        assert!(p.snapshot().live_display);
        assert!(p.mark_navigation_joined().is_ok());
        assert!(p.mark_navigation_joined().is_err());
    }
    #[tokio::test]
    async fn new_capture_join_barrier_does_not_release_before_old_native_view_receipt() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        p.record(PageLoadEvent::Started);
        p.navigation_retired(Ok(1));
        let copy = p.clone();
        let waiting = tokio::spawn(async move { copy.wait_navigation_joined().await });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        assert!(p.mark_navigation_joined().is_ok());
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), waiting).await,
            Ok(Ok(Ok(())))
        ));
    }

    #[test]
    fn initial_readonly_display_is_not_reused_as_fresh_page_capture_or_done() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.navigation_retired(Ok(0));
        p.record(PageLoadEvent::Finished);
        let old = p.injection().unwrap_or_default().0;
        assert!(p.authorize_view(&old).is_err());
        assert!(p.confirm_nonce(&old).is_ok());
        assert!(p.authorize_view(&old).is_ok());
        assert!(p.mark_captured(&old).is_ok());
        assert!(p.mark_rendered(&old).is_ok());
        assert!(p.snapshot().initial_display);
        assert!(p.view_finished());
        p.record(PageLoadEvent::Started);
        p.navigation_retired(Ok(1));
        assert!(p.snapshot().initial_display);
        assert!(!p.snapshot().rendered);
        assert!(!p.view_finished());
        assert!(p.mark_captured(&old).is_err());
        assert!(p.mark_rendered(&old).is_err());
        p.record(PageLoadEvent::Finished);
        let fresh = p.injection().unwrap_or_default().0;
        assert!(p.authorize_view(&fresh).is_err());
        assert!(p.confirm_nonce(&old).is_err());
        assert!(p.confirm_nonce(&fresh).is_ok());
        assert!(p.authorize_view(&fresh).is_ok());
        assert!(!p.snapshot().rendered);
        assert!(p.mark_rendered(&fresh).is_err());
        assert!(p.mark_captured(&fresh).is_ok());
        assert!(p.mark_rendered(&fresh).is_ok());
        assert!(p.snapshot().rendered);
        assert!(p.snapshot().initial_display);
    }

    #[test]
    fn navigation_retirement_receipt_requires_the_actual_second_started_phase_and_one_view() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.navigation_retired(Ok(0));
        p.record(PageLoadEvent::Finished);
        p.record(PageLoadEvent::Started);
        assert!(!p.snapshot().navigation_retired);
        p.navigation_retired(Ok(1));
        assert!(p.snapshot().navigation_retired);
        assert!(!p.snapshot().invalid);
        p.record(PageLoadEvent::Finished);
        assert!(p.snapshot().navigation_retired);
        for result in [Ok(0), Ok(2), Err("failed".to_string())] {
            let failed = PageTracker::new();
            failed.record(PageLoadEvent::Started);
            failed.navigation_retired(Ok(0));
            failed.record(PageLoadEvent::Finished);
            failed.record(PageLoadEvent::Started);
            failed.navigation_retired(result);
            assert!(failed.snapshot().invalid);
            assert!(!failed.snapshot().navigation_retired);
        }
        let early = PageTracker::new();
        early.navigation_retired(Ok(1));
        assert!(early.snapshot().invalid);
    }

    #[test]
    fn readonly_view_requires_authenticated_fresh_page_and_rendered_receipt_is_single_use() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let old = p.injection().unwrap_or_default().0;
        assert!(p.authorize_view(&old).is_err());
        assert!(p.confirm_nonce(&old).is_ok());
        assert!(p.authorize_view(&old).is_ok());
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
                navigation_retired: false,
                initial_display: false,
                bootstrap_advanced: false,
                claimed: false,
                live_display: false,
                navigation_joined: false,
                hold_ready: false,
                pending_ack: false,
                refresh_allowed: false,
                refreshed_live_display: false,
                refreshed_bootstrap_advanced: false,
                refreshed_joined: false,
                shell_hints_checked: false,
                initial_shell_hints_checked: false,
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
                navigation_retired: false,
                initial_display: false,
                bootstrap_advanced: false,
                claimed: false,
                live_display: false,
                navigation_joined: false,
                hold_ready: false,
                pending_ack: false,
                refresh_allowed: false,
                refreshed_live_display: false,
                refreshed_bootstrap_advanced: false,
                refreshed_joined: false,
                shell_hints_checked: false,
                initial_shell_hints_checked: false,
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

    #[cfg(feature = "login-probe")]
    #[test]
    fn confirmed_page_action_holds_navigation_lock_and_old_scope_cannot_revoke_successor()
    -> Result<()> {
        use pulse_app::registry::{RegistryError, SessionRegistry};
        let registry = SessionRegistry::new();
        let owner = registry.open_window().map_err(error)?;
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        p.record(PageLoadEvent::Finished);
        let old_nonce = p.injection()?.0;
        assert!(p.with_ready_page(&old_nonce, || Ok(())).is_err());
        p.confirm_nonce(&old_nonce)?;
        let old = p.with_ready_page(&old_nonce, || {
            assert!(p.nonce.try_lock().is_err());
            registry.activate_launch_page(owner).map_err(error)
        })?;
        let ticket = registry.begin_page_local_launch(&old).map_err(error)?;
        p.record(PageLoadEvent::Started);
        registry.retire_window_views(owner).map_err(error)?;
        assert!(matches!(
            registry.begin_page_local_launch(&old),
            Err(RegistryError::Forbidden)
        ));
        drop(ticket);
        p.record(PageLoadEvent::Finished);
        let fresh = p.injection()?.0;
        assert!(p.with_ready_page(&old_nonce, || Ok(())).is_err());
        assert!(p.confirm_nonce(&old_nonce).is_err());
        p.confirm_nonce(&fresh)?;
        let current = p.with_ready_page(&fresh, || {
            registry.activate_launch_page(owner).map_err(error)
        })?;
        drop(old);
        drop(registry.begin_page_local_launch(&current).map_err(error)?);
        Ok(())
    }
}
