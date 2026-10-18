//! Existing root sessions only. Page-bound v2 delivery and confirmed UTF-8 input.
use super::*;
use crate::raw_frame::RawFrame;
use pulse_app::{
    attachment::AttachmentConfig,
    terminal_view::{TerminalViewBinding, TerminalViewDelivery},
};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot, watch};
const MAX_VIEWS: usize = 64;
const CONNECT_DEADLINE: Duration = Duration::from_secs(3);
struct Entry {
    nonce: Uuid,
    capture: pulse_term::session::SessionState,
    deadline: tokio::time::Instant,
    connected: std::sync::atomic::AtomicBool,
    confirmed: std::sync::atomic::AtomicBool,
    input_enabled: std::sync::atomic::AtomicBool,
    input_displayed: std::sync::atomic::AtomicBool,
    resize_started: std::sync::atomic::AtomicBool,
    completed: std::sync::atomic::AtomicBool,
    admission: Mutex<()>,
    input_credit: Arc<Semaphore>,
    binding: Arc<TerminalViewBinding>,
    connect: Mutex<Option<oneshot::Sender<tauri::ipc::Channel<RawFrame>>>>,
    cancel: watch::Sender<bool>,
    // Includes awaiting connect and retiring relays, not merely map entries.
    _credit: OwnedSemaphorePermit,
}
pub(super) struct TerminalBank {
    entries: Mutex<HashMap<Uuid, Arc<Entry>>>,
    credit: Arc<Semaphore>,
    link_opens: Arc<Semaphore>,
    closed: std::sync::atomic::AtomicBool,
    clipboard: Mutex<Option<clipboard::Grant>>,
    clipboard_reads: Arc<Semaphore>,
}
#[derive(serde::Serialize)]
pub(super) struct Prepared {
    ticket: String,
    generation: String,
    palette: String,
    history: usize,
    ended: bool,
    appearance: pulse_store::profile_editor::ProfileAppearance,
}
impl TerminalBank {
    pub(super) fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            credit: Arc::new(Semaphore::new(MAX_VIEWS)),
            link_opens: Arc::new(Semaphore::new(1)),
            clipboard: Mutex::new(None),
            clipboard_reads: Arc::new(Semaphore::new(1)),
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }
    pub(super) fn retire(&self) {
        if let Ok(mut grant) = self.clipboard.lock() {
            grant.take();
        }
        if let Ok(mut entries) = self.entries.lock() {
            for (_, entry) in entries.drain() {
                entry.cancel.send_replace(true);
            }
        }
    }
    pub(super) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.retire();
        let _clipboard = self
            .clipboard_reads
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "workspace.shutdown_failed")?;
        let _links = self
            .link_opens
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "workspace.shutdown_failed")?;
        // Never close the semaphore: all credits are a receipt of actual task exit.
        let _all = self
            .credit
            .clone()
            .acquire_many_owned(MAX_VIEWS as u32)
            .await
            .map_err(|_| "workspace.shutdown_failed")?;
        Ok(())
    }
    async fn prepare(
        self: &Arc<Self>,
        registry: &SessionRegistry,
        page: Arc<pulse_app::registry::PageLaunchScope>,
        id: pulse_app::registry::SessionId,
        nonce: Uuid,
    ) -> Result<Prepared> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("workspace.denied");
        }
        let credit = self
            .credit
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        let view = TerminalViewBinding::attach_v2_retaining_ended(
            registry,
            page,
            id,
            AttachmentConfig::default(),
        )
        .await
        .map_err(|_| "workspace.denied")?;
        let capture = view
            .captured_palette()
            .map_err(|_| "workspace.denied")?
            .boundary
            .clone();
        let palette = serde_json::to_string(&serde_json::json!({"version":1,"palette":view.captured_palette().map_err(|_| "workspace.denied")?.palette})).map_err(|_| "workspace.unavailable")?;
        let ended = capture.lifecycle == pulse_term::session::Lifecycle::Ended;
        let delivery = TerminalViewDelivery::new(view).map_err(|_| "workspace.denied")?;
        let binding = delivery.binding();
        let generation = binding.generation().to_string();
        let ticket = Uuid::new_v4();
        let (connect, receiver) = oneshot::channel();
        let (cancel, cancelled) = watch::channel(false);
        let entry = Arc::new(Entry {
            nonce,
            capture,
            deadline: tokio::time::Instant::now() + CONNECT_DEADLINE,
            connected: std::sync::atomic::AtomicBool::new(false),
            confirmed: std::sync::atomic::AtomicBool::new(false),
            input_enabled: std::sync::atomic::AtomicBool::new(false),
            input_displayed: std::sync::atomic::AtomicBool::new(false),
            resize_started: std::sync::atomic::AtomicBool::new(false),
            completed: std::sync::atomic::AtomicBool::new(false),
            admission: Mutex::new(()),
            input_credit: Arc::new(Semaphore::new(1)),
            binding,
            connect: Mutex::new(Some(connect)),
            cancel,
            _credit: credit,
        });
        {
            let mut entries = self.entries.lock().map_err(|_| "workspace.unavailable")?;
            if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("workspace.denied");
            }
            entry
                .binding
                .check_current()
                .map_err(|_| "workspace.denied")?;
            entries.insert(ticket, entry.clone());
        }
        let bank = self.clone();
        // This task owns delivery and credit even when the IPC waiter disappears.
        tokio::spawn(async move {
            bank.forward(ticket, entry, delivery, receiver, cancelled)
                .await;
        });
        Ok(Prepared {
            ticket: ticket.to_string(),
            generation,
            palette,
            history: 10_000,
            ended,
            appearance: Default::default(),
        })
    }
    async fn forward(
        self: Arc<Self>,
        ticket: Uuid,
        entry: Arc<Entry>,
        delivery: TerminalViewDelivery,
        receiver: oneshot::Receiver<tauri::ipc::Channel<RawFrame>>,
        mut cancelled: watch::Receiver<bool>,
    ) {
        let deadline = entry.deadline;
        let work = async move {
            let channel = tokio::time::timeout_at(deadline, receiver)
                .await
                .map_err(|_| ())?
                .map_err(|_| ())?;
            let receipt = delivery
                .forward(|bytes| channel.send(RawFrame(bytes)).is_ok())
                .await
                .map_err(|_| ())?;
            let ended = receipt.boundary.lifecycle == pulse_term::session::Lifecycle::Ended;
            if ended {
                eprintln!("workspace.terminal_eof_acked");
            }
            Ok::<_, ()>(ended)
        };
        let completed = tokio::select! {
            biased;
            _ = async { if !*cancelled.borrow_and_update() { let _ = cancelled.changed().await; } } => false,
            result = work => result.unwrap_or(false),
        };
        if completed {
            // Keep only the bounded source-fixed control. No forwarding task or native
            // lease survives End ACK; ordinary operations reject this completed entry.
            if let Ok(_admission) = entry.admission.lock() {
                entry
                    .input_enabled
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                entry
                    .completed
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            } else if let Ok(mut entries) = self.entries.lock() {
                entries.remove(&ticket);
            }
        } else if let Ok(mut entries) = self.entries.lock() {
            entries.remove(&ticket);
        }
        drop(entry); // Retained entries remain charged until explicit view/page retirement.
    }
    fn entry(&self, nonce: Uuid, ticket: Uuid) -> Result<Arc<Entry>> {
        let entry = self.retained_entry(nonce, ticket)?;
        if entry.completed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("workspace.denied");
        }
        Ok(entry)
    }
    fn retained_entry(&self, nonce: Uuid, ticket: Uuid) -> Result<Arc<Entry>> {
        let entry = self
            .entries
            .lock()
            .map_err(|_| "workspace.unavailable")?
            .get(&ticket)
            .filter(|e| e.nonce == nonce)
            .cloned()
            .ok_or("workspace.denied")?;
        entry
            .binding
            .check_current()
            .map_err(|_| "workspace.denied")?;
        Ok(entry)
    }
    fn connect(
        &self,
        nonce: Uuid,
        ticket: Uuid,
        channel: tauri::ipc::Channel<RawFrame>,
    ) -> Result<()> {
        let entry = self.entry(nonce, ticket)?;
        if tokio::time::Instant::now() >= entry.deadline {
            return Err("workspace.denied");
        }
        entry
            .connect
            .lock()
            .map_err(|_| "workspace.unavailable")?
            .take()
            .ok_or("workspace.denied")?
            .send(channel)
            .map_err(|_| "workspace.denied")?;
        entry
            .connected
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    async fn confirm(&self, nonce: Uuid, ticket: Uuid, rendered: &CaptureDrawing) -> Result<()> {
        let entry = self.entry(nonce, ticket)?;
        if entry.confirmed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("workspace.denied");
        }
        let boundary = &entry.capture;
        if rendered.revision != boundary.revision.to_string()
            || rendered.source_bytes != boundary.parsed_bytes.to_string()
            || rendered.utf8_bytes != boundary.utf8_bytes.to_string()
            || rendered.columns != boundary.size.columns()
            || rendered.rows != boundary.size.rows()
        {
            return Err("workspace.denied");
        }
        eprintln!("workspace.terminal_confirm_pending");
        let result = entry
            .binding
            .confirm_capture_observed(
                pulse_app::registry::RenderedScreen {
                    boundary: boundary.clone(),
                    digest: rendered.digest.clone(),
                },
                || eprintln!("workspace.terminal_confirm_waiting"),
            )
            .await
            .map_err(|error| {
                if matches!(
                    error,
                    pulse_app::registry::RegistryError::Stale
                        | pulse_app::registry::RegistryError::Forbidden
                ) {
                    eprintln!("workspace.terminal_confirm_retired");
                }
                "workspace.denied"
            })?;
        if result != pulse_app::registry::VisibleComparison::Equal {
            return Err("workspace.capture_mismatch");
        }
        entry
            .confirmed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    async fn enable_input(&self, nonce: Uuid, ticket: Uuid) -> Result<()> {
        let entry = self.entry(nonce, ticket)?;
        if entry.capture.lifecycle != pulse_term::session::Lifecycle::Running
            || !entry.connected.load(std::sync::atomic::Ordering::SeqCst)
            || entry
                .resize_started
                .load(std::sync::atomic::Ordering::SeqCst)
            || !entry.confirmed.load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err("workspace.denied");
        }
        tokio::time::timeout(
            Duration::from_millis(250),
            entry.binding.wait_bootstrap_ready(),
        )
        .await
        .map_err(|_| "workspace.denied")?
        .map_err(|_| "workspace.denied")?;
        self.entry(nonce, ticket)?;
        let _admission = entry
            .admission
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if entry.completed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("workspace.denied");
        }
        if entry
            .resize_started
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err("workspace.denied");
        }
        entry
            .input_enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    async fn input(&self, nonce: Uuid, ticket: Uuid, bytes: Vec<u8>) -> Result<()> {
        let entry = self.entry(nonce, ticket)?;
        if !entry
            .input_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
            || entry
                .resize_started
                .load(std::sync::atomic::Ordering::SeqCst)
            || bytes.is_empty()
            || bytes.len() > 1024
            || std::str::from_utf8(&bytes).is_err()
        {
            return Err("workspace.denied");
        }
        tokio::time::timeout(
            Duration::from_millis(250),
            entry.binding.wait_bootstrap_ready(),
        )
        .await
        .map_err(|_| "workspace.denied")?
        .map_err(|_| "workspace.denied")?;
        let credit = {
            let _admission = entry
                .admission
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            if entry.completed.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("workspace.denied");
            }
            if entry
                .resize_started
                .load(std::sync::atomic::Ordering::SeqCst)
                || !entry
                    .input_enabled
                    .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err("workspace.denied");
            }
            entry
                .input_credit
                .clone()
                .try_acquire_owned()
                .map_err(|_| "workspace.busy")?
        };
        // Keep actual input and both quotas supervised after IPC waiter cancellation.
        tokio::spawn(async move {
            let _credit = credit;
            entry
                .binding
                .input(bytes)
                .await
                .map_err(|_| "workspace.denied")
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    /// Layout-only current-page/gen control, independent of capture/input proof.
    /// The renderer retires input, waits for transport-specific layout completion and
    /// recaptures a new binding. IPC cancellation never aborts the native worker.
    async fn resize(
        &self,
        nonce: Uuid,
        ticket: Uuid,
        columns: u16,
        rows: u16,
    ) -> Result<pulse_term::session::ResizeReceipt> {
        if !(2..=160).contains(&columns) || !(2..=60).contains(&rows) {
            return Err("workspace.denied");
        }
        let size = pulse_core::TermSize::new(columns, rows).map_err(|_| "workspace.denied")?;
        let entry = self.entry(nonce, ticket)?;
        // The backend-issued binding already authenticates the current page,
        // generation and session ownership; no input permission is minted here.
        if entry.capture.lifecycle != pulse_term::session::Lifecycle::Running
            || !entry.connected.load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err("workspace.denied");
        }
        self.entry(nonce, ticket)?;
        {
            let _admission = entry
                .admission
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            if entry.completed.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("workspace.denied");
            }
            if entry
                .resize_started
                .swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                return Err("workspace.busy");
            }
            entry
                .input_enabled
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        tokio::spawn(async move {
            // Serial with already-admitted input; no quota release merely because
            // the caller stopped waiting. SSH completion never claims a remote kernel size proof.
            let _credit = entry
                .input_credit
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| "workspace.denied")?;
            entry
                .binding
                .resize_with_receipt(size)
                .await
                .map_err(|_| "workspace.denied")
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    fn ack(&self, nonce: Uuid, ticket: Uuid, sequence: u32) -> Result<()> {
        self.entry(nonce, ticket)?
            .binding
            .acknowledge(sequence)
            .map_err(|_| "workspace.denied")
    }
    fn detach(&self, nonce: Uuid, ticket: Uuid) -> Result<()> {
        let entry = self.retained_entry(nonce, ticket)?;
        entry.cancel.send_replace(true);
        self.entries
            .lock()
            .map_err(|_| "workspace.unavailable")?
            .remove(&ticket);
        Ok(())
    }
}
fn page(
    window: &WebviewWindow,
    state: &Workspace,
    nonce: &str,
) -> Result<(Uuid, Arc<pulse_app::registry::PageLaunchScope>)> {
    if !local_workspace(window) {
        return Err("workspace.denied");
    }
    current_document(
        &state.0.documents,
        state.0.owner,
        &window.url().map_err(|_| "workspace.denied")?,
        nonce,
    )
}
fn ticket(value: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| "workspace.denied")?;
    if id.to_string() != value {
        return Err("workspace.denied");
    }
    Ok(id)
}
#[tauri::command]
pub(super) async fn workspace_terminal_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    session: String,
) -> Result<Prepared> {
    let (nonce, scope) = page(&window, &state, &nonce)?;
    // IDs select a root-owned session, never create or authorize one. Exact string
    // matching rejects alternate encodings and avoids lossy JS numeric conversion.
    let id = state
        .0
        .sessions
        .lock()
        .map_err(|_| "workspace.unavailable")?
        .ids()
        .into_iter()
        .find(|id| id.get().to_string() == session)
        .ok_or("workspace.denied")?;
    let mut prepared = state
        .0
        .terminals
        .prepare(&state.0.registry, scope, id, nonce)
        .await?;
    if page(&window, &state, &nonce.to_string()).is_err() {
        let _ = state.0.terminals.detach(nonce, ticket(&prepared.ticket)?);
        return Err("workspace.denied");
    }
    prepared.appearance = state
        .0
        .sessions
        .lock()
        .map_err(|_| "workspace.unavailable")?
        .profile_appearance(id);
    Ok(prepared)
}
#[tauri::command]
pub(super) fn workspace_terminal_connect(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
    channel: tauri::ipc::Channel<RawFrame>,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    state
        .0
        .terminals
        .connect(nonce, ticket(&connection)?, channel)
}
#[tauri::command]
pub(super) fn workspace_terminal_ack(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
    sequence: u32,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    state.0.terminals.ack(nonce, ticket(&connection)?, sequence)
}
#[tauri::command]
pub(super) fn workspace_terminal_detach(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    state.0.terminals.detach(nonce, ticket(&connection)?)
}
/// Only the Commit-captured image, NOT a Marker/latest-state certificate.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CaptureDrawing {
    columns: u16,
    rows: u16,
    revision: String,
    source_bytes: String,
    utf8_bytes: String,
    digest: String,
}
#[tauri::command]
pub(super) async fn workspace_terminal_confirm(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
    rendered: CaptureDrawing,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    state
        .0
        .terminals
        .confirm(nonce, ticket(&connection)?, &rendered)
        .await?;
    eprintln!("workspace.terminal_capture_confirmed");
    Ok(())
}

#[tauri::command]
pub(super) async fn workspace_terminal_enable_input(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    state
        .0
        .terminals
        .enable_input(nonce, ticket(&connection)?)
        .await
}
#[tauri::command]
pub(super) async fn workspace_terminal_input(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
    bytes: Vec<u8>,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    state
        .0
        .terminals
        .input(nonce, ticket(&connection)?, bytes)
        .await
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HistorySearchArgs {
    query: String,
    match_case: bool,
    regex: bool,
    whole_word: bool,
    backwards: bool,
    alternate: bool,
    cursor: Option<usize>,
    revision: Option<String>,
}
#[tauri::command]
pub(super) async fn workspace_terminal_history_search(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    session: String,
    request: HistorySearchArgs,
) -> Result<pulse_term::HistorySearch> {
    let (_, scope) = page(&window, &state, &nonce)?;
    if request.query.is_empty()
        || request.query.len() > 512
        || request.query.chars().any(char::is_control)
        || request.cursor.is_some() != request.revision.is_some()
        || request.cursor.is_some_and(|cursor| cursor > 100_256)
    {
        return Err("workspace.denied");
    }
    let revision = request
        .revision
        .map(|value| {
            value
                .parse::<u64>()
                .ok()
                .filter(|number| number.to_string() == value)
                .ok_or("workspace.denied")
        })
        .transpose()?;
    let id = state
        .0
        .sessions
        .lock()
        .map_err(|_| "workspace.unavailable")?
        .ids()
        .into_iter()
        .find(|id| id.get().to_string() == session)
        .ok_or("workspace.denied")?;
    let request = pulse_term::HistorySearchRequest {
        query: request.query,
        match_case: request.match_case,
        regex: request.regex,
        whole_word: request.whole_word,
        backwards: request.backwards,
        alternate: request.alternate,
        cursor: request.cursor,
        revision,
    };
    tokio::time::timeout(
        Duration::from_secs(1),
        state.0.registry.history_search(&scope, id, request),
    )
    .await
    .map_err(|_| "workspace.history_busy")?
    .map_err(|error| match error {
        pulse_app::registry::RegistryError::History(pulse_term::HistoryPageError::Stale) => {
            "workspace.history_changed"
        }
        pulse_app::registry::RegistryError::History(pulse_term::HistoryPageError::Pattern) => {
            "workspace.history_pattern"
        }
        pulse_app::registry::RegistryError::History(pulse_term::HistoryPageError::TooLarge) => {
            "workspace.history_too_large"
        }
        pulse_app::registry::RegistryError::Busy => "workspace.history_busy",
        _ => "workspace.history_unavailable",
    })
}
#[tauri::command]
pub(super) async fn workspace_terminal_history_page(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    session: String,
    before: Option<usize>,
    revision: Option<String>,
) -> Result<pulse_term::HistoryPage> {
    let (_, scope) = page(&window, &state, &nonce)?;
    if before.is_some() != revision.is_some() {
        return Err("workspace.denied");
    }
    let revision = revision
        .map(|value| {
            value
                .parse::<u64>()
                .ok()
                .filter(|number| number.to_string() == value)
                .ok_or("workspace.denied")
        })
        .transpose()?;
    let id = state
        .0
        .sessions
        .lock()
        .map_err(|_| "workspace.unavailable")?
        .ids()
        .into_iter()
        .find(|id| id.get().to_string() == session)
        .ok_or("workspace.denied")?;
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        state.0.registry.history_page(&scope, id, before, revision),
    )
    .await
    .map_err(|_| "workspace.history_busy")?;
    result.map_err(|error| match error {
        pulse_app::registry::RegistryError::History(pulse_term::HistoryPageError::Stale) => {
            "workspace.history_changed"
        }
        pulse_app::registry::RegistryError::History(pulse_term::HistoryPageError::Pattern) => {
            "workspace.history_pattern"
        }
        pulse_app::registry::RegistryError::History(pulse_term::HistoryPageError::TooLarge) => {
            "workspace.history_too_large"
        }
        pulse_app::registry::RegistryError::Busy => "workspace.history_busy",
        _ => "workspace.history_unavailable",
    })
}
#[derive(serde::Serialize)]
pub(super) struct LayoutReceipt {
    columns: u16,
    rows: u16,
    basis: &'static str,
}
#[tauri::command]
pub(super) async fn workspace_terminal_resize(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
    columns: u16,
    rows: u16,
) -> Result<LayoutReceipt> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    let receipt = state
        .0
        .terminals
        .resize(nonce, ticket(&connection)?, columns, rows)
        .await?;
    let basis = match receipt.basis {
        Some(pulse_term::session::ResizeBasis::LocalNativeConfirmed) => "local-native-confirmed",
        Some(pulse_term::session::ResizeBasis::SshWindowRequestAdmitted) => "ssh-request-admitted",
        None => "unchanged",
    };
    eprintln!("workspace.terminal_resized.{columns}.{rows}");
    Ok(LayoutReceipt {
        columns: receipt.size.columns(),
        rows: receipt.size.rows(),
        basis,
    })
}
/// Observational frontend queue/focus-installed receipt, NEVER an extra permission.
#[tauri::command]
pub(super) fn workspace_terminal_input_ready(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    let entry = state.0.terminals.entry(nonce, ticket(&connection)?)?;
    if !entry
        .input_enabled
        .load(std::sync::atomic::Ordering::SeqCst)
        || entry
            .input_displayed
            .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err("workspace.denied");
    }
    eprintln!(
        "workspace.terminal_grid_ready.{}.{}",
        entry.capture.size.columns(),
        entry.capture.size.rows()
    );
    eprintln!("workspace.terminal_input_ready");
    Ok(())
}

/// Content-free diagnostic only. Reporting cannot authorize input or consume ACK.
#[tauri::command]
pub(super) fn workspace_terminal_report_failure(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    code: String,
) -> Result<()> {
    page(&window, &state, &nonce)?;
    match code.as_str() {
        "input.uncertain"
        | "input.transport"
        | "view.retired"
        | "terminal.dom_projection"
        | "terminal.dom_geometry"
        | "terminal.dom_rows"
        | "terminal.render_timeout"
        | "terminal.render_cancelled"
        | "terminal.render_unavailable"
        | "terminal.style_nonce_missing"
        | "terminal.connection_failed"
        | "terminal.write_failed"
        | "terminal.write_busy"
        | "terminal.surface_disposed"
        | "terminal.write_oversize"
        | "attach2.consume"
        | "attach2.queue_limit"
        | "attach2.cancelled"
        | "attach.sequence"
        | "attach.phase"
        | "attach.data"
        | "attach.checkpoint"
        | "attach.checkpoint_revision"
        | "attach.generation"
        | "attach.failed"
        | "attach.length"
        | "attach.kind"
        | "attach.limits"
        | "attach.part"
        | "attach.restore"
        | "attach.version_or_flags"
        | "attach2.failed"
        | "attach2.length_or_kind"
        | "attach2.source_regression"
        | "attach2.source_boundary"
        | "attach2.queue_limits"
        | "terminal.not_visible"
        | "terminal.bootstrap_timeout"
        | "terminal.non_raw_channel"
        | "terminal.consume_failed"
        | "workspace.denied"
        | "unknown" => {
            eprintln!("workspace.terminal_failure.{code}");
            Ok(())
        }
        _ => Err("workspace.denied"),
    }
}

#[path = "product_terminal_clipboard.rs"]
pub(super) mod clipboard;
#[path = "product_terminal_links.rs"]
pub(super) mod links;
#[cfg(test)]
#[path = "product_terminal_tests.rs"]
mod tests;
