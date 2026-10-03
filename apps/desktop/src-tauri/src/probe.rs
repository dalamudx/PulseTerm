use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use pulse_ipc::flow::CreditWindow;
use pulse_ipc::{Frame, FrameKind, MAX_PAYLOAD};
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{Manager, State};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

const WAIT: Duration = Duration::from_secs(10);
type ProbeResult<T> = Result<T, String>;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeConfig {
    chunk_bytes: usize,
    frames: usize,
    window: usize,
}

impl ProbeConfig {
    fn validate(self) -> ProbeResult<Self> {
        if !(256..=MAX_PAYLOAD).contains(&self.chunk_bytes)
            || !(1..=16_384).contains(&self.frames)
            || !(1..=64).contains(&self.window)
            || self.chunk_bytes * self.frames > 256 * MAX_PAYLOAD
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
        let window = CreditWindow::new(config.window, config.window * config.chunk_bytes)
            .map_err(|error| error.to_string())?;
        Ok(Arc::new(Self {
            id,
            config,
            credits: Arc::new(Semaphore::new(config.window)),
            flow: Mutex::new(FlowState {
                window,
                sent: VecDeque::with_capacity(config.window),
                rtts_ms: Vec::with_capacity(config.frames),
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
            .record_send(self.config.chunk_bytes)
            .map_err(|e| e.to_string())?;
        let mut payload = vec![0; self.config.chunk_bytes];
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
            self.credits
                .clone()
                .acquire_many_owned(self.config.window as u32),
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
            peak_frames: flow.peak_frames,
            peak_bytes: flow.peak_bytes,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackendStats {
    frames: usize,
    bytes: usize,
    elapsed_ms: f64,
    ack_rtt_p95_ms: f64,
    peak_frames: usize,
    peak_bytes: usize,
}

struct BenchState {
    epoch: Instant,
    active: Mutex<Option<Arc<Run>>>,
    automatic: bool,
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
fn probe_mode(state: State<'_, BenchState>) -> bool {
    state.automatic
}

#[tauri::command]
fn probe_clock(state: State<'_, BenchState>) -> f64 {
    state.epoch.elapsed().as_secs_f64() * 1000.0
}

#[tauri::command]
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
async fn probe_channel(
    id: String,
    config: ProbeConfig,
    on_data: Channel<InvokeResponseBody>,
    state: State<'_, BenchState>,
) -> ProbeResult<BackendStats> {
    let run = state.begin(id, config)?;
    let start = Instant::now();
    let result = async {
        for _ in 0..config.frames {
            on_data
                .send(InvokeResponseBody::Raw(run.frame(state.epoch).await?))
                .map_err(|e| e.to_string())?;
        }
        run.completed(start).await
    }
    .await;
    state.end(&run.id);
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SocketTicket {
    url: String,
    token: String,
}

#[tauri::command]
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
fn probe_finish(
    report: serde_json::Value,
    app: tauri::AppHandle,
    state: State<'_, BenchState>,
) -> ProbeResult<()> {
    if state.active.lock().map_err(|_| "probe.poisoned")?.is_some() {
        return Err("probe.busy".into());
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
        let failed = report.get("completed").and_then(serde_json::Value::as_bool) != Some(true);
        app.exit(i32::from(failed));
    }
    Ok(())
}

/// Launch the local-only validation app. No product sessions are exposed yet.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let automatic = args.iter().any(|arg| arg == "--probe");
    let report_path = args
        .iter()
        .position(|arg| arg == "--report")
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from);
    let state = BenchState {
        epoch: Instant::now(),
        active: Mutex::new(None),
        automatic,
        report_path,
    };
    let result = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            probe_mode,
            probe_clock,
            probe_channel,
            probe_ack,
            probe_cancel,
            probe_socket,
            probe_finish
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Desktop startup failed: {error}");
        std::process::exit(1);
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
                chunk_bytes: MAX_PAYLOAD,
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
