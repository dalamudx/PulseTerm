//! Local-only session validation. No user shell, remote navigation or frontend owner token.
use crate::raw_frame::RawFrame;
use pulse_app::attachment::AttachmentConfig;
use pulse_app::registry::{SessionId, SessionRegistry, WindowToken};
use pulse_app::spawn_local;
use pulse_core::TermSize;
use pulse_pty::PtyCommand;
use pulse_term::session::{Lifecycle, SessionHandle};
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
    active: Mutex<Option<Active>>,
    slot: Arc<Semaphore>,
}
struct Active {
    owner: WindowToken,
    nonce: String,
    id: SessionId,
    peer: Option<oneshot::Sender<bool>>,
}
impl Default for WindowSessions {
    fn default() -> Self {
        Self(Arc::new(Inner {
            registry: SessionRegistry::new(),
            windows: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            slot: Arc::new(Semaphore::new(1)),
        }))
    }
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
        windows.insert(label.clone(), token);
        let state = self.clone();
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                let _ = state.revoke(&label, token);
            }
        });
        Ok(token)
    }
    fn revoke(&self, label: &str, token: WindowToken) -> Result<()> {
        let mut windows = self.0.windows.lock().map_err(error)?;
        if windows.get(label) == Some(&token) {
            windows.remove(label);
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
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim_end() != "go" {
        return Err(std::io::Error::other("invalid probe input"));
    }
    let mut out = std::io::stdout().lock();
    out.write_all(b"\x1b[2J\x1b[H")?;
    let block = "hello 中文🌍\r\n".repeat(4096);
    for _ in 0..40 {
        out.write_all(block.as_bytes())?;
    }
    out.write_all(b"\x1b[31mEOF\x1b[0m\xe4\xb8")?;
    out.flush()
}
async fn native() -> Result<(SessionHandle, pulse_app::LocalSessionTask)> {
    let mut command = PtyCommand::new(std::env::current_exe().map_err(error)?);
    command.args.push("--session-probe-child".into());
    spawn_local(command, TermSize::new(40, 12).map_err(error)?, 128)
        .await
        .map_err(error)
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
    pub generation: String,
    pub digest: String,
    pub revision: String,
    pub parsed_bytes: String,
    pub frames: u32,
    pub peak_credit_bytes: u32,
    pub native_exit: u32,
    pub peer_denied: bool,
    pub window_revoked: bool,
    pub session_survived: bool,
}
impl SessionEvidence {
    pub(crate) fn validate(&self) -> Result<()> {
        parse_u64(&self.generation)?;
        parse_u64(&self.revision)?;
        if parse_u64(&self.parsed_bytes)? < 1024 * 1024
            || self.digest.len() != 16
            || !self
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !(4..=16_384).contains(&self.frames)
            || !(1..=8212).contains(&self.peak_credit_bytes)
            || self.native_exit != 0
            || !self.peer_denied
            || !self.window_revoked
            || !self.session_survived
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
) -> Result<()> {
    let owner = state.caller(&window)?;
    let (id, sender) = {
        let mut active = state.0.active.lock().map_err(error)?;
        let run = active
            .as_mut()
            .filter(|run| run.nonce == nonce && run.owner != owner)
            .ok_or("session.forbidden".to_string())?;
        (run.id, run.peer.take())
    };
    let denied = state
        .0
        .registry
        .input(owner, id, b"go\n".to_vec())
        .await
        .is_err()
        && state.0.registry.acknowledge(owner, id, 1, 0).is_err();
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
    *state.0.active.lock().map_err(error)? = Some(Active {
        owner,
        nonce: nonce.clone(),
        id,
        peer: Some(peer_tx),
    });
    let (peer_handle, peer_task) = native().await?;
    // Each window incarnation is installed on the runtime, never from a frontend label.
    let peer_state = state.clone();
    let script = format!(
        "void window.__TAURI_INTERNALS__.invoke('probe_session_peer',{{nonce:'{nonce}'}}).catch(console.error)"
    );
    let peer = WebviewWindowBuilder::new(
        window.app_handle(),
        "session-peer",
        WebviewUrl::App("index.html?session-peer=1".into()),
    )
    .visible(false)
    .on_page_load(move |peer, payload| {
        if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
            && peer_state.install(&peer).is_ok()
            && peer.eval(&script).is_err()
        {
            eprintln!("session.peer_eval_failed");
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
    let peer_denied = tokio::time::timeout(Duration::from_secs(10), peer_rx)
        .await
        .map_err(|_| "session.peer_timeout".to_string())?
        .map_err(|_| "session.peer_cancelled".to_string())?;
    peer.close().map_err(error)?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if state.0.registry.state(peer_owner, peer_id).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "session.window_revoke_timeout".to_string())?;
    let session_survived = peer_handle.state.borrow().lifecycle == Lifecycle::Running;
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
        bytes: 8212,
        ack_timeout: Duration::from_secs(10),
    };
    let mut attachment = state
        .0
        .registry
        .attach(owner, id, config)
        .await
        .map_err(|_| "session.capture_failed".to_string())?;
    let generation = attachment.generation;
    let relay = async {
        while let Some(wire) = attachment.wire.recv().await {
            channel.send(RawFrame(wire)).map_err(error)?;
        }
        attachment
            .completion
            .await
            .map_err(|_| "session.relay_cancelled".to_string())?
            .map_err(|_| "session.relay_failed".to_string())
    };
    let receipt = tokio::time::timeout(Duration::from_secs(60), relay)
        .await
        .map_err(error)??;
    let (final_state, screen, native) =
        tokio::time::timeout(Duration::from_secs(5), task.join_with_screen())
            .await
            .map_err(error)?
            .map_err(error)?;
    if native.exit_code != 0 {
        return Err("session.native_exit_failed".into());
    }
    if native.cancelled {
        return Err("session.native_cancelled".into());
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
        generation: generation.to_string(),
        digest: pulse_term::reconciliation::screen_digest(&screen),
        revision: receipt.boundary.revision.to_string(),
        parsed_bytes: receipt.boundary.parsed_bytes.to_string(),
        frames: u32::try_from(receipt.frames).map_err(error)?,
        peak_credit_bytes: u32::try_from(receipt.peak_credit_bytes).map_err(error)?,
        native_exit: native.exit_code,
        peer_denied,
        window_revoked: true,
        session_survived,
    })
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
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
            generation: "2".into(),
            digest: "9e1323ca657a4985".into(),
            revision: "777".into(),
            parsed_bytes: "3112985".into(),
            frames: 780,
            peak_credit_bytes: 6092,
            native_exit: 0,
            peer_denied: true,
            window_revoked: true,
            session_survived: true,
        };
        assert!(e.validate().is_ok());
        e.generation = "02".into();
        assert!(e.validate().is_err());
        e.generation = "2".into();
        e.digest = "G".repeat(16);
        assert!(e.validate().is_err());
        e.digest = "9e1323ca657a4985".into();
        e.peak_credit_bytes = 8213;
        assert!(e.validate().is_err());
        e.peak_credit_bytes = 6092;
        e.session_survived = false;
        assert!(e.validate().is_err());
    }
    #[test]
    fn exact_control_integers() {
        assert_eq!(parse_u64("18446744073709551615").unwrap(), u64::MAX);
        for n in ["0", "01", "+1", " 1", "18446744073709551616"] {
            assert!(parse_u64(n).is_err());
        }
    }
}
