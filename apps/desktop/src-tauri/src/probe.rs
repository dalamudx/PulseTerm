use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use pulse_ipc::flow::CreditWindow;
use pulse_ipc::{Frame, FrameKind, MAX_PAYLOAD};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri_specta::Event;

use crate::raw_frame::RawFrame;
use crate::window_sessions::WindowSessions;
use tauri::{Manager, State};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

const WAIT: Duration = Duration::from_secs(10);
type ProbeResult<T> = Result<T, String>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeConfig {
    chunk_bytes: u32,
    frames: u32,
    window: u32,
}

impl ProbeConfig {
    fn validate(self) -> ProbeResult<Self> {
        if !(256..=MAX_PAYLOAD as u32).contains(&self.chunk_bytes)
            || !(1..=16_384).contains(&self.frames)
            || !(1..=64).contains(&self.window)
            || u64::from(self.chunk_bytes) * u64::from(self.frames) > 256 * MAX_PAYLOAD as u64
        {
            return Err("probe.invalid_config".into());
        }
        Ok(self)
    }
}

struct FlowState {
    window: CreditWindow,
    sent: VecDeque<Instant>,
    rtts_ms: Vec<f64>,
    peak_frames: usize,
    peak_bytes: usize,
}

struct Run {
    id: String,
    config: ProbeConfig,
    credits: Arc<Semaphore>,
    flow: Mutex<FlowState>,
}

impl Run {
    fn new(id: String, config: ProbeConfig) -> ProbeResult<Arc<Self>> {
        uuid::Uuid::parse_str(&id).map_err(|_| "probe.invalid_id")?;
        let config = config.validate()?;
        let window = CreditWindow::new(
            config.window as usize,
            (config.window * config.chunk_bytes) as usize,
        )
        .map_err(|error| error.to_string())?;
        Ok(Arc::new(Self {
            id,
            config,
            credits: Arc::new(Semaphore::new(config.window as usize)),
            flow: Mutex::new(FlowState {
                window,
                sent: VecDeque::with_capacity(config.window as usize),
                rtts_ms: Vec::with_capacity(config.frames as usize),
                peak_frames: 0,
                peak_bytes: 0,
            }),
        }))
    }

    fn acknowledge(&self, sequence: u32) -> ProbeResult<()> {
        let mut flow = self.flow.lock().map_err(|_| "probe.poisoned")?;
        let released = flow
            .window
            .acknowledge(sequence)
            .map_err(|_| "probe.invalid_ack")?;
        let now = Instant::now();
        for _ in 0..released.frames {
            let sent = flow.sent.pop_front().ok_or("probe.invalid_timing_queue")?;
            flow.rtts_ms
                .push(now.duration_since(sent).as_secs_f64() * 1000.0);
        }
        self.credits.add_permits(released.frames);
        Ok(())
    }

    async fn frame(&self, epoch: Instant) -> ProbeResult<Vec<u8>> {
        let permit = timeout(WAIT, self.credits.clone().acquire_owned())
            .await
            .map_err(|_| "probe.ack_timeout")?
            .map_err(|_| "probe.cancelled")?;
        let mut flow = self.flow.lock().map_err(|_| "probe.poisoned")?;
        let sequence = flow
            .window
            .record_send(self.config.chunk_bytes as usize)
            .map_err(|e| e.to_string())?;
        let mut payload = vec![0; self.config.chunk_bytes as usize];
        for (index, byte) in payload.iter_mut().enumerate().skip(8) {
            *byte = sequence.wrapping_add(index as u32) as u8;
        }
        let now = Instant::now();
        payload[..8]
            .copy_from_slice(&(now.duration_since(epoch).as_secs_f64() * 1000.0).to_le_bytes());
        flow.sent.push_back(now);
        flow.peak_frames = flow.peak_frames.max(flow.window.outstanding_frames());
        flow.peak_bytes = flow.peak_bytes.max(flow.window.outstanding_bytes());
        let bytes = Frame {
            kind: FrameKind::Data,
            sequence,
            payload: &payload,
        }
        .encode()
        .map_err(|error| error.to_string())?;
        permit.forget();
        Ok(bytes)
    }

    async fn completed(&self, start: Instant) -> ProbeResult<BackendStats> {
        let _permit = timeout(
            WAIT,
            self.credits.clone().acquire_many_owned(self.config.window),
        )
        .await
        .map_err(|_| "probe.final_ack_timeout")?
        .map_err(|_| "probe.cancelled")?;
        let mut flow = self.flow.lock().map_err(|_| "probe.poisoned")?;
        flow.rtts_ms.sort_by(f64::total_cmp);
        let index = (flow.rtts_ms.len() * 95).div_ceil(100).saturating_sub(1);
        Ok(BackendStats {
            frames: self.config.frames,
            bytes: self.config.frames * self.config.chunk_bytes,
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
            ack_rtt_p95_ms: flow.rtts_ms.get(index).copied().unwrap_or(0.0),
            peak_frames: flow.peak_frames as u32,
            peak_bytes: flow.peak_bytes as u32,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct BackendStats {
    frames: u32,
    bytes: u32,
    elapsed_ms: f64,
    ack_rtt_p95_ms: f64,
    peak_frames: u32,
    peak_bytes: u32,
}

struct BenchState {
    epoch: Instant,
    active: Mutex<Option<Arc<Run>>>,
    automatic: bool,
    minimize: bool,
    external_wm: bool,
    report_path: Option<PathBuf>,
}

impl BenchState {
    fn begin(&self, id: String, config: ProbeConfig) -> ProbeResult<Arc<Run>> {
        let mut active = self.active.lock().map_err(|_| "probe.poisoned")?;
        if active.is_some() {
            return Err("probe.busy".into());
        }
        let run = Run::new(id, config)?;
        *active = Some(run.clone());
        Ok(run)
    }

    fn end(&self, id: &str) {
        if let Ok(mut active) = self.active.lock()
            && active.as_ref().is_some_and(|run| run.id == id)
        {
            *active = None;
        }
    }
}

#[tauri::command]
#[specta::specta]
fn probe_mode(state: State<'_, BenchState>) -> bool {
    state.automatic
}

#[tauri::command]
#[specta::specta]
fn probe_clock(state: State<'_, BenchState>) -> f64 {
    state.epoch.elapsed().as_secs_f64() * 1000.0
}

#[tauri::command]
#[specta::specta]
fn probe_ack(id: String, sequence: u32, state: State<'_, BenchState>) -> ProbeResult<()> {
    let run = state
        .active
        .lock()
        .map_err(|_| "probe.poisoned")?
        .as_ref()
        .filter(|run| run.id == id)
        .cloned()
        .ok_or("probe.stale_run")?;
    run.acknowledge(sequence)
}

#[tauri::command]
#[specta::specta]
fn probe_cancel(id: String, state: State<'_, BenchState>) -> ProbeResult<()> {
    let active = state.active.lock().map_err(|_| "probe.poisoned")?;
    let run = active
        .as_ref()
        .filter(|run| run.id == id)
        .ok_or("probe.stale_run")?;
    run.credits.close();
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn probe_channel(
    id: String,
    config: ProbeConfig,
    on_data: Channel<RawFrame>,
    state: State<'_, BenchState>,
) -> ProbeResult<BackendStats> {
    let run = state.begin(id, config)?;
    let start = Instant::now();
    let result = async {
        for _ in 0..config.frames {
            on_data
                .send(RawFrame(run.frame(state.epoch).await?))
                .map_err(|e| e.to_string())?;
        }
        run.completed(start).await
    }
    .await;
    state.end(&run.id);
    result
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct SocketTicket {
    url: String,
    token: String,
}

#[tauri::command]
#[specta::specta]
async fn probe_socket(
    id: String,
    config: ProbeConfig,
    app: tauri::AppHandle,
    state: State<'_, BenchState>,
) -> ProbeResult<SocketTicket> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|e| e.to_string())?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    let token = format!("p_{}", uuid::Uuid::new_v4().simple());
    let run = state.begin(id, config)?;
    let server_token = token.clone();
    let epoch = state.epoch;
    tauri::async_runtime::spawn(async move {
        let result = socket_stream(listener, server_token, run.clone(), epoch).await;
        run.credits.close();
        app.state::<BenchState>().end(&run.id);
        if let Err(error) = result {
            eprintln!("WebSocket probe: {error}");
        }
    });
    Ok(SocketTicket {
        url: format!("ws://{address}"),
        token,
    })
}

// tungstenite requires this concrete HTTP error-response type for its callback.
#[allow(clippy::result_large_err)]
async fn socket_stream(
    listener: TcpListener,
    token: String,
    run: Arc<Run>,
    epoch: Instant,
) -> ProbeResult<()> {
    let (tcp, _) = timeout(WAIT, listener.accept())
        .await
        .map_err(|_| "probe.connect_timeout")?
        .map_err(|e| e.to_string())?;
    let socket = timeout(
        WAIT,
        tokio_tungstenite::accept_hdr_async_with_config(
            tcp,
            move |request: &Request, mut response: Response| {
                let protocols = request
                    .headers()
                    .get("sec-websocket-protocol")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let offered: Vec<_> = protocols.split(',').map(str::trim).collect();
                if !offered.contains(&token.as_str()) || !offered.contains(&"pulseterm-probe") {
                    return Err(tokio_tungstenite::tungstenite::http::Response::builder()
                        .status(403)
                        .body(Some("invalid probe token".into()))
                        .unwrap_or_default());
                }
                response.headers_mut().insert(
                    "sec-websocket-protocol",
                    tokio_tungstenite::tungstenite::http::HeaderValue::from_static(
                        "pulseterm-probe",
                    ),
                );
                Ok(response)
            },
            Some(
                tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                    .read_buffer_size(4096)
                    .write_buffer_size(0)
                    .max_write_buffer_size(2 * MAX_PAYLOAD)
                    .max_message_size(Some(4))
                    .max_frame_size(Some(125)),
            ),
        ),
    )
    .await
    .map_err(|_| "probe.handshake_timeout")?
    .map_err(|e| e.to_string())?;
    let (mut sender, mut receiver) = socket.split();
    let reader_run = run.clone();
    let ack_task = tauri::async_runtime::spawn(async move {
        while let Some(message) = receiver.next().await {
            match message {
                Ok(Message::Binary(bytes)) if bytes.len() == 4 => {
                    let sequence = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    if reader_run.acknowledge(sequence).is_err() {
                        break;
                    }
                }
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
                _ => break,
            }
        }
        reader_run.credits.close();
    });
    let start = Instant::now();
    let result = async {
        for _ in 0..run.config.frames {
            timeout(
                WAIT,
                sender.send(Message::Binary(run.frame(epoch).await?.into())),
            )
            .await
            .map_err(|_| "probe.send_timeout")?
            .map_err(|e| e.to_string())?;
        }
        let stats = run.completed(start).await?;
        let json = serde_json::to_string(&stats).map_err(|e| e.to_string())?;
        sender
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;
    ack_task.abort();
    result
}

#[tauri::command]
#[specta::specta]
fn probe_finish(
    report: ProbeReport,
    app: tauri::AppHandle,
    state: State<'_, BenchState>,
) -> ProbeResult<()> {
    if state.active.lock().map_err(|_| "probe.poisoned")?.is_some() {
        return Err("probe.busy".into());
    }
    report.validate().inspect_err(|error| {
        eprintln!(
            "probe.report_rejected: {error}; session_runs={} dom_runs={}",
            report.session_runs, report.dom_runs
        )
    })?;
    report.validate_window_mode(state.minimize)?;
    if report.completed
        && !window_mode_matches(
            state.external_wm,
            report.dom_session.as_ref().map(|s| s.external_wm_checked),
            report.session.as_ref().map(|s| s.external_wm_checked),
        )
    {
        return Err("probe.external_mode_mismatch".into());
    }
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("probe.report_too_large".into());
    }
    if let Some(path) = &state.report_path {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    if state.automatic {
        let failed = !report.completed;
        app.exit(i32::from(failed));
    }
    Ok(())
}

// Explicit JSON reports avoid recursively reflecting serde_json::Value and reject
// malformed shapes at the IPC boundary. Bounds are still checked by the probe runner.
#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeReport {
    schema_version: u32,
    completed: bool,
    cancellation: bool,
    typed_events: bool,
    session: Option<crate::window_sessions::SessionEvidence>,
    session_runs: u32,
    dom_session: Option<crate::window_sessions::SessionEvidence>,
    dom_evidence: Option<DomEvidence>,
    dom_runs: u32,
    measurements: Vec<ProbeMeasurement>,
    error: Option<String>,
    environment: ProbeEnvironment,
    limitations: Vec<String>,
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DomEvidence {
    render_events: u32,
    frame_fences: u32,
    rows_checked: u32,
    width_px: f64,
    height_px: f64,
    readonly: bool,
    keyboard_checked: bool,
    composition_checked: bool,
    paste_blocked: bool,
    fit_checked: bool,
    visibility_checked: bool,
    native_signal_checked: bool,
    native_pause_checked: bool,
    view_host_checked: bool,
    view_pool_checked: bool,
}
impl DomEvidence {
    fn validate(&self) -> ProbeResult<()> {
        if self.render_events == 0
            || self.frame_fences == 0
            || self.rows_checked != 12
            || !self.readonly
            || !self.keyboard_checked
            || !self.composition_checked
            || !self.paste_blocked
            || !self.fit_checked
            || !self.visibility_checked
            || !self.native_signal_checked
            || !self.native_pause_checked
            || !self.view_host_checked
            || !self.view_pool_checked
            || !self.width_px.is_finite()
            || !self.height_px.is_finite()
            || !(1.0..=4096.0).contains(&self.width_px)
            || !(1.0..=4096.0).contains(&self.height_px)
        {
            return Err("probe.invalid_dom_evidence".into());
        }
        Ok(())
    }
}

fn window_mode_matches(minimize: bool, dom: Option<bool>, headless: Option<bool>) -> bool {
    dom == Some(minimize) && headless == Some(false)
}
impl ProbeReport {
    fn validate_window_mode(&self, minimize: bool) -> ProbeResult<()> {
        if self.completed
            && !window_mode_matches(
                minimize,
                self.dom_session.as_ref().map(|s| s.native_minimize_checked),
                self.session.as_ref().map(|s| s.native_minimize_checked),
            )
        {
            return Err("probe.window_mode_mismatch".into());
        }
        Ok(())
    }

    fn validate(&self) -> ProbeResult<()> {
        if self.schema_version != 4
            || self.measurements.len() > 7
            || (self.completed
                && (self.session_runs != 2
                    || self.dom_runs != 2
                    || self
                        .dom_evidence
                        .as_ref()
                        .is_none_or(|e| e.validate().is_err())
                    || self.dom_session.as_ref().is_none_or(|s| {
                        s.validate().is_err()
                            || s.resize_count != 3
                            || s.pause_count != 1
                            || !s.native_window_checked
                            || !s.native_only_checked
                            || !s.repair_checked
                            || !s.idle_checked
                            || !s.marker_checked
                    })
                    || !self.cancellation
                    || !self.typed_events
                    || self.session.as_ref().is_none_or(|s| {
                        !s.peer_denied
                            || !s.window_revoked
                            || !s.session_survived
                            || s.native_exit != 0
                            || s.peak_credit_bytes > 8212
                            || s.frames < 4
                    })
                    || self.error.is_some()
                    || self.measurements.len() != 7
                    || self.measurements.iter().any(|row| !row.verified)))
        {
            return Err("probe.invalid_report".into());
        }
        if let Some(session) = &self.dom_session {
            session.validate()?;
        }
        if let Some(evidence) = &self.dom_evidence {
            evidence.validate()?;
        }
        if let Some(session) = &self.session {
            session.validate()?;
        }
        for row in &self.measurements {
            row.config.validate()?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeMeasurement {
    label: String,
    transport: ProbeTransport,
    config: ProbeConfig,
    bytes_per_second: f64,
    delivery_p95_ms: f64,
    clock_uncertainty_ms: f64,
    heartbeat_p95_ms: f64,
    heartbeat_max_ms: f64,
    backend: BackendStats,
    verified: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
enum ProbeTransport {
    Channel,
    Websocket,
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeEnvironment {
    user_agent: String,
    hardware_concurrency: u32,
}

#[derive(Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
struct ProbeEcho {
    nonce: String,
    protocol_version: u8,
}

#[tauri::command]
#[specta::specta]
fn probe_event(nonce: String, app: tauri::AppHandle) -> ProbeResult<()> {
    uuid::Uuid::parse_str(&nonce).map_err(|_| "probe.invalid_nonce")?;
    ProbeEcho {
        nonce,
        protocol_version: 1,
    }
    .emit(&app)
    .map_err(|error| error.to_string())
}

fn command_bindings() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new()
        .commands(tauri_specta::collect_commands![
            probe_mode,
            probe_clock,
            probe_channel,
            probe_ack,
            probe_cancel,
            probe_socket,
            probe_finish,
            probe_event,
            crate::window_sessions::probe_session,
            crate::window_sessions::probe_session_ack,
            crate::window_sessions::probe_session_compare,
            crate::window_sessions::probe_session_repair,
            crate::window_sessions::probe_session_marker,
            crate::window_sessions::probe_session_resume,
            crate::window_sessions::probe_session_input,
            crate::window_sessions::probe_session_resize,
            crate::window_sessions::probe_session_visibility,
            crate::window_sessions::probe_session_native_snapshot,
            crate::window_sessions::probe_session_hidden_output,
            crate::window_sessions::probe_session_window_cycle,
            crate::window_sessions::probe_session_native_cycle,
            crate::window_sessions::probe_session_detach,
            crate::window_sessions::probe_session_go,
            crate::window_sessions::probe_session_peer
        ])
        .events(tauri_specta::collect_events![
            ProbeEcho,
            crate::native_visibility::NativeVisibilitySample
        ])
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
}

fn export_bindings(check: bool) -> ProbeResult<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let path = root.join("ui/src/ipc/gen/commands.ts");
    let scratch = root.join(format!("target/bindings-{}.ts", std::process::id()));
    command_bindings()
        .export(specta_typescript::Typescript::default(), &scratch)
        .map_err(|error| error.to_string())?;
    let generated = std::fs::read_to_string(&scratch).map_err(|error| error.to_string())?;
    // Normalize only trailing whitespace, keeping generation and checks identical.
    let generated = format!("{}\n", generated.trim_end());
    std::fs::remove_file(&scratch).map_err(|error| error.to_string())?;
    if check {
        let committed = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        if committed.replace("\r\n", "\n") != generated.replace("\r\n", "\n") {
            return Err("IPC bindings are stale; run mise run bindings".into());
        }
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, generated).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Launch the local-only validation app. No product sessions are exposed yet.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--session-probe-child") {
        if crate::window_sessions::child().is_err() {
            std::process::exit(2);
        }
        return;
    }
    if args
        .iter()
        .any(|arg| arg == "--export-bindings" || arg == "--check-bindings")
    {
        if let Err(error) = export_bindings(args.iter().any(|arg| arg == "--check-bindings")) {
            eprintln!("IPC binding export failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    let external_wm = args.iter().any(|arg| arg == "--probe-window-external");
    let minimize = external_wm || args.iter().any(|arg| arg == "--probe-window");
    let external_control = if external_wm {
        match std::env::var_os("PULSE_PROBE_WINDOW_CONTROL").map(PathBuf::from) {
            Some(path) if path.is_absolute() && path.is_dir() => Some(path),
            _ => {
                eprintln!("probe.external_controller_missing");
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let automatic = minimize || args.iter().any(|arg| arg == "--probe");
    let report_path = args
        .iter()
        .position(|arg| arg == "--report")
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from);
    let state = BenchState {
        epoch: Instant::now(),
        active: Mutex::new(None),
        automatic,
        minimize,
        external_wm,
        report_path,
    };
    let bindings = command_bindings();
    let result = tauri::Builder::default()
        .manage(state)
        .manage(WindowSessions::new(minimize, external_control))
        .invoke_handler(bindings.invoke_handler())
        .setup(move |app| {
            bindings.mount_events(app);
            let window = app
                .get_webview_window("main")
                .ok_or("main window missing")?;
            app.state::<WindowSessions>().install(&window)?;
            Ok(())
        })
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(|handle, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                handle.state::<WindowSessions>().shutdown();
            }
        }),
        Err(error) => {
            eprintln!("Desktop startup failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn config() -> ProbeConfig {
        ProbeConfig {
            chunk_bytes: 256,
            frames: 4,
            window: 2,
        }
    }

    #[tokio::test]
    async fn semaphore_and_window_stay_in_sync() {
        let run = Run::new(uuid::Uuid::new_v4().to_string(), config()).unwrap();
        let epoch = Instant::now();
        let first = run.frame(epoch).await.unwrap();
        let second = run.frame(epoch).await.unwrap();
        assert_eq!(pulse_ipc::decode(&first).unwrap().sequence, 0);
        assert_eq!(pulse_ipc::decode(&second).unwrap().sequence, 1);
        assert_eq!(run.acknowledge(8), Err("probe.invalid_ack".into()));
        assert_eq!(run.credits.available_permits(), 0);
        assert!(
            timeout(Duration::from_millis(20), run.frame(epoch))
                .await
                .is_err()
        );
        run.acknowledge(1).unwrap();
        assert_eq!(run.credits.available_permits(), 2);
        assert_eq!(run.acknowledge(1), Err("probe.invalid_ack".into()));
        run.frame(epoch).await.unwrap();
        run.credits.close();
        assert_eq!(run.frame(epoch).await, Err("probe.cancelled".into()));
    }

    #[test]
    fn active_slot_is_exclusive_and_stale_ids_cannot_clear_it() {
        let state = BenchState {
            epoch: Instant::now(),
            active: Mutex::new(None),
            automatic: false,
            minimize: false,
            external_wm: false,
            report_path: None,
        };
        let id = uuid::Uuid::new_v4().to_string();
        state.begin(id.clone(), config()).unwrap();
        assert!(
            state
                .begin(uuid::Uuid::new_v4().to_string(), config())
                .is_err()
        );
        state.end("stale");
        assert!(state.active.lock().unwrap().is_some());
        state.end(&id);
        assert!(
            state
                .begin(uuid::Uuid::new_v4().to_string(), config())
                .is_ok()
        );
    }

    #[tokio::test]
    async fn unauthenticated_socket_cannot_receive_data() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let run = Run::new(uuid::Uuid::new_v4().to_string(), config()).unwrap();
        let server = tokio::spawn(socket_stream(
            listener,
            "secret".into(),
            run.clone(),
            Instant::now(),
        ));
        let connection = tokio_tungstenite::connect_async(format!("ws://{address}")).await;
        assert!(connection.is_err());
        assert!(server.await.unwrap().is_err());
        assert_eq!(run.flow.lock().unwrap().window.outstanding_frames(), 0);
    }

    #[test]
    fn control_numbers_are_bounded_without_overflow() {
        for config in [
            ProbeConfig {
                chunk_bytes: u32::MAX,
                frames: u32::MAX,
                window: u32::MAX,
            },
            ProbeConfig {
                chunk_bytes: MAX_PAYLOAD as u32,
                frames: 16_384,
                window: 64,
            },
        ] {
            assert!(config.validate().is_err());
        }
        assert!(
            ProbeConfig {
                chunk_bytes: MAX_PAYLOAD as u32,
                frames: 256,
                window: 64
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn config_serde_matches_generated_wire_spelling() {
        let value = serde_json::to_value(config()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "chunkBytes": 256, "frames": 4, "window": 2 })
        );
        assert!(serde_json::from_value::<ProbeConfig>(value).is_ok());
        assert!(
            serde_json::from_value::<ProbeConfig>(
                serde_json::json!({ "chunk_bytes": 256, "frames": 4, "window": 2 })
            )
            .is_err()
        );
    }

    #[test]
    fn report_window_mode_cannot_substitute_hide_for_minimize_or_claim_headless() {
        for mode in [false, true] {
            assert!(window_mode_matches(mode, Some(mode), Some(false)));
            assert!(!window_mode_matches(mode, Some(!mode), Some(false)));
            assert!(!window_mode_matches(mode, None, Some(false)));
            assert!(!window_mode_matches(mode, Some(mode), Some(true)));
            assert!(!window_mode_matches(mode, Some(mode), None));
        }
    }
    #[test]
    fn report_validation_requires_version_and_real_evidence() {
        let mut report = ProbeReport {
            schema_version: 4,
            completed: false,
            cancellation: false,
            typed_events: false,
            session: None,
            session_runs: 0,
            dom_session: None,
            dom_evidence: None,
            dom_runs: 0,
            measurements: vec![],
            error: Some("probe.failure".into()),
            environment: ProbeEnvironment {
                user_agent: "test".into(),
                hardware_concurrency: 1,
            },
            limitations: vec![],
        };
        assert!(report.validate().is_ok());
        report.schema_version = 1;
        assert!(report.validate().is_err());
        report.schema_version = 4;
        report.completed = true;
        assert!(report.validate().is_err());
        report.cancellation = true;
        report.typed_events = true;
        report.error = None;
        assert!(report.validate().is_err()); // Still missing measurements.
    }

    #[test]
    fn dom_evidence_requires_visible_finite_dimensions_and_actual_rows() {
        let mut e = DomEvidence {
            render_events: 1,
            frame_fences: 2,
            rows_checked: 12,
            width_px: 270.0,
            height_px: 128.0,
            readonly: true,
            keyboard_checked: true,
            composition_checked: true,
            paste_blocked: true,
            fit_checked: true,
            visibility_checked: true,
            native_signal_checked: true,
            native_pause_checked: true,
            view_host_checked: true,
            view_pool_checked: true,
        };
        assert!(e.validate().is_ok());
        e.render_events = 0;
        assert!(e.validate().is_err());
        e.render_events = 1;
        e.frame_fences = 0;
        assert!(e.validate().is_err());
        e.frame_fences = 2;
        e.rows_checked = 0;
        assert!(e.validate().is_err());
        e.rows_checked = 12;
        for width in [0.0, f64::NAN, f64::INFINITY, 4097.0] {
            e.width_px = width;
            assert!(e.validate().is_err());
        }
        e.width_px = 270.0;
        e.paste_blocked = false;
        assert!(e.validate().is_err());
        e.paste_blocked = true;
        e.keyboard_checked = false;
        assert!(e.validate().is_err());
        e.keyboard_checked = true;
        e.composition_checked = false;
        assert!(e.validate().is_err());
        e.composition_checked = true;
        e.view_pool_checked = false;
        assert!(e.validate().is_err());
        e.view_pool_checked = true;
        e.readonly = false;
        assert!(e.validate().is_err());
    }

    #[test]
    fn config_prevents_unbounded_work() {
        for config in [
            ProbeConfig {
                chunk_bytes: 0,
                frames: 1,
                window: 1,
            },
            ProbeConfig {
                chunk_bytes: 256,
                frames: 0,
                window: 1,
            },
            ProbeConfig {
                chunk_bytes: MAX_PAYLOAD as u32,
                frames: 1024,
                window: 1,
            },
            ProbeConfig {
                chunk_bytes: 256,
                frames: 1,
                window: 65,
            },
        ] {
            assert!(config.validate().is_err());
        }
    }
}
