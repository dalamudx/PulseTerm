//! One-use clipboard reads bound to a confirmed input view; native callbacks own credit.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(target_os = "linux", test))]
use tokio::sync::OwnedSemaphorePermit;
#[cfg(any(target_os = "linux", test))]
const CLIPBOARD_DEADLINE: Duration = Duration::from_secs(5);
#[derive(Clone)]
pub(super) struct Grant {
    id: Uuid,
    nonce: Uuid,
    #[cfg(any(target_os = "linux", test))]
    connection: Uuid,
    #[cfg(any(target_os = "linux", test))]
    pub(super) expires: tokio::time::Instant,
    cancelled: Arc<AtomicBool>,
    #[cfg(any(target_os = "linux", test))]
    started: bool,
}
#[cfg(any(target_os = "linux", test))]
pub(super) struct ReadJob {
    grant: Grant,
    _credit: OwnedSemaphorePermit,
}
#[cfg(any(target_os = "linux", test))]
impl ReadJob {
    pub(super) fn current(&self, bank: &TerminalBank) -> Result<()> {
        if self.grant.cancelled.load(Ordering::SeqCst)
            || tokio::time::Instant::now() >= self.grant.expires
        {
            return Err("workspace.denied");
        }
        bank.clipboard_current(self.grant.nonce, self.grant.connection)
    }
}
impl TerminalBank {
    #[cfg(any(target_os = "linux", test))]
    fn clipboard_current(&self, nonce: Uuid, connection: Uuid) -> Result<()> {
        let entry = self.entry(nonce, connection)?;
        let _admission = entry
            .admission
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if !entry.input_enabled.load(Ordering::SeqCst)
            || entry.resize_started.load(Ordering::SeqCst)
            || !entry.confirmed.load(Ordering::SeqCst)
        {
            return Err("workspace.denied");
        }
        entry
            .binding
            .check_current()
            .map_err(|_| "workspace.denied")
    }
    #[cfg(any(target_os = "linux", test))]
    pub(super) fn prepare_clipboard(&self, nonce: Uuid, connection: Uuid) -> Result<Uuid> {
        self.clipboard_current(nonce, connection)?;
        let mut slot = self.clipboard.lock().map_err(|_| "workspace.unavailable")?;
        if self.clipboard_reads.available_permits() == 0
            || slot
                .as_ref()
                .is_some_and(|g| tokio::time::Instant::now() < g.expires)
        {
            return Err("workspace.busy");
        }
        let id = Uuid::new_v4();
        *slot = Some(Grant {
            id,
            nonce,
            connection,
            expires: tokio::time::Instant::now() + CLIPBOARD_DEADLINE,
            cancelled: Arc::new(AtomicBool::new(false)),
            started: false,
        });
        Ok(id)
    }
    pub(super) fn cancel_clipboard(&self, nonce: Uuid, id: Uuid) -> Result<()> {
        let mut slot = self.clipboard.lock().map_err(|_| "workspace.unavailable")?;
        if slot
            .as_ref()
            .is_some_and(|g| g.nonce == nonce && g.id == id)
            && let Some(grant) = slot.take()
        {
            grant.cancelled.store(true, Ordering::SeqCst);
        }
        Ok(())
    }
    #[cfg(any(target_os = "linux", test))]
    pub(super) fn begin_clipboard(&self, nonce: Uuid, id: Uuid) -> Result<ReadJob> {
        let mut slot = self.clipboard.lock().map_err(|_| "workspace.unavailable")?;
        let grant = slot
            .as_mut()
            .filter(|g| g.nonce == nonce && g.id == id && !g.started)
            .ok_or("workspace.denied")?;
        if grant.cancelled.load(Ordering::SeqCst) || tokio::time::Instant::now() >= grant.expires {
            return Err("workspace.denied");
        }
        self.clipboard_current(nonce, grant.connection)?;
        let credit = self
            .clipboard_reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        grant.started = true;
        Ok(ReadJob {
            grant: grant.clone(),
            _credit: credit,
        })
    }
}
#[tauri::command]
pub(in super::super) fn workspace_terminal_clipboard_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
) -> Result<String> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    #[cfg(target_os = "linux")]
    {
        state
            .0
            .terminals
            .prepare_clipboard(nonce, ticket(&connection)?)
            .map(|id| id.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (nonce, connection);
        Err("workspace.denied")
    }
}
#[tauri::command]
pub(in super::super) fn workspace_terminal_clipboard_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    grant: String,
) -> Result<()> {
    if window.label() != state.native_label()? {
        return Err("workspace.denied");
    }
    state
        .0
        .terminals
        .cancel_clipboard(ticket(&nonce)?, ticket(&grant)?)
}
#[tauri::command]
pub(in super::super) async fn workspace_terminal_clipboard_read(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    grant: String,
) -> Result<String> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    let id = ticket(&grant)?;
    #[cfg(target_os = "linux")]
    {
        let state = state.inner().clone();
        let job = state.0.terminals.begin_clipboard(nonce, id)?;
        let source = window.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        window
            .with_webview(move |_| {
                if job.current(&state.0.terminals).is_err()
                    || !source.is_focused().unwrap_or(false)
                    || page(&source, &state, &nonce.to_string()).is_err()
                {
                    let _ = send.send(Err("workspace.denied"));
                    return;
                }
                gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).request_text(move |_, text| {
                    let result = job.current(&state.0.terminals).and_then(|()| {
                        if !source.is_focused().unwrap_or(false)
                            || page(&source, &state, &nonce.to_string()).is_err()
                        {
                            return Err("workspace.denied");
                        }
                        let text = text.ok_or("workspace.clipboard_unavailable")?;
                        if text.len() > 1012 {
                            return Err("workspace.clipboard_unavailable");
                        }
                        Ok(text.to_owned())
                    });
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "workspace.clipboard_read_finished.{}",
                        if result.is_ok() { "ok" } else { "rejected" }
                    );
                    let _ = send.send(result);
                    drop(job);
                });
            })
            .map_err(|_| "workspace.clipboard_unavailable")?;
        receive
            .await
            .map_err(|_| "workspace.clipboard_unavailable")?
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (nonce, id);
        Err("workspace.denied")
    }
}
