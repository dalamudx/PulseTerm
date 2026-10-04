//! Local-only session validation. No user shell, remote navigation or frontend owner token.
#[path = "encoding_probe.rs"]
pub(crate) mod encoding_probe;
#[path = "window_peer_view.rs"]
pub(crate) mod peer_view;
#[path = "window_reload_probe.rs"]
mod reload;
#[path = "window_transfer_probe.rs"]
mod transfer;
#[path = "window_transfer_view.rs"]
pub(crate) mod transfer_view;
use crate::raw_frame::RawFrame;
use pulse_app::attachment::AttachmentConfig;
use pulse_app::registry::{
    RenderedScreen, RepairAdmission, SessionId, SessionRegistry, VisibleComparison, WindowToken,
};
use pulse_app::spawn_local;
use pulse_core::TermSize;
use pulse_pty::PtyCommand;
use pulse_term::session::{Lifecycle, SessionHandle, SessionState};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tokio::sync::{Semaphore, oneshot};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone)]
pub(crate) struct WindowSessions(Arc<Inner>);
struct Inner {
    registry: SessionRegistry,
    windows: Mutex<HashMap<String, WindowToken>>,
    observers: Mutex<HashMap<WindowToken, Arc<crate::native_visibility::NativeWatcher>>>,
    active: Mutex<Option<Active>>,
    transfer_view: Mutex<Option<transfer_view::Current>>,
    peer_view_slot: Arc<Semaphore>,
    transfer_view_slot: Arc<Semaphore>,
    slot: Arc<Semaphore>,
    minimize: bool,
    dual: bool,
    encoding: bool,
    external_control: Option<std::path::PathBuf>,
}
struct Active {
    owner: WindowToken,
    nonce: String,
    id: SessionId,
    peer: Option<oneshot::Sender<bool>>,
    peer_pages: reload::PageTracker,
    peer_owner: Option<WindowToken>,
    peer_id: Option<SessionId>,
    visibility: Arc<crate::viewer_visibility::VisibilityGate>,
}
impl Active {
    fn new(
        owner: WindowToken,
        nonce: String,
        id: SessionId,
        visibility: Arc<crate::viewer_visibility::VisibilityGate>,
    ) -> Self {
        Self {
            owner,
            nonce,
            id,
            visibility,
            peer: None,
            peer_pages: reload::PageTracker::new(),
            peer_owner: None,
            peer_id: None,
        }
    }
    fn peer_view(
        &self,
        owner: WindowToken,
        nonce: &str,
        page_nonce: &str,
    ) -> Result<(SessionId, reload::PageTracker)> {
        if self.nonce != nonce || self.peer_owner != Some(owner) || self.owner == owner {
            return Err("session.forbidden".into());
        }
        self.peer_pages.authorize_view(page_nonce)?;
        Ok((
            self.peer_id.ok_or("session.peer_not_registered")?,
            self.peer_pages.clone(),
        ))
    }

    // Only the trusted builder callback binds the fixed peer incarnation. Never a JS label/token.
    fn bind_peer(&mut self, owner: WindowToken) -> Result<()> {
        if owner == self.owner || self.peer_owner.is_some_and(|known| known != owner) {
            return Err("session.forbidden".into());
        }
        self.peer_owner = Some(owner);
        Ok(())
    }
    fn admit_peer(
        &mut self,
        owner: WindowToken,
        nonce: &str,
        page_nonce: &str,
    ) -> Result<(SessionId, Option<oneshot::Sender<bool>>)> {
        if self.peer_owner != Some(owner) || self.owner == owner || self.nonce != nonce {
            return Err("session.forbidden".into());
        }
        self.peer_pages.confirm_nonce(page_nonce)?;
        Ok((self.id, self.peer.take()))
    }
}

impl Default for WindowSessions {
    fn default() -> Self {
        Self::new(false, None, false, false)
    }
}
impl WindowSessions {
    pub(crate) fn new(
        minimize: bool,
        external_control: Option<std::path::PathBuf>,
        dual: bool,
        encoding: bool,
    ) -> Self {
        Self(Arc::new(Inner {
            registry: SessionRegistry::new(),
            windows: Mutex::new(HashMap::new()),
            observers: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            transfer_view: Mutex::new(None),
            peer_view_slot: Arc::new(Semaphore::new(1)),
            transfer_view_slot: Arc::new(Semaphore::new(1)),
            slot: Arc::new(Semaphore::new(1)),
            minimize,
            dual,
            encoding,
            external_control,
        }))
    }
}
impl Inner {
    async fn attach_codec(
        &self,
        owner: WindowToken,
        id: SessionId,
        config: AttachmentConfig,
    ) -> std::result::Result<
        pulse_app::registry::RegisteredAttachment,
        pulse_app::registry::RegistryError,
    > {
        if self.dual {
            self.registry.attach_v2(owner, id, config).await
        } else {
            self.registry.attach(owner, id, config).await
        }
    }
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_session_protocol(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
) -> Result<u8> {
    if !local_url(&window.url().map_err(error)?) {
        return Err("session.remote_origin".into());
    }
    Ok(if state.0.dual { 2 } else { 1 })
}

fn error(_: impl std::fmt::Display) -> String {
    "session.probe_failed".into()
}
fn local_url(url: &tauri::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && matches!(
            (url.scheme(), url.host_str()),
            ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost"))
        )
}
impl WindowSessions {
    pub(crate) fn install(&self, window: &WebviewWindow) -> Result<WindowToken> {
        let label = window.label().to_string();
        let mut windows = self.0.windows.lock().map_err(error)?;
        if let Some(token) = windows.get(&label) {
            return Ok(*token);
        }
        let token = self.0.registry.open_window().map_err(error)?;
        let observer = crate::native_visibility::NativeWatcher::new();
        self.0
            .observers
            .lock()
            .map_err(error)?
            .insert(token, observer.clone());
        windows.insert(label.clone(), token);
        observer.launch(window.clone());
        let state = self.clone();
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                observer.close();
                let _ = state.revoke(&label, token);
            } else if matches!(
                event,
                WindowEvent::Focused(_)
                    | WindowEvent::Resized(_)
                    | WindowEvent::ScaleFactorChanged { .. }
            ) {
                observer.hint();
            }
        });
        Ok(token)
    }
    fn revoke(&self, label: &str, token: WindowToken) -> Result<()> {
        let mut windows = self.0.windows.lock().map_err(error)?;
        if windows.get(label) == Some(&token) {
            windows.remove(label);
        }
        // Release window metadata before touching active/gate locks. Destroyed is
        // terminal; CloseRequested may be prevented and is not a revoke signal.
        drop(windows);
        if let Some(observer) = self.0.observers.lock().map_err(error)?.remove(&token) {
            observer.close();
        }
        if let Some(run) = self
            .0
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|r| r.owner == token)
        {
            run.visibility.close();
        }
        self.0.registry.close_window(token).map_err(error)
    }
    fn caller(&self, window: &WebviewWindow) -> Result<WindowToken> {
        if !local_url(&window.url().map_err(error)?) {
            return Err("session.forbidden_origin".into());
        }
        self.0
            .windows
            .lock()
            .map_err(error)?
            .get(window.label())
            .copied()
            .ok_or("session.unknown_window".into())
    }
    fn owned(&self, window: &WebviewWindow, nonce: &str) -> Result<(WindowToken, SessionId)> {
        let owner = self.caller(window)?;
        let active = self.0.active.lock().map_err(error)?;
        let run = active
            .as_ref()
            .filter(|r| r.nonce == nonce && r.owner == owner)
            .ok_or("session.forbidden".to_string())?;
        Ok((owner, run.id))
    }
    pub(crate) fn shutdown(&self) {
        if let Ok(mut observers) = self.0.observers.lock() {
            for (_, observer) in observers.drain() {
                observer.close();
            }
        }
        let _ = self.0.registry.shutdown();
    }
}
// Finite static helper, not arbitrary code execution. Native PTY output is Unicode
// and intentionally ends inside a scalar to exercise genuine EOF decoder flushing.
pub(crate) fn child() -> std::io::Result<()> {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(40));
        std::process::exit(2);
    });
    std::io::stdout().write_all("RESIZE_SEED 中文🌍\r\n".as_bytes())?;
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    while line.trim_end() == "hidden" {
        std::io::stdout().write_all("HIDDEN_SEED 中文🌍\r\n".as_bytes())?;
        std::io::stdout().flush()?;
        line.clear();
        std::io::stdin().lock().read_line(&mut line)?;
    }
    if line.trim_end() != "go" {
        return Err(std::io::Error::other("invalid probe input"));
    }
    let mut out = std::io::stdout().lock();
    out.write_all(b"\x1b[2J\x1b[H")?;
    let block = "hello 中文🌍\r\n".repeat(4096);
    for _ in 0..40 {
        out.write_all(block.as_bytes())?;
    }
    out.write_all(b"MARKER_READY\r\n\x1b[31")?;
    out.flush()?;
    line.clear();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim_end() != "m" {
        return Err(std::io::Error::other("invalid resume input"));
    }
    out.write_all(b"mINPUT_READY\r\n")?;
    out.flush()?;
    line.clear();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim_end() != "typed中文🌍" {
        return Err(std::io::Error::other("invalid keyboard sample"));
    }
    out.write_all("INPUT_OK typed中文🌍\r\n".as_bytes())?;
    out.write_all(b"EOF\x1b[0m\xe4\xb8")?;
    out.flush()
}
async fn native() -> Result<(SessionHandle, pulse_app::LocalSessionTask)> {
    let mut command = PtyCommand::new(std::env::current_exe().map_err(error)?);
    command.args.push("--session-probe-child".into());
    let (handle, task) = spawn_local(command, TermSize::new(40, 12).map_err(error)?, 128)
        .await
        .map_err(error)?;
    let mut observed = handle.state.clone();
    tokio::time::timeout(Duration::from_secs(2), async {
        while observed.borrow().parsed_bytes < "RESIZE_SEED 中文🌍\r\n".len() as u64 {
            observed.changed().await.map_err(error)?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(error)??;
    Ok((handle, task))
}
struct RunGuard {
    state: WindowSessions,
    owner: WindowToken,
    id: SessionId,
    handle: SessionHandle,
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        let _ = self.state.0.registry.close_session(self.owner, self.id);
        self.handle.close();
        reap_later(self.state.clone(), self.id, self.handle.clone());
        if let Ok(mut active) = self.state.0.active.lock()
            && active.as_ref().is_some_and(|run| run.id == self.id)
        {
            *active = None;
        }
    }
}
fn reap_later(state: WindowSessions, id: SessionId, handle: SessionHandle) {
    tauri::async_runtime::spawn(async move {
        let mut observed = handle.state.clone();
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            while observed.borrow().lifecycle == Lifecycle::Running {
                if observed.changed().await.is_err() {
                    return;
                }
            }
            let _ = state.0.registry.reap_closed(id);
        })
        .await;
    });
}
struct PeerGuard {
    state: WindowSessions,
    window: WebviewWindow,
    handle: SessionHandle,
    id: Option<SessionId>,
}
impl Drop for PeerGuard {
    fn drop(&mut self) {
        let _ = self.window.close();
        self.handle.close();
        if let Some(id) = self.id {
            reap_later(self.state.clone(), id, self.handle.clone());
        }
    }
}
#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionEvidence {
    pub attachment_version: u8,
    pub generation: String,
    pub digest: String,
    pub revision: String,
    pub parsed_bytes: String,
    pub frames: u32,
    pub peak_credit_bytes: u32,
    pub native_exit: u32,
    pub peer_denied: bool,
    pub peer_transfer_checked: bool,
    pub peer_display_checked: bool,
    pub peer_reload_checked: bool,
    pub peer_reload_display_checked: bool,
    pub window_revoked: bool,
    pub session_survived: bool,
    pub idle_checked: bool,
    pub marker_checked: bool,
    pub repair_checked: bool,
    pub input_checked: bool,
    pub resize_count: u32,
    pub pause_count: u32,
    pub native_window_checked: bool,
    pub native_minimize_checked: bool,
    pub native_only_checked: bool,
    pub external_wm_checked: bool,
    pub columns: u16,
    pub rows: u16,
}
impl SessionEvidence {
    pub(crate) fn validate(&self) -> Result<()> {
        parse_u64(&self.generation)?;
        if !matches!(self.attachment_version, 1 | 2)
            || self.resize_count > 3
            || self.pause_count > 2
            || (self.native_window_checked && self.pause_count == 0)
            || (self.native_minimize_checked && !self.native_window_checked)
            || (self.native_only_checked && !self.native_window_checked)
            || (self.external_wm_checked
                && (!self.native_only_checked || !self.native_minimize_checked))
            || !(2..=160).contains(&self.columns)
            || !(2..=60).contains(&self.rows)
        {
            return Err("session.invalid_geometry".into());
        }
        parse_u64(&self.revision)?;
        if parse_u64(&self.parsed_bytes)? < 1024 * 1024
            || self.digest.len() != 16
            || !self
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !(4..=16_384).contains(&self.frames)
            || !(1..=8192 + if self.attachment_version == 2 { 28 } else { 20 })
                .contains(&self.peak_credit_bytes)
            || self.native_exit != 0
            || !self.peer_denied
            || !self.peer_transfer_checked
            || !self.peer_display_checked
            || !self.peer_reload_checked
            || !self.peer_reload_display_checked
            || !self.window_revoked
            || !self.session_survived
            || !self.idle_checked
            || !self.marker_checked
            || !self.repair_checked
            || !self.input_checked
        {
            return Err("session.invalid_evidence".into());
        }
        Ok(())
    }
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_session_ack(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    sequence: u32,
) -> Result<()> {
    let (owner, id) = state.owned(&window, &nonce)?;
    let generation = parse_u64(&generation)?;
    state
        .0
        .registry
        .acknowledge(owner, id, generation, sequence)
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_marker(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
) -> Result<()> {
    let (owner, id) = state.owned(&window, &nonce)?;
    state
        .0
        .registry
        .request_marker(owner, id, parse_u64(&generation)?)
        .await
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_resize(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    columns: u16,
    rows: u16,
) -> Result<()> {
    if !(2..=160).contains(&columns) || !(2..=60).contains(&rows) {
        return Err("session.invalid_geometry".into());
    }
    let (owner, id) = state.owned(&window, &nonce)?;
    state
        .0
        .registry
        .resize_bound(
            owner,
            id,
            parse_u64(&generation)?,
            TermSize::new(columns, rows).map_err(error)?,
        )
        .await
        .map_err(error)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_native_snapshot(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
) -> Result<crate::native_visibility::NativeVisibilitySample> {
    let (owner, id) = state.owned(&window, &nonce)?;
    state.0.registry.state(owner, id).map_err(error)?;
    let observer = state
        .0
        .observers
        .lock()
        .map_err(error)?
        .get(&owner)
        .cloned()
        .ok_or("session.unknown_window")?;
    let sample = observer.sample(window.clone()).await?;
    if state.caller(&window)? != owner {
        return Err("session.window_replaced".into());
    }
    Ok(sample)
}

fn visibility_gate(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
) -> Result<Arc<crate::viewer_visibility::VisibilityGate>> {
    let active = state.0.active.lock().map_err(error)?;
    active
        .as_ref()
        .filter(|r| r.owner == owner && r.id == id)
        .map(|r| r.visibility.clone())
        .ok_or("session.forbidden".into())
}
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_session_visibility(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    visible: bool,
) -> Result<()> {
    let (owner, id) = state.owned(&window, &nonce)?;
    let generation = parse_u64(&generation)?;
    let gate = visibility_gate(&state, owner, id)?;
    state.0.registry.state(owner, id).map_err(error)?;
    if visible {
        gate.resume(generation)
    } else {
        gate.pause(generation, || {
            state
                .0
                .registry
                .detach(owner, id, generation)
                .map_err(error)
        })
    }
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_hidden_output(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
) -> Result<String> {
    let (owner, id) = state.owned(&window, &nonce)?;
    let gate = visibility_gate(&state, owner, id)?;
    let generation = parse_u64(&generation)?;
    hidden_output(&state, owner, id, &gate, generation).await
}
async fn hidden_output(
    state: &WindowSessions,
    owner: WindowToken,
    id: SessionId,
    gate: &crate::viewer_visibility::VisibilityGate,
    generation: u64,
) -> Result<String> {
    gate.hidden(generation)?;
    let before = state
        .0
        .registry
        .state(owner, id)
        .map_err(error)?
        .parsed_bytes;
    state
        .0
        .registry
        .input(owner, id, b"hidden\n".to_vec())
        .await
        .map_err(error)?;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            gate.hidden(generation)?;
            let current = state.0.registry.state(owner, id).map_err(error)?;
            if current.lifecycle != Lifecycle::Running {
                return Err("session.closed_while_hidden".into());
            }
            if current.parsed_bytes >= before + "HIDDEN_SEED 中文🌍\r\n".len() as u64 {
                return Ok(current.parsed_bytes.to_string());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(error)?
}

/// Restores only this runtime window incarnation on cancellation/failure. Never
/// reopens a destroyed window or resumes the terminal stream from Drop.
struct NativeVisibilityGuard {
    window: WebviewWindow,
    state: WindowSessions,
    owner: WindowToken,
    armed: bool,
    minimize: bool,
}
impl Drop for NativeVisibilityGuard {
    fn drop(&mut self) {
        if self.armed && self.state.caller(&self.window).ok() == Some(self.owner) {
            if self.minimize {
                let _ = self.window.unminimize();
            }
            let _ = self.window.show();
        }
    }
}
#[derive(Clone, Copy)]
enum NativeVisibility {
    Visible,
    Hidden,
    Minimized,
}
fn native_state_matches(visible: bool, minimized: bool, expected: NativeVisibility) -> bool {
    match expected {
        NativeVisibility::Visible => visible && !minimized,
        NativeVisibility::Hidden => !visible && !minimized,
        NativeVisibility::Minimized => minimized,
    }
}

async fn wait_native_visible(
    window: &WebviewWindow,
    state: &WindowSessions,
    owner: WindowToken,
    expected: NativeVisibility,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if state.caller(window)? != owner {
                return Err("session.window_replaced".into());
            }
            if native_state_matches(
                window.is_visible().map_err(error)?,
                window.is_minimized().map_err(error)?,
                expected,
            ) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "session.native_visibility_timeout".to_string())?
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_window_cycle(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
) -> Result<String> {
    run_native_cycle(window, state.inner(), nonce, generation, false).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_native_cycle(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
) -> Result<String> {
    run_native_cycle(window, state.inner(), nonce, generation, true).await
}
// Private temp directory is supplied only by the isolated test wrapper/CLI.
// Marker contains a fixed action and decimal generation, never shell source.
fn request_external(state: &WindowSessions, generation: u64, minimize: bool) -> Result<()> {
    let dir = state
        .0
        .external_control
        .as_ref()
        .ok_or("session.external_controller_missing")?;
    let text = format!(
        "{}:{generation}",
        if minimize { "minimize" } else { "restore" }
    );
    std::fs::write(dir.join("request.tmp"), text).map_err(error)?;
    std::fs::rename(dir.join("request.tmp"), dir.join("request")).map_err(error)
}

async fn run_native_cycle(
    window: WebviewWindow,
    state: &WindowSessions,
    nonce: String,
    generation: String,
    external: bool,
) -> Result<String> {
    let (owner, id) = state.owned(&window, &nonce)?;
    let generation = parse_u64(&generation)?;
    let gate = visibility_gate(state, owner, id)?;
    state.0.registry.state(owner, id).map_err(error)?;
    if external {
        state
            .0
            .registry
            .check_binding(owner, id, generation)
            .map_err(error)?;
        gate.reserve_external(generation)?;
    } else {
        gate.begin_native(generation)?;
    }
    if !window.is_visible().map_err(error)? || window.is_minimized().map_err(error)? {
        return Err("session.window_not_visible".into());
    }
    let mut guard = NativeVisibilityGuard {
        window: window.clone(),
        state: state.clone(),
        owner,
        armed: true,
        minimize: state.0.minimize,
    };
    if state.0.external_control.is_some() {
        request_external(state, generation, true)?;
    } else if state.0.minimize {
        window.minimize().map_err(error)?;
    } else {
        window.hide().map_err(error)?;
    }
    wait_native_visible(
        &window,
        state,
        owner,
        if state.0.minimize {
            NativeVisibility::Minimized
        } else {
            NativeVisibility::Hidden
        },
    )
    .await?;
    if external {
        tokio::time::timeout(Duration::from_secs(2), gate.wait_external_pause(generation))
            .await
            .map_err(|_| "session.native_pause_timeout".to_string())??;
    }
    // This backend deadline and restoration do not depend on animation frames,
    // document visibility events, or JavaScript running while unmapped.
    let boundary = hidden_output(state, owner, id, &gate, generation).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    gate.hidden(generation)?;
    if state.0.external_control.is_some() {
        request_external(state, generation, false)?;
    } else {
        if state.0.minimize {
            window.unminimize().map_err(error)?;
        }
        window.show().map_err(error)?;
    }
    wait_native_visible(&window, state, owner, NativeVisibility::Visible).await?;
    if state.0.registry.state(owner, id).map_err(error)?.lifecycle != Lifecycle::Running {
        return Err("session.closed_while_hidden".into());
    }
    gate.finish_native(generation)?;
    guard.armed = false;
    Ok(boundary)
}

/// Revoke exactly this viewer generation; registry never closes the session.
#[tauri::command]
#[specta::specta]
pub(crate) fn probe_session_detach(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
) -> Result<()> {
    let (owner, id) = state.owned(&window, &nonce)?;
    state
        .0
        .registry
        .detach(owner, id, parse_u64(&generation)?)
        .map_err(error)
}

fn validate_probe_input(text: &str) -> Result<()> {
    if text.is_empty() || text.len() > 1024 || !text.chars().all(|c| "typed中文🌍\r".contains(c))
    {
        return Err("session.invalid_input".into());
    }
    Ok(())
}

/// Finite sample-only input, NOT a general session/user-shell endpoint.
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_input(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    text: String,
) -> Result<()> {
    validate_probe_input(&text)?;
    let (owner, id) = state.owned(&window, &nonce)?;
    state
        .0
        .registry
        .input_bound(owner, id, parse_u64(&generation)?, text.into_bytes())
        .await
        .map_err(error)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_resume(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
) -> Result<()> {
    let (owner, id) = state.owned(&window, &nonce)?;
    state
        .0
        .registry
        .input(owner, id, b"m\n".to_vec())
        .await
        .map_err(error)
}
/// Explicit small control DTO; terminal bytes remain exclusively on Raw Channel.
#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RenderedProjection {
    pub columns: u16,
    pub rows: u16,
    pub revision: String,
    pub parsed_bytes: String,
    pub digest: String,
}
#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ComparisonEvidence {
    Equal,
    Mismatch,
    Stale,
}
fn parse_counter(value: &str) -> Result<u64> {
    let parsed = value.parse::<u64>().map_err(error)?;
    if parsed.to_string() != value {
        return Err("session.invalid_integer".into());
    }
    Ok(parsed)
}
#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RepairEvidence {
    Equal,
    Stale,
    Queued,
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_repair(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    rendered: RenderedProjection,
) -> Result<RepairEvidence> {
    let (owner, id) = state.owned(&window, &nonce)?;
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
    let admitted = state
        .0
        .registry
        .request_repair(owner, id, parse_u64(&generation)?, rendered)
        .await
        .map_err(error)?;
    Ok(match admitted {
        RepairAdmission::Equal => RepairEvidence::Equal,
        RepairAdmission::Stale => RepairEvidence::Stale,
        RepairAdmission::Queued => RepairEvidence::Queued,
    })
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_compare(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    generation: String,
    rendered: RenderedProjection,
) -> Result<ComparisonEvidence> {
    let (owner, id) = state.owned(&window, &nonce)?;
    let generation = parse_u64(&generation)?;
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
    let comparison = state
        .0
        .registry
        .compare_visible(owner, id, generation, rendered)
        .await
        .map_err(error)?;
    Ok(match comparison {
        VisibleComparison::Equal => ComparisonEvidence::Equal,
        VisibleComparison::Mismatch => ComparisonEvidence::Mismatch,
        VisibleComparison::Stale => ComparisonEvidence::Stale,
    })
}
fn parse_u64(value: &str) -> Result<u64> {
    let parsed = value.parse::<u64>().map_err(error)?;
    if parsed == 0 || parsed.to_string() != value {
        return Err("session.invalid_integer".into());
    }
    Ok(parsed)
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_go(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
) -> Result<()> {
    let (owner, id) = state.owned(&window, &nonce)?;
    state
        .0
        .registry
        .input(owner, id, b"go\n".to_vec())
        .await
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session_peer(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
) -> Result<()> {
    let owner = state.caller(&window)?;
    let (id, sender) = {
        let mut active = state.0.active.lock().map_err(error)?;
        let run = active.as_mut().ok_or("session.forbidden")?;
        run.admit_peer(owner, &nonce, &page_nonce)?
    };
    let denied = state
        .0
        .registry
        .input(owner, id, b"go\n".to_vec())
        .await
        .is_err()
        && state
            .0
            .registry
            .input_bound(owner, id, 1, b"typed".to_vec())
            .await
            .is_err()
        && state.0.registry.acknowledge(owner, id, 1, 0).is_err()
        && state.0.registry.request_marker(owner, id, 1).await.is_err()
        && state
            .0
            .registry
            .request_repair(
                owner,
                id,
                1,
                RenderedScreen {
                    boundary: SessionState {
                        size: TermSize::new(32, 8).map_err(error)?,
                        lifecycle: Lifecycle::Running,
                        revision: 0,
                        parsed_bytes: 0,
                        utf8_bytes: 0,
                        source_is_utf8: true,
                    },
                    digest: "0000000000000000".into(),
                },
            )
            .await
            .is_err();
    if let Some(sender) = sender {
        let _ = sender.send(denied);
    }
    Ok(())
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_session(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    channel: Channel<RawFrame>,
    transfer_channel: Channel<RawFrame>,
) -> Result<SessionEvidence> {
    let nonce = uuid::Uuid::parse_str(&nonce).map_err(error)?.to_string();
    let state = state.inner().clone();
    let owner = state.caller(&window)?;
    let _slot = state.0.slot.clone().try_acquire_owned().map_err(error)?;
    let (handle, task) = native().await?;
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
    let (peer_tx, peer_rx) = oneshot::channel();
    let pages = reload::PageTracker::new();
    let visibility = Arc::new(crate::viewer_visibility::VisibilityGate::default());
    *state.0.active.lock().map_err(error)? = Some(Active {
        peer: Some(peer_tx),
        peer_pages: pages.clone(),
        ..Active::new(owner, nonce.clone(), id, visibility.clone())
    });
    let (peer_handle, peer_task) = native().await?;
    // Each window incarnation is installed on the runtime, never from a frontend label.
    let peer_state = state.clone();
    let loaded_pages = pages.clone();

    let peer = WebviewWindowBuilder::new(
        window.app_handle(),
        "session-peer",
        WebviewUrl::App("index.html?session-peer=1".into()),
    )
    .visible(true)
    .title("PulseTerm readonly reload validation")
    .inner_size(600.0,400.0)
    .on_page_load(move |peer, payload| {
        loaded_pages.record(payload.event());
        let bound = if matches!(payload.event(),tauri::webview::PageLoadEvent::Finished) {
            peer_state.install(&peer).and_then(|owner|{
                if peer_state.caller(&peer)?!=owner{return Err("session.forbidden".into());}
                let mut active=peer_state.0.active.lock().map_err(error)?;
                let run=active.as_mut().filter(|r|r.nonce==nonce).ok_or("session.forbidden")?;
                run.bind_peer(owner)
            })
        } else {Err("session.peer_page_started".to_string())};
        if matches!(payload.event(),tauri::webview::PageLoadEvent::Finished) && bound.is_ok(){
            if let Ok((current,previous))=loaded_pages.injection(){
                let refreshed=previous.is_some();
                let denied=previous.map(|old|format!("let denied=false;try{{await call({{nonce:'{nonce}',pageNonce:'{old}'}})}}catch{{denied=true}}if(!denied)throw new Error('session.peer_old_page_nonce_admitted');")).unwrap_or_default();
                let script=format!("void(async()=>{{const call=p=>window.__TAURI_INTERNALS__.invoke('probe_session_peer',p);{denied}await call({{nonce:'{nonce}',pageNonce:'{current}'}});if({refreshed})await window.__PULSE_PEER_VIEW__('{nonce}','{current}')}})().catch(console.error)");
                if peer.eval(&script).is_err(){eprintln!("session.peer_eval_failed");}
            }else{eprintln!("session.peer_page_not_ready");}
        }
    })
    .build()
    .map_err(error)?;
    let mut peer_guard = PeerGuard {
        state: state.clone(),
        window: peer.clone(),
        handle: peer_handle.clone(),
        id: None,
    };
    let peer_owner = state.install(&peer)?;
    let peer_id = state
        .0
        .registry
        .register(peer_owner, peer_handle.clone())
        .map_err(error)?;
    peer_guard.id = Some(peer_id);
    state
        .0
        .active
        .lock()
        .map_err(error)?
        .as_mut()
        .ok_or("session.forbidden")?
        .peer_id = Some(peer_id);
    let peer_denied = tokio::time::timeout(Duration::from_secs(10), peer_rx)
        .await
        .map_err(|_| "session.peer_timeout".to_string())?
        .map_err(|_| "session.peer_cancelled".to_string())?;
    reload::verify(&state, &peer, peer_owner, peer_id, &peer_handle, &pages).await?;
    // Fixed sample performs two render comparisons on the SAME actor. Respect
    // registry 500ms/session quota across ownership changes; never reset it.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let peer_transfer = transfer::handoff(
        &state,
        &window,
        &peer,
        owner,
        peer_owner,
        peer_id,
        &peer_handle,
    )
    .await?;
    peer.close().map_err(error)?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if state.caller(&peer).is_err()
                && window
                    .app_handle()
                    .get_webview_window(peer.label())
                    .is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "session.window_revoke_timeout".to_string())?;
    let session_survived = peer_handle.state.borrow().lifecycle == Lifecycle::Running
        && state
            .0
            .registry
            .state(owner, peer_id)
            .map_err(error)?
            .lifecycle
            == Lifecycle::Running;
    transfer::after_source_destroyed(
        &state,
        owner,
        peer_id,
        &peer_handle,
        peer_transfer,
        &transfer_channel,
    )
    .await?;
    peer_handle.close();
    tokio::time::timeout(Duration::from_secs(5), peer_task.join())
        .await
        .map_err(|_| "session.peer_join_timeout".to_string())?
        .map_err(|_| "session.peer_join_failed".to_string())?;
    state.0.registry.reap_closed(peer_id).map_err(error)?;
    peer_guard.id = None;
    let config = AttachmentConfig {
        fragment: 8192,
        frames: 1,
        bytes: 8192 + if state.0.dual { 28 } else { 20 },
        ack_timeout: Duration::from_secs(10),
    };
    let mut attachment = state
        .0
        .attach_codec(owner, id, config)
        .await
        .map_err(|_| "session.capture_failed".to_string())?;
    let relay = async {
        let mut resizes = 0;
        let mut pauses = 0;
        loop {
            let generation = attachment.generation;
            while let Some(wire) = attachment.wire.recv().await {
                channel.send(RawFrame(wire)).map_err(error)?;
            }
            let result = attachment
                .completion
                .await
                .map_err(|_| "session.relay_cancelled".to_string())?;
            match result {
                Ok(receipt) => return Ok((generation, receipt, resizes, pauses)),
                Err(pulse_app::attachment::AttachmentError::Invalidated(
                    pulse_term::session::StreamEnd::Reconfigured,
                )) if resizes < 3 => {
                    resizes += 1;
                    attachment = state
                        .0
                        .attach_codec(owner, id, config)
                        .await
                        .map_err(|_| "session.recapture_failed".to_string())?;
                }
                Err(pulse_app::attachment::AttachmentError::Detached) if pauses < 2 => {
                    tokio::time::timeout(
                        Duration::from_secs(5),
                        visibility.wait_resume(generation),
                    )
                    .await
                    .map_err(|_| "session.resume_timeout".to_string())??;
                    if state.0.registry.state(owner, id).map_err(error)?.lifecycle
                        != Lifecycle::Running
                    {
                        return Err("session.closed_while_hidden".into());
                    }
                    pauses += 1;
                    attachment = state
                        .0
                        .attach_codec(owner, id, config)
                        .await
                        .map_err(|_| "session.resume_capture_failed".to_string())?;
                }
                Err(_) => return Err("session.relay_failed".to_string()),
            }
        }
    };
    let (generation, receipt, resizes, pauses) =
        tokio::time::timeout(Duration::from_secs(60), relay)
            .await
            .map_err(error)??;
    let (final_state, screen, native) =
        tokio::time::timeout(Duration::from_secs(5), task.join_with_screen())
            .await
            .map_err(error)?
            .map_err(error)?;
    if native.exit_code != 0 {
        eprintln!(
            "session.native_exit_receipt code={} cancelled={} resizes={}",
            native.exit_code, native.cancelled, native.resize_count
        );
        return Err("session.native_exit_failed".into());
    }
    if native.cancelled {
        return Err("session.native_cancelled".into());
    }
    if native.resize_count != u64::from(resizes) {
        return Err("session.native_resize_mismatch".into());
    }
    if final_state != receipt.boundary {
        return Err("session.boundary_mismatch".into());
    }
    if !peer_denied {
        return Err("session.peer_admitted".into());
    }
    if !session_survived {
        return Err("session.closed_by_window".into());
    }
    Ok(SessionEvidence {
        attachment_version: if state.0.dual { 2 } else { 1 },
        generation: generation.to_string(),
        digest: pulse_term::reconciliation::screen_digest(&screen),
        revision: receipt.boundary.revision.to_string(),
        parsed_bytes: receipt.boundary.parsed_bytes.to_string(),
        frames: u32::try_from(receipt.frames).map_err(error)?,
        peak_credit_bytes: u32::try_from(receipt.peak_credit_bytes).map_err(error)?,
        native_exit: native.exit_code,
        peer_denied,
        peer_transfer_checked: true,
        peer_display_checked: true,
        peer_reload_checked: true,
        peer_reload_display_checked: true,
        window_revoked: true,
        session_survived,
        idle_checked: false,
        marker_checked: false,
        repair_checked: false, // Frontend completes its controlled idle comparison before go.
        input_checked: false,
        resize_count: resizes,
        pause_count: pauses,
        native_window_checked: visibility.native_checked(),
        native_minimize_checked: state.0.minimize && visibility.native_checked(),
        native_only_checked: visibility.external_checked(),
        external_wm_checked: state.0.external_control.is_some() && visibility.external_checked(),
        columns: final_state.size.columns(),
        rows: final_state.size.rows(),
    })
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn peer_admission_requires_the_bound_runtime_incarnation_before_consuming_nonce_or_sender()
     {
        use pulse_term::session::{ChannelTransport, spawn_session};
        let registry = SessionRegistry::new();
        let main = registry.open_window().unwrap();
        let peer = registry.open_window().unwrap();
        let foreign = registry.open_window().unwrap();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = spawn_session(TermSize::new(40, 12).unwrap(), 128, transport);
        let id = registry.register(main, handle).unwrap();
        let pages = reload::PageTracker::new();
        pages.record(tauri::webview::PageLoadEvent::Started);
        pages.record(tauri::webview::PageLoadEvent::Finished);
        let page_nonce = pages.injection().unwrap().0;
        let (tx, mut rx) = oneshot::channel();
        let mut run = Active {
            owner: main,
            nonce: "run".into(),
            id,
            peer: Some(tx),
            peer_owner: None,
            peer_id: None,
            peer_pages: pages,
            visibility: Arc::new(crate::viewer_visibility::VisibilityGate::default()),
        };
        assert!(run.admit_peer(peer, "run", &page_nonce).is_err());
        assert!(run.bind_peer(main).is_err());
        run.bind_peer(peer).unwrap();
        run.bind_peer(peer).unwrap();
        assert!(run.bind_peer(foreign).is_err());
        assert!(run.admit_peer(foreign, "run", &page_nonce).is_err());
        assert!(run.admit_peer(main, "run", &page_nonce).is_err());
        assert!(run.admit_peer(peer, "wrong-run", &page_nonce).is_err());
        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let (accepted, sender) = run.admit_peer(peer, "run", &page_nonce).unwrap();
        assert_eq!(accepted, id);
        sender.unwrap().send(true).unwrap();
        assert!(rx.await.unwrap());
        assert!(run.admit_peer(peer, "run", &page_nonce).is_err());
        registry.close_session(main, id).unwrap();
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

    #[test]
    fn finite_input_validation_uses_utf8_byte_budget_and_rejects_controls() {
        for input in ["typed", "中文🌍", "\r"] {
            assert!(validate_probe_input(input).is_ok());
        }
        for input in ["", "\n", "\u{1b}[31m", "typed;rm", "x", "\u{fffd}"] {
            assert!(validate_probe_input(input).is_err());
        }
        assert!(validate_probe_input(&"t".repeat(1024)).is_ok());
        assert!(validate_probe_input(&"t".repeat(1025)).is_err());
        assert!(validate_probe_input(&"中".repeat(341)).is_ok());
        assert!(validate_probe_input(&"中".repeat(342)).is_err());
    }

    #[test]
    fn native_state_gate_distinguishes_hide_minimize_and_restored_visibility() {
        for visible in [false, true] {
            for minimized in [false, true] {
                assert_eq!(
                    native_state_matches(visible, minimized, NativeVisibility::Minimized),
                    minimized
                );
                assert_eq!(
                    native_state_matches(visible, minimized, NativeVisibility::Hidden),
                    !visible && !minimized
                );
                assert_eq!(
                    native_state_matches(visible, minimized, NativeVisibility::Visible),
                    visible && !minimized
                );
            }
        }
    }
    #[test]
    fn origins_are_local_only() {
        for url in [
            "tauri://localhost/index.html",
            "http://tauri.localhost/index.html?session-peer=1",
            "https://tauri.localhost/",
        ] {
            assert!(local_url(&tauri::Url::parse(url).unwrap()));
        }
        for url in [
            "https://example.com/",
            "http://localhost/",
            "http://tauri.localhost:99/",
            "http://u@tauri.localhost/",
            "file:///tmp/index.html",
            "tauri://localhost.evil/",
        ] {
            assert!(!local_url(&tauri::Url::parse(url).unwrap()));
        }
    }
    #[test]
    fn report_requires_canonical_native_session_evidence() {
        let mut e = SessionEvidence {
            attachment_version: 1,
            generation: "2".into(),
            digest: "9e1323ca657a4985".into(),
            revision: "777".into(),
            parsed_bytes: "3112985".into(),
            frames: 780,
            peak_credit_bytes: 6092,
            native_exit: 0,
            peer_denied: true,
            peer_transfer_checked: true,
            peer_display_checked: true,
            peer_reload_checked: true,
            peer_reload_display_checked: true,
            window_revoked: true,
            session_survived: true,
            idle_checked: true,
            marker_checked: true,
            repair_checked: true,
            input_checked: true,
            resize_count: 0,
            pause_count: 0,
            native_window_checked: false,
            native_minimize_checked: false,
            native_only_checked: false,
            external_wm_checked: false,
            columns: 40,
            rows: 12,
        };
        assert!(e.validate().is_ok());
        e.native_window_checked = true;
        assert!(e.validate().is_err());
        e.pause_count = 1;
        assert!(e.validate().is_ok());
        e.native_minimize_checked = true;
        assert!(e.validate().is_ok());
        e.external_wm_checked = true;
        assert!(e.validate().is_err());
        e.native_only_checked = true;
        assert!(e.validate().is_ok());
        e.external_wm_checked = false;
        e.native_only_checked = false;
        e.native_window_checked = false;
        assert!(e.validate().is_err());
        e.native_minimize_checked = false;
        e.pause_count = 0;
        e.generation = "02".into();
        assert!(e.validate().is_err());
        e.generation = "2".into();
        e.digest = "G".repeat(16);
        assert!(e.validate().is_err());
        e.digest = "9e1323ca657a4985".into();
        e.peak_credit_bytes = 8213;
        assert!(e.validate().is_err());
        e.attachment_version = 2;
        e.peak_credit_bytes = 8220;
        assert!(e.validate().is_ok());
        e.peak_credit_bytes = 8221;
        assert!(e.validate().is_err());
        e.attachment_version = 3;
        assert!(e.validate().is_err());
        e.attachment_version = 1;
        e.peak_credit_bytes = 6092;
        e.marker_checked = false;
        assert!(e.validate().is_err());
        e.marker_checked = true;
        e.idle_checked = false;
        assert!(e.validate().is_err());
        e.idle_checked = true;
        e.input_checked = false;
        assert!(e.validate().is_err());
        e.input_checked = true;
        e.peer_transfer_checked = false;
        assert!(e.validate().is_err());
        e.peer_transfer_checked = true;
        e.peer_display_checked = false;
        assert!(e.validate().is_err());
        e.peer_display_checked = true;
        e.peer_reload_checked = false;
        assert!(e.validate().is_err());
        e.peer_reload_checked = true;
        e.peer_reload_display_checked = false;
        assert!(e.validate().is_err());
        e.peer_reload_display_checked = true;
        e.session_survived = false;
        assert!(e.validate().is_err());
    }
    #[test]
    fn exact_control_integers() {
        assert_eq!(parse_u64("18446744073709551615").unwrap(), u64::MAX);
        assert_eq!(parse_counter("0").unwrap(), 0);
        assert_eq!(parse_counter("18446744073709551615").unwrap(), u64::MAX);
        for n in ["00", "01", "+1", " 1", "18446744073709551616"] {
            assert!(parse_counter(n).is_err());
        }
        for n in ["0", "01", "+1", " 1", "18446744073709551616"] {
            assert!(parse_u64(n).is_err());
        }
    }
}
