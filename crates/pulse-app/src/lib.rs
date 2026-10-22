//! Runtime-neutral application composition for supervised local terminal sessions.
#![forbid(unsafe_code)]
pub mod attachment;
/// Opt-in Linux/Windows local control transport; authentication is not effect authorization.
#[cfg(all(feature = "cli-control", any(target_os = "linux", windows)))]
pub mod cli_control;
/// Trusted runtime scheduling for a registered local PTY; no frontend command surface.
pub mod local_launch;
/// Application-owned local-session completion and cleanup, independent of page lifetime.
pub mod local_session_owner;
/// Root-owned local sessions and pending native cleanup; never put in document state.
pub mod local_session_root;
mod native_control;
pub mod native_lifecycle;
/// Bounded native document lifecycle for a future product workspace, independent of probes.
pub mod product_document;
/// Validated local app-state preferences to automatic profile catalog composition.
pub mod profile_state;
/// Protocol-neutral native lifetime ownership; trusted backend adapters only, no network dependency.
pub mod protocol_session_owner;
pub mod registry;
/// Backend-only generation-scoped settings capture and publication; not window authority.
pub mod settings_context;
/// Validated theme to terminal query/rendering base; no input or window permission.
pub mod terminal_palette;
mod terminal_start_config;
pub use terminal_start_config::TerminalStartConfig;
/// Trusted current-document v2 terminal views; not frontend or session-launch authority.
pub mod terminal_view;
/// Backend theme-selection epochs and explicit publication; not window/GUI authority.
pub mod theme_context;
/// Current-page opaque unlock handoff; frontend confirmation/cancellation is separate from key auth.
pub mod vault_handoff;
/// Independent original-page private-key/passphrase ciphertext write tickets; no SSH or read authority.
pub mod vault_key_write_handoff;
/// Native single-use saved-password tickets; not credential reading or SSH authority.
pub mod vault_password_handoff;
/// Original-page action preparation; not CLI authentication or an effect executor.
pub mod window_action;
/// Native-only multiwindow routing and lifecycle; labels never grant page authority.
pub mod window_directory;
#[cfg(test)]
fn test_tempdir() -> std::io::Result<tempfile::TempDir> {
    let root = std::env::temp_dir();
    // macOS exposes temporary storage through /var; private stores reject symlink ancestors.
    #[cfg(unix)]
    let root = root.canonicalize()?;
    tempfile::tempdir_in(root)
}
#[cfg(all(test, unix))]
pub(crate) static VAULT_CRYPTO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Rust-only current-page Vault operations; no password IPC or SecretRef-use permission.
pub mod vault_owner;
use pulse_core::TermSize;
use pulse_pty::{PtyCommand, PtySession};
use pulse_term::session::{
    self, ChannelTransport, DriverEndpoint, MAX_CHUNK, SessionError, SessionHandle, SessionState,
    SessionTask, TRANSPORT_QUEUE_CAPACITY, TransportWrite,
};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc as blocking};
use std::time::Duration;
use tokio::sync::oneshot;
/// Local diagnostics only; do not display unredacted backend errors remotely.
#[derive(Debug, thiserror::Error)]
#[error("local session: {message}")]
pub struct LocalSessionError {
    message: String,
    before_native: bool,
}
impl LocalSessionError {
    fn unconfirmed(message: String) -> Self {
        Self {
            message,
            before_native: false,
        }
    }
    fn from_pty(error: pulse_pty::PtyError) -> Self {
        Self {
            before_native: matches!(error, pulse_pty::PtyError::WorkingDirectory),
            message: error.to_string(),
        }
    }
    pub(crate) fn rejected_before_native(&self) -> bool {
        self.before_native
    }
}
/// Direct-child completion, after all native workers have joined.
#[derive(Debug)]
pub struct NativeExit {
    /// Native exit code, preserved also for nonzero normal termination.
    pub exit_code: u32,
    /// Controller requested termination of a still-running child. A reaped exit
    /// wins over a simultaneous stop; an already-ended child is not cancellation.
    pub cancelled: bool,
    /// Successful native reads admitted to the fixed-capacity output queue.
    pub read_bytes: u64,
    /// Completed input and authoritative query-response bytes.
    pub written_bytes: u64,
    /// Completed native resize operations.
    pub resize_count: u64,
}
/// Native completion and actor metadata are separate: a remote D status is not exit_code.
#[derive(Debug)]
pub struct LocalSessionReceipt {
    /// Final actor state, projection and undrained untrusted hints.
    pub actor: pulse_term::session::SessionReceipt,
    /// Actual native child status, after reader/writer/controller join.
    pub native: NativeExit,
}

/// Combined actor/native task. Dropping it requests close, never aborts native
/// workers. Descendant process trees and OS-hung syscalls are not yet guaranteed.
pub struct LocalSessionTask {
    handle: SessionHandle,
    actor: Option<SessionTask>,
    native: Option<oneshot::Receiver<Result<NativeExit, LocalSessionError>>>,
}
impl Drop for LocalSessionTask {
    fn drop(&mut self) {
        self.handle.close();
    }
}
impl LocalSessionTask {
    /// Join actor then direct child, reader, writer and native controller. Caller
    /// owns a finite deadline; cancelling this future requests out-of-band close.
    pub async fn join(self) -> Result<(SessionState, NativeExit), LocalSessionError> {
        let (state, _screen, native) = self.join_with_screen().await?;
        Ok((state, native))
    }
    /// Diagnostic visible screen at the final actor boundary, including EOF decoding.
    /// Not a complete snapshot. Native workers are joined under the caller's deadline.
    pub async fn join_with_screen(
        self,
    ) -> Result<(SessionState, pulse_term::ObservedScreen, NativeExit), LocalSessionError> {
        let receipt = self.join_with_receipt().await?;
        Ok((receipt.actor.state, receipt.actor.screen, receipt.native))
    }
    /// Preserve final bounded hints after true EOF or explicit stop; no synthetic D.
    /// Native workers still join, and native exit remains independent of untrusted hints.
    pub async fn join_with_receipt(mut self) -> Result<LocalSessionReceipt, LocalSessionError> {
        let actor = self
            .actor
            .take()
            .ok_or_else(|| LocalSessionError::unconfirmed("actor receipt consumed".into()))?;
        let actor = actor
            .join_with_receipt()
            .await
            .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))?;
        let native = self
            .native
            .take()
            .ok_or_else(|| LocalSessionError::unconfirmed("native receipt consumed".into()))?
            .await
            .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))??;
        Ok(LocalSessionReceipt { actor, native })
    }
}
/// Spawn/setup runs on a blocking pool; ongoing I/O uses three supervised native
/// workers, not the UI/runtime worker pool. Fixed QUEUE output and input buffers
/// carry at most MAX_CHUNK per data message. No terminal state is shared by mutex.
pub async fn spawn_local(
    command: PtyCommand,
    size: TermSize,
    history: usize,
) -> Result<(SessionHandle, LocalSessionTask), LocalSessionError> {
    spawn_local_with_palette(command, size, history, pulse_term::QueryPalette::default()).await
}

/// Trusted native startup with a fixed base before the actor processes output.
pub async fn spawn_local_with_palette(
    mut command: PtyCommand,
    size: TermSize,
    history: usize,
    config: impl Into<TerminalStartConfig>,
) -> Result<(SessionHandle, LocalSessionTask), LocalSessionError> {
    let config = config.into();
    config.apply_local_term(&mut command);
    let (transport, native) = tokio::task::spawn_blocking(move || setup(command, size))
        .await
        .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))??;
    let (handle, actor) = session::spawn_session_with_history_limit(
        size,
        history,
        transport,
        config.palette,
        config.history_limit,
    );
    let task = LocalSessionTask {
        handle: handle.clone(),
        actor: Some(actor),
        native: Some(native),
    };
    Ok((handle, task))
}
type NativeReceipt = oneshot::Receiver<Result<NativeExit, LocalSessionError>>;
fn setup(
    command: PtyCommand,
    size: TermSize,
) -> Result<(ChannelTransport, NativeReceipt), LocalSessionError> {
    let mut pty = PtySession::spawn(&command, size).map_err(LocalSessionError::from_pty)?;
    let mut reader = pty
        .take_reader()
        .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))?;
    let mut writer = pty
        .take_writer()
        .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))?;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let DriverEndpoint {
        output,
        mut input,
        stop,
        failed,
    } = driver;
    let reads = Arc::new(AtomicU64::new(0));
    let writes = Arc::new(AtomicU64::new(0));
    let read_count = reads.clone();
    let eof = Arc::new(AtomicBool::new(false));
    let reader_eof = eof.clone();
    let read_failure = failed.clone();
    let read_stop = stop.clone();
    let reader_thread = std::thread::Builder::new()
        .name("pulse-pty-read".into())
        .spawn(move || {
            let mut buf = vec![0; MAX_CHUNK];
            let mut delivering = true;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        reader_eof.store(true, Ordering::Release);
                        break;
                    }
                    Ok(n) => {
                        if delivering {
                            if output.blocking_send(buf[..n].to_vec()).is_err() {
                                delivering = false;
                            } else {
                                read_count.fetch_add(n as u64, Ordering::Relaxed);
                            }
                        } // Always drain during ConPTY teardown even after sink cancellation.
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        if !*read_stop.borrow() && read_stop.has_changed().is_ok() {
                            read_failure.send_replace(true);
                        }
                        break;
                    }
                }
            }
        })
        .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))?;
    let (resize_tx, resize_rx) = blocking::sync_channel::<(
        TermSize,
        oneshot::Sender<Result<(), SessionError>>,
    )>(TRANSPORT_QUEUE_CAPACITY);
    let write_count = writes.clone();
    let write_failure = failed.clone();
    let write_stop = stop.clone();
    let writer_thread = std::thread::Builder::new()
        .name("pulse-pty-write".into())
        .spawn(move || {
            while let Some(command) = input.blocking_recv() {
                match command {
                    TransportWrite::Input(bytes) | TransportWrite::Reply(bytes) => {
                        if writer
                            .write_all(&bytes)
                            .and_then(|_| writer.flush())
                            .is_err()
                        {
                            if !*write_stop.borrow() && write_stop.has_changed().is_ok() {
                                write_failure.send_replace(true);
                            }
                            break;
                        }
                        write_count.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    }
                    TransportWrite::Resize(size, reply) => {
                        match resize_tx.try_send((size, reply)) {
                            Ok(()) => {}
                            Err(error) => {
                                let (_, reply) = match error {
                                    blocking::TrySendError::Full(value)
                                    | blocking::TrySendError::Disconnected(value) => value,
                                };
                                let _ = reply.send(Err(SessionError::Busy));
                            }
                        }
                    }
                }
            }
            // Unix portable-pty sends newline + VEOF when its writer drops.
            // Retain that writer in the JoinHandle until child exit is known.
            #[cfg(unix)]
            {
                writer
            }
            #[cfg(not(unix))]
            {
                drop(writer);
            }
        })
        .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))?;
    let (completion, native) = oneshot::channel();
    std::thread::Builder::new()
        .name("pulse-pty-control".into())
        .spawn(move || {
            let result = (|| {
                let mut resizes = 0;
                let mut cancelled = false;
                let mut eof_deadline = None;
                let exit = loop {
                    // EOF can stop the actor before this controller samples the child.
                    // Observe an already-reaped exit before deciding to terminate.
                    let observed = pty
                        .try_wait()
                        .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))?;
                    let stopping =
                        *stop.borrow() || stop.has_changed().is_err() || *failed.borrow();
                    let eof_grace = native_control::eof_grace(
                        stopping,
                        *failed.borrow(),
                        eof.load(Ordering::Acquire),
                        std::time::Instant::now(),
                        &mut eof_deadline,
                    );
                    match native_control::action(observed, stopping && !eof_grace) {
                        native_control::Action::Exited(exit) => break exit,
                        native_control::Action::Terminate => {
                            cancelled = true;
                            break pty
                                .terminate()
                                .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))?;
                        }
                        native_control::Action::Wait => {}
                    }
                    match resize_rx.recv_timeout(Duration::from_millis(20)) {
                        Ok((size, reply)) => {
                            let result = pty.resize(size).map_err(|_| SessionError::Transport);
                            if result.is_ok() {
                                resizes += 1;
                            }
                            let _ = reply.send(result);
                        }
                        Err(blocking::RecvTimeoutError::Timeout) => {}
                        Err(blocking::RecvTimeoutError::Disconnected) => {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                };
                // Reader concurrently drains ConPTY before and during master destruction.
                drop(pty);
                reader_thread
                    .join()
                    .map_err(|_| LocalSessionError::unconfirmed("native reader panicked".into()))?;
                // Joining releases the returned writer only after child exit,
                // so input closure cannot execute a pending Readline buffer.
                writer_thread
                    .join()
                    .map_err(|_| LocalSessionError::unconfirmed("native writer panicked".into()))?;
                if *failed.borrow() {
                    return Err(LocalSessionError::unconfirmed("native I/O failed".into()));
                }
                Ok(NativeExit {
                    exit_code: exit.exit_code(),
                    cancelled,
                    read_bytes: reads.load(Ordering::Relaxed),
                    written_bytes: writes.load(Ordering::Relaxed),
                    resize_count: resizes,
                })
            })();
            let _ = completion.send(result);
        })
        .map_err(|e| LocalSessionError::unconfirmed(e.to_string()))?;
    Ok((transport, native))
}
