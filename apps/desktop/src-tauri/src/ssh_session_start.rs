//! Supervised saved-source SSH startup. No Tauri entry, arbitrary path/program or plaintext result.
use super::kinds::Intent;
use pulse_app::{
    local_session_root::SessionRoot,
    protocol_session_owner::{
        ProtocolSessionError, ProtocolSessionReservation, StartedProtocolSession,
    },
    registry::{PageLaunchScope, SessionId, SessionRegistry},
    vault_owner::PageVaultCancellation,
};
use pulse_core::TermSize;
use pulse_ssh::peer::{NativeIoCompletion, PinnedTransport};
use pulse_store::StateStore;
use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::{Arc, Mutex},
};
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) struct StartContext {
    pub registry: SessionRegistry,
    pub page: Arc<PageLaunchScope>,
    pub root: Arc<Mutex<SessionRoot>>,
    pub store: StateStore,
    pub size: TermSize,
    pub history_rows: usize,
    pub palette: pulse_app::TerminalStartConfig,
}
/// Drop revokes the ORIGINAL intent, never aborts the cleanup supervisor or closes an adopted session.
pub(crate) struct SessionStartTask {
    result: tokio::sync::oneshot::Receiver<Result<SessionId>>,
    cancel: PageVaultCancellation,
    received: bool,
}
impl SessionStartTask {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub async fn wait(mut self) -> Result<SessionId> {
        let result = (&mut self.result)
            .await
            .map_err(|_| "workspace.ssh_failed")?;
        self.received = true;
        result
    }
}
impl Drop for SessionStartTask {
    fn drop(&mut self) {
        if !self.received {
            self.cancel();
        }
    }
}
/// Trusted native caller must first create an EXPLICIT saved-session intent for this original page.
pub(crate) fn start_prepared(context: StartContext, intent: Intent) -> Result<SessionStartTask> {
    context
        .registry
        .check_launch_page(&context.page)
        .map_err(|_| "workspace.denied")?;
    context
        .root
        .lock()
        .map_err(|_| "workspace.ssh_failed")?
        .preflight_protocol(&context.registry)
        .map_err(|_| "workspace.denied")?;
    if context.size.columns() < 2
        || context.size.columns() > 160
        || context.size.rows() < 2
        || context.size.rows() > 60
        || context.history_rows > 10000
    {
        return Err("workspace.denied");
    }
    let started = ProtocolSessionReservation::reserve(&context.registry, &context.page)
        .map_err(|_| "workspace.busy")?
        .begin()
        .map_err(|_| "workspace.denied")?;
    let cancel = intent.cancellation();
    let (sender, result) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = execute(context, intent, started).await;
        let _ = sender.send(result);
    });
    Ok(SessionStartTask {
        result,
        cancel,
        received: false,
    })
}
async fn execute(
    context: StartContext,
    intent: Intent,
    started: StartedProtocolSession,
) -> Result<SessionId> {
    let mut intent = Some(intent);
    let mut started = Some(started);
    let mut native = None;
    let outcome = establish(&context, &mut intent, &mut started, &mut native).await;
    if outcome.is_err() {
        if let Some(started) = started.take() {
            started
                .finish_unregistered(async move {
                    if let Some(native) = native {
                        native
                            .wait()
                            .await
                            .map_err(|_| ProtocolSessionError::NativeUnknown)?;
                    }
                    drop(intent);
                    Ok(())
                })
                .join()
                .await
                .map_err(|_| "workspace.ssh_cleanup_failed")?;
        } else if let Some(native) = native {
            // Any moved actor/SDK supervisor remains tracked by the ORIGINAL registry's native ledger.
            // This wait proves physical stream exit, NOT its actor/SDK join or a remote exit status.
            native
                .wait()
                .await
                .map_err(|_| "workspace.ssh_cleanup_failed")?;
        }
    }
    outcome
}
async fn establish(
    context: &StartContext,
    intent: &mut Option<Intent>,
    started: &mut Option<StartedProtocolSession>,
    native: &mut Option<NativeIoCompletion>,
) -> Result<SessionId> {
    let prepared = intent.as_ref().ok_or("workspace.denied")?;
    let guard = prepared.authorization();
    let cancelled = prepared.cancellation();
    let deadline = tokio::time::Instant::from_std(prepared.deadline());
    if !guard() {
        return Err("workspace.denied");
    }
    let (target, pins, (host, identity)) = prepared.dial_target()?;
    // Resolve under the supervisor, NEVER abandon a blocking DNS worker on cancellation.
    // Original authorization may expire while cleanup waits; that cannot authorize another dial.
    let host_name = target.host().to_owned();
    let port = target.port();
    let addresses = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<SocketAddr>> {
        Ok((host_name.as_str(), port)
            .to_socket_addrs()?
            .take(8)
            .collect())
    })
    .await
    .map_err(|_| "workspace.ssh_failed")?
    .map_err(|_| "workspace.ssh_dial_failed")?;
    if !guard() || tokio::time::Instant::now() >= deadline {
        return Err("workspace.denied");
    }
    let mut stream = None;
    for address in addresses {
        if !guard() {
            return Err("workspace.denied");
        }
        let connected = tokio::select! {biased;
         _=cancelled.wait_cancelled()=>return Err("workspace.denied"),
         _=tokio::time::sleep_until(deadline)=>return Err("workspace.denied"),
         result=tokio::net::TcpStream::connect(address)=>result,
        };
        if let Ok(tcp) = connected {
            stream = Some(tcp);
            break;
        }
    }
    let stream = stream.ok_or("workspace.ssh_dial_failed")?;
    let pending = PinnedTransport::begin_connect_stream(
        target,
        Some(pins),
        None,
        Arc::new(russh::client::Config::default()),
        stream,
    )
    .map_err(|_| "workspace.ssh_failed")?;
    *native = Some(pending.native_io_completion());
    let connection = tokio::select! {biased;
     _=cancelled.wait_cancelled()=>return Err("workspace.denied"),
     _=tokio::time::sleep_until(deadline)=>return Err("workspace.denied"),
     result=pending.connect()=>result.map_err(|_|"workspace.ssh_peer_failed")?,
    };
    if !guard() {
        connection
            .close_and_join()
            .await
            .map_err(|_| "workspace.ssh_cleanup_failed")?;
        return Err("workspace.denied");
    }
    let temporary = intent.as_ref().ok_or("workspace.denied")?.is_temporary();
    let owner = Intent::establish(
        intent,
        context,
        connection,
        (host, identity),
        started,
        native,
    )
    .await?;
    // Registration is the handoff boundary. After it succeeds, page loss is NOT session close.
    let id = context
        .root
        .lock()
        .map_err(|_| "workspace.ssh_failed")?
        .adopt_protocol(owner)
        .map_err(|_| "workspace.denied")?;
    // Device-local metadata is recorded only after successful adoption. Failure cannot
    // turn an already registered session into a retryable startup failure. The
    // supervisor retains its lifetime credit while the shared SQL worker exits.
    if !temporary && context.store.record_recent_ssh_host(host).await.is_err() {
        eprintln!("workspace.recent_host_record_failed");
    }
    Ok(id)
}
