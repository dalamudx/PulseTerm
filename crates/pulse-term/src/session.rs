//! Bounded, window-independent actor foundation with an EchoTransport test driver.
//! Screen inspection is a diagnostic projection, NOT a detach/attach snapshot.
use crate::{ObservedScreen, TerminalProbe};
use pulse_core::TermSize;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

/// Largest individual input/output chunk accepted by this foundation.
pub const MAX_CHUNK: usize = 64 * 1024;
const QUEUE: usize = 8;
const MAX_REPLIES: usize = 256 * 1024;

/// Coarse lifecycle, not linked to any view or window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Actor accepts commands and parses output.
    Running,
    /// Explicit close or last handle dropped; pending I/O may be cancelled.
    Closed,
    /// Transport EOF was processed, including incomplete UTF-8 tail.
    Ended,
    /// Bounded protocol/transport failure; not a silent output drop.
    Failed,
}
/// Small observable metadata; no terminal bytes or grid is stored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionState {
    /// Coarse lifecycle.
    pub lifecycle: Lifecycle,
    /// Current backend dimensions.
    pub size: TermSize,
    /// Number of output bytes actually parsed (not queued).
    pub parsed_bytes: u64,
    /// Monotonic state revision for polling/diagnostic consumers.
    pub revision: u64,
}
/// Explicit overload and lifecycle errors. A full queue never implies acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    /// More than MAX_CHUNK bytes in one message.
    #[error("session chunk exceeds 64 KiB")]
    ChunkTooLarge,
    /// Caller must retry or propagate pressure; message was not accepted.
    #[error("session bounded queue is full")]
    Busy,
    /// Actor no longer accepts commands.
    #[error("session is closed")]
    Closed,
    /// Reserved for bad transport events/protocol response overflow.
    #[error("session transport failed")]
    Transport,
}

enum Command {
    Input(Vec<u8>, oneshot::Sender<Result<(), SessionError>>),
    Resize(TermSize, oneshot::Sender<Result<(), SessionError>>),
    Palette(
        crate::QueryPalette,
        oneshot::Sender<Result<(), SessionError>>,
    ),
    CellPixels(
        Option<(u16, u16)>,
        oneshot::Sender<Result<(), SessionError>>,
    ),
    Inspect(oneshot::Sender<ObservedScreen>),
}
#[derive(Debug)]
enum IoCommand {
    Input(Vec<u8>),
    Reply(Vec<u8>),
}

/// Bounded channel endpoint consumed by the terminal actor. This is not yet the
/// production async Transport trait or a PTY blocking-I/O bridge.
pub struct ChannelTransport {
    output: mpsc::Receiver<Vec<u8>>,
    input: mpsc::Sender<IoCommand>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}
/// Test observations are bounded metadata, not a recording of terminal content.
#[derive(Clone)]
pub struct EchoObservation {
    input_bytes: Arc<AtomicU64>,
    reply_bytes: Arc<AtomicU64>,
    last_reply: watch::Receiver<Vec<u8>>,
}
impl EchoObservation {
    /// Number of user input bytes processed by the echo driver.
    pub fn input_bytes(&self) -> u64 {
        self.input_bytes.load(Ordering::SeqCst)
    }
    /// Number of backend reply bytes consumed upstream by the driver.
    pub fn reply_bytes(&self) -> u64 {
        self.reply_bytes.load(Ordering::SeqCst)
    }
    /// Most recent backend reply; at most MAX_CHUNK bytes.
    pub fn last_reply(&self) -> Vec<u8> {
        self.last_reply.borrow().clone()
    }
}
/// Synthetic transport for actor tests: user input becomes output; authoritative
/// replies are observed upstream and are not fed back as new program output.
pub struct EchoTransport;
impl EchoTransport {
    /// Spawn bounded input/output channels and a cooperatively cancelled driver.
    /// Must be called inside a Tokio runtime.
    pub fn spawn() -> (ChannelTransport, EchoObservation) {
        let (input, mut commands) = mpsc::channel(QUEUE);
        let (output, receiver) = mpsc::channel(QUEUE);
        let (stop, mut stopping) = watch::channel(false);
        let (last, last_reply) = watch::channel(vec![]);
        let input_bytes = Arc::new(AtomicU64::new(0));
        let reply_bytes = Arc::new(AtomicU64::new(0));
        let observation = EchoObservation {
            input_bytes: input_bytes.clone(),
            reply_bytes: reply_bytes.clone(),
            last_reply,
        };
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = stopping.changed() => break,
                    command = commands.recv() => match command {
                        None => break,
                        Some(IoCommand::Input(bytes)) => {
                            let count = bytes.len() as u64;
                            tokio::select! { biased;
                                _ = stopping.changed() => break,
                                result = output.send(bytes) => { if result.is_err() { break; } }
                            }
                            input_bytes.fetch_add(count, Ordering::SeqCst);
                        }
                        Some(IoCommand::Reply(bytes)) => {
                            reply_bytes.fetch_add(bytes.len() as u64, Ordering::SeqCst);
                            last.send_replace(bytes);
                        }
                    }
                }
            }
        });
        (
            ChannelTransport {
                output: receiver,
                input,
                stop,
                task,
            },
            observation,
        )
    }
}
impl Drop for ChannelTransport {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        self.task.abort(); // Also closes a driver blocked on a full channel.
    }
}

/// Cloneable control handle. Dropping a view does not close it: callers keep the
/// handle in the session registry. Dropping the last handle is explicit abandonment.
#[derive(Clone)]
pub struct SessionHandle {
    commands: mpsc::Sender<Command>,
    cancel: watch::Sender<bool>,
    /// Coarse state subscription. Cloning this receiver is not an ownership lease.
    pub state: watch::Receiver<SessionState>,
}
impl SessionHandle {
    /// Try bounded command admission, then wait for actor acceptance. Input may be
    /// cancelled later on explicit close; success is NOT a physical-write receipt.
    pub async fn input(&self, bytes: Vec<u8>) -> Result<(), SessionError> {
        if bytes.len() > MAX_CHUNK {
            return Err(SessionError::ChunkTooLarge);
        }
        let (reply, result) = oneshot::channel();
        self.enqueue(Command::Input(bytes, reply))?;
        result.await.map_err(|_| SessionError::Closed)?
    }
    /// Change the owned parser size. Echo has no OS dimensions to update.
    pub async fn resize(&self, size: TermSize) -> Result<(), SessionError> {
        let (reply, result) = oneshot::channel();
        self.enqueue(Command::Resize(size, reply))?;
        result.await.map_err(|_| SessionError::Closed)?
    }
    /// Serialize a UI palette update with output handling; remote OSC overrides
    /// keep precedence until the program resets them.
    pub async fn set_query_palette(
        &self,
        palette: crate::QueryPalette,
    ) -> Result<(), SessionError> {
        let (reply, result) = oneshot::channel();
        self.enqueue(Command::Palette(palette, reply))?;
        result.await.map_err(|_| SessionError::Closed)?
    }
    /// Set measured cell geometry, or None when no renderer reports it.
    pub async fn set_cell_pixels(&self, pixels: Option<(u16, u16)>) -> Result<(), SessionError> {
        let (reply, result) = oneshot::channel();
        self.enqueue(Command::CellPixels(pixels, reply))?;
        result.await.map_err(|_| SessionError::Closed)?
    }
    /// Return a visible-screen diagnostic, never a complete restore snapshot.
    pub async fn inspect(&self) -> Result<ObservedScreen, SessionError> {
        let (reply, result) = oneshot::channel();
        self.enqueue(Command::Inspect(reply))?;
        result.await.map_err(|_| SessionError::Closed)
    }
    fn enqueue(&self, command: Command) -> Result<(), SessionError> {
        if self.state.borrow().lifecycle != Lifecycle::Running {
            return Err(SessionError::Closed);
        }
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => SessionError::Busy,
                mpsc::error::TrySendError::Closed(_) => SessionError::Closed,
            })
    }
    /// Out-of-band cancellation is not queued behind data or inspect requests.
    pub fn close(&self) {
        self.cancel.send_replace(true);
    }
}
/// Supervision receipt; do not abort it to close a session. Use SessionHandle::close.
/// Join completion means the synthetic transport driver was also joined.
pub struct SessionTask(JoinHandle<(SessionState, ObservedScreen)>);
impl SessionTask {
    /// Wait for the actor and driver to exit. Apply an application-owned deadline.
    pub async fn join(self) -> Result<SessionState, tokio::task::JoinError> {
        self.0.await.map(|(state, _screen)| state)
    }
    /// Diagnostic final visible screen, including EOF replacement characters.
    /// This does not contain full restore modes or historical state.
    pub async fn join_with_screen(
        self,
    ) -> Result<(SessionState, ObservedScreen), tokio::task::JoinError> {
        self.0.await
    }
}
/// Spawn one owner of parser/grid state. No shared Mutex<Term>, no GUI references.
/// Uses the existing probe parser, so snapshot/OSC/color limitations still apply.
pub fn spawn_session(
    size: TermSize,
    history: usize,
    mut transport: ChannelTransport,
) -> (SessionHandle, SessionTask) {
    let (commands, mut receiver) = mpsc::channel(QUEUE);
    let (cancel, mut cancelled) = watch::channel(false);
    let initial = SessionState {
        lifecycle: Lifecycle::Running,
        size,
        parsed_bytes: 0,
        revision: 0,
    };
    let (state, subscription) = watch::channel(initial.clone());
    let handle = SessionHandle {
        commands,
        cancel,
        state: subscription,
    };
    let task = tokio::spawn(async move {
        let mut terminal = TerminalProbe::new(size, history);
        let mut current = initial;
        let mut replies = VecDeque::<Vec<u8>>::new();
        let mut reply_bytes = 0;
        loop {
            // Check cancellation before each bounded processing turn, then use
            // fair selection so sustained output cannot starve resize/inspect.
            if *cancelled.borrow() || cancelled.has_changed().is_err() {
                current.lifecycle = Lifecycle::Closed;
                break;
            }
            tokio::select! {
                _ = cancelled.changed() => { current.lifecycle = Lifecycle::Closed; break; }
                permit = transport.input.reserve(), if !replies.is_empty() => {
                    match permit {
                        Ok(permit) => {
                            if let Some(reply) = replies.pop_front() {
                                reply_bytes -= reply.len(); permit.send(IoCommand::Reply(reply));
                            }
                        }
                        Err(_) => { current.lifecycle = Lifecycle::Failed; break; }
                    }
                }
                // Output remains readable while flushing replies so a driver stuck
                // behind its output queue cannot deadlock against input pressure.
                output = transport.output.recv() => {
                    match output {
                        Some(bytes) if bytes.len() <= MAX_CHUNK => {
                            terminal.feed(&bytes);
                            current.parsed_bytes += bytes.len() as u64;
                            for reply in terminal.take_replies() {
                                if reply.len() > MAX_CHUNK || reply_bytes + reply.len() > MAX_REPLIES {
                                    current.lifecycle = Lifecycle::Failed; break;
                                }
                                reply_bytes += reply.len(); replies.push_back(reply.into_bytes());
                            }
                            if current.lifecycle == Lifecycle::Failed { break; }
                            current.revision += 1; state.send_replace(current.clone());
                        }
                        Some(_) => { current.lifecycle = Lifecycle::Failed; break; }
                        None => {
                            terminal.finish_input();
                            current.lifecycle = if replies.is_empty() { Lifecycle::Ended } else { Lifecycle::Failed };
                            break;
                        }
                    }
                }
                command = receiver.recv() => match command {
                    None => { current.lifecycle = Lifecycle::Closed; break; }
                    Some(Command::Input(bytes, reply)) => {
                        let result = if !replies.is_empty() { Err(SessionError::Busy) } else {
                            transport.input.try_send(IoCommand::Input(bytes)).map_err(|error| match error {
                                mpsc::error::TrySendError::Full(_) => SessionError::Busy,
                                mpsc::error::TrySendError::Closed(_) => SessionError::Transport,
                            })
                        };
                        let _ = reply.send(result);
                    }
                    Some(Command::Resize(size, reply)) => {
                        terminal.resize(size); current.size = size; current.revision += 1;
                        state.send_replace(current.clone()); let _ = reply.send(Ok(()));
                    }
                    Some(Command::Palette(palette, reply)) => {
                        terminal.set_query_palette(palette); let _ = reply.send(Ok(()));
                    }
                    Some(Command::CellPixels(pixels, reply)) => {
                        terminal.set_cell_pixels(pixels); let _ = reply.send(Ok(()));
                    }
                    Some(Command::Inspect(reply)) => { let _ = reply.send(terminal.screen()); }
                }
            }
        }
        // Discard only on declared cancellation/failure. EOF has already been
        // finalized. No attempt to disguise lost pending data as a normal end.
        transport.stop.send_replace(true);
        // The test driver always awaits on cancellable channels, not OS I/O.
        let _ = (&mut transport.task).await;
        current.revision += 1;
        state.send_replace(current.clone());
        (current, terminal.screen())
    });
    (handle, SessionTask(task))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;
    fn size() -> TermSize {
        TermSize::new(32, 8).unwrap()
    }
    async fn parsed(handle: &SessionHandle, expected: u64) {
        let mut state = handle.state.clone();
        timeout(Duration::from_secs(2), async {
            while state.borrow().parsed_bytes < expected {
                state.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
    fn driven_transport(capacity: usize) -> (ChannelTransport, mpsc::Sender<Vec<u8>>) {
        let (output, receiver) = mpsc::channel(capacity);
        let (input, _commands) = mpsc::channel(QUEUE);
        let (stop, mut stopping) = watch::channel(false);
        let task = tokio::spawn(async move {
            let _ = stopping.changed().await;
        });
        (
            ChannelTransport {
                output: receiver,
                input,
                stop,
                task,
            },
            output,
        )
    }
    #[tokio::test]
    async fn orderly_eof_finalizes_utf8_and_keeps_trailing_output() {
        let (transport, output) = driven_transport(1);
        let (handle, task) = spawn_session(size(), 0, transport);
        output.send(b"tail:\xf0\x9f".to_vec()).await.unwrap();
        drop(output);
        let (state, screen) = timeout(Duration::from_secs(2), task.join_with_screen())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.lifecycle, Lifecycle::Ended);
        assert_eq!(state.parsed_bytes, 7);
        assert!(
            screen.cells[0]
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .starts_with("tail:\u{fffd}")
        );
        assert_eq!(handle.inspect().await, Err(SessionError::Closed));
    }
    #[tokio::test]
    async fn oversized_transport_output_is_an_explicit_failure() {
        let (transport, output) = driven_transport(1);
        let (_handle, task) = spawn_session(size(), 0, transport);
        output.send(vec![0; MAX_CHUNK + 1]).await.unwrap();
        let state = timeout(Duration::from_secs(2), task.join())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.lifecycle, Lifecycle::Failed);
        assert_eq!(state.parsed_bytes, 0);
    }
    #[tokio::test]
    async fn sustained_output_does_not_starve_inspection_or_close() {
        let (transport, output) = driven_transport(1);
        let (handle, task) = spawn_session(size(), 0, transport);
        let producer =
            tokio::spawn(async move { while output.send(vec![b'x'; 1024]).await.is_ok() {} });
        parsed(&handle, 1024).await;
        for _ in 0..8 {
            timeout(Duration::from_secs(2), handle.inspect())
                .await
                .unwrap()
                .unwrap();
        }
        handle.close();
        assert_eq!(
            timeout(Duration::from_secs(2), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        timeout(Duration::from_secs(2), producer)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn dropping_transport_aborts_a_full_output_driver() {
        let (transport, _) = EchoTransport::spawn();
        for _ in 0..QUEUE + 1 {
            transport
                .input
                .send(IoCommand::Input(vec![b'x']))
                .await
                .unwrap();
            tokio::task::yield_now().await;
        }
        let input = transport.input.clone();
        drop(transport);
        timeout(Duration::from_secs(2), input.closed())
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn echo_roundtrip_has_one_state_owner_and_no_view() {
        let (transport, observed) = EchoTransport::spawn();
        let (handle, task) = spawn_session(size(), 128, transport);
        let bytes = "abc 中".as_bytes().to_vec();
        handle.input(bytes.clone()).await.unwrap();
        parsed(&handle, bytes.len() as u64).await;
        assert!(
            handle.inspect().await.unwrap().cells[0]
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .starts_with("abc 中")
        );
        assert_eq!(observed.input_bytes(), bytes.len() as u64);
        handle.close();
        assert_eq!(
            timeout(Duration::from_secs(2), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        assert_eq!(handle.input(vec![b'x']).await, Err(SessionError::Closed));
    }
    #[tokio::test]
    async fn dropping_state_subscribers_does_not_terminate_session() {
        let (transport, _) = EchoTransport::spawn();
        let (handle, task) = spawn_session(size(), 0, transport);
        drop(handle.state.clone());
        let transient_view = handle.clone();
        drop(transient_view);
        handle.input(b"hidden-output".to_vec()).await.unwrap();
        parsed(&handle, 13).await;
        assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Running);
        handle.close();
        task.join().await.unwrap();
    }
    #[tokio::test]
    async fn last_handle_abandonment_closes_driver() {
        let (transport, _) = EchoTransport::spawn();
        let (handle, task) = spawn_session(size(), 0, transport);
        let state = handle.state.clone();
        drop(handle);
        assert_eq!(
            timeout(Duration::from_secs(2), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        assert_eq!(state.borrow().lifecycle, Lifecycle::Closed);
    }
    #[tokio::test]
    async fn oversized_input_is_not_accepted() {
        let (transport, observed) = EchoTransport::spawn();
        let (handle, task) = spawn_session(size(), 0, transport);
        assert_eq!(
            handle.input(vec![0; MAX_CHUNK + 1]).await,
            Err(SessionError::ChunkTooLarge)
        );
        assert_eq!(observed.input_bytes(), 0);
        handle.close();
        task.join().await.unwrap();
    }
    #[tokio::test]
    async fn resize_updates_parser_and_metadata() {
        let (transport, _) = EchoTransport::spawn();
        let (handle, task) = spawn_session(size(), 0, transport);
        let new_size = TermSize::new(48, 12).unwrap();
        handle.resize(new_size).await.unwrap();
        let screen = handle.inspect().await.unwrap();
        assert_eq!((screen.cells[0].len(), screen.cells.len()), (48, 12));
        assert_eq!(handle.state.borrow().size, new_size);
        handle.close();
        task.join().await.unwrap();
    }
    #[tokio::test]
    async fn backend_reply_is_upstream_not_echoed_output() {
        let (transport, mut observed) = EchoTransport::spawn();
        let (handle, task) = spawn_session(size(), 0, transport);
        handle.input(b"abc\x1b[6n".to_vec()).await.unwrap();
        timeout(Duration::from_secs(2), observed.last_reply.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.last_reply(), b"\x1b[1;4R");
        assert_eq!(observed.reply_bytes(), 6);
        assert_eq!(handle.state.borrow().parsed_bytes, 7);
        handle.close();
        task.join().await.unwrap();
    }
    #[tokio::test]
    async fn out_of_band_close_works_with_full_command_queue() {
        let (transport, _) = EchoTransport::spawn();
        let (handle, task) = spawn_session(size(), 0, transport);
        // Current-thread runtime means actor cannot consume while this fills.
        for _ in 0..QUEUE {
            let (reply, _) = oneshot::channel();
            handle.enqueue(Command::Input(vec![b'x'], reply)).unwrap();
        }
        let (reply, _) = oneshot::channel();
        assert_eq!(
            handle.enqueue(Command::Input(vec![b'y'], reply)),
            Err(SessionError::Busy)
        );
        handle.close();
        assert_eq!(
            timeout(Duration::from_secs(2), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
    }
}
