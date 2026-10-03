//! Runtime-neutral application composition for supervised local terminal sessions.
#![forbid(unsafe_code)]
pub mod attachment;
mod native_control;
pub mod registry;
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
#[error("local session: {0}")]
pub struct LocalSessionError(String);
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
/// Combined actor/native receipt. Dropping it requests close, never aborts native
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
        mut self,
    ) -> Result<(SessionState, pulse_term::ObservedScreen, NativeExit), LocalSessionError> {
        let actor = self
            .actor
            .take()
            .ok_or_else(|| LocalSessionError("actor receipt consumed".into()))?;
        let (state, screen) = actor
            .join_with_screen()
            .await
            .map_err(|e| LocalSessionError(e.to_string()))?;
        let native = self
            .native
            .take()
            .ok_or_else(|| LocalSessionError("native receipt consumed".into()))?
            .await
            .map_err(|e| LocalSessionError(e.to_string()))??;
        Ok((state, screen, native))
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
    let (transport, native) = tokio::task::spawn_blocking(move || setup(command, size))
        .await
        .map_err(|e| LocalSessionError(e.to_string()))??;
    let (handle, actor) = session::spawn_session(size, history, transport);
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
    let mut pty =
        PtySession::spawn(&command, size).map_err(|e| LocalSessionError(e.to_string()))?;
    let mut reader = pty
        .take_reader()
        .map_err(|e| LocalSessionError(e.to_string()))?;
    let mut writer = pty
        .take_writer()
        .map_err(|e| LocalSessionError(e.to_string()))?;
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
        .map_err(|e| LocalSessionError(e.to_string()))?;
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
        })
        .map_err(|e| LocalSessionError(e.to_string()))?;
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
                        .map_err(|e| LocalSessionError(e.to_string()))?;
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
                                .map_err(|e| LocalSessionError(e.to_string()))?;
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
                    .map_err(|_| LocalSessionError("native reader panicked".into()))?;
                writer_thread
                    .join()
                    .map_err(|_| LocalSessionError("native writer panicked".into()))?;
                if *failed.borrow() {
                    return Err(LocalSessionError("native I/O failed".into()));
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
        .map_err(|e| LocalSessionError(e.to_string()))?;
    Ok((transport, native))
}
