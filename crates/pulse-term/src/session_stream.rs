//! Atomic diagnostic snapshot/live lease. No window IDs or renderer objects.
use super::{Lifecycle, SessionBootstrap, SessionRestoreError, SessionState, TerminalProbe};
use tokio::sync::{mpsc, watch};
/// A bounded post-boundary stream, not a production window/permission binding.
/// Keep the receiver while loading bootstrap. Dropping it releases pressure without
/// closing the session. Only one current diagnostic subscriber is supported.
#[derive(Debug)]
pub struct SessionStream {
    /// Monotonic actor serial, NOT an authenticated wire token. Zero denotes a
    /// frozen EOF stream with no actor lease or control permissions.
    pub lease_id: u64,
    /// History and screen captured before any queued Data, on the same actor turn.
    pub bootstrap: SessionBootstrap,
    /// Eight records: Data <=64 KiB, plus at most one <=1 MiB repair per lease
    /// across the queue/driver until Commit ACK. Excludes allocator overhead.
    pub output: mpsc::Receiver<StreamRecord>,
    /// Non-lossy termination reason; must cancel the old renderer generation.
    pub status: watch::Receiver<StreamStatus>,
}
/// All records are ordered. Fences do not consume/re-encode output bytes.
#[derive(Debug)]
pub enum StreamRecord {
    /// Normalized display bytes parsed by this actor, source endpoint in boundary.
    Data {
        /// Normalized UTF-8 display offset before this chunk.
        start: u64,
        /// Display bytes, never drop-on-full; source bytes may have different length.
        bytes: Vec<u8>,
        /// Parsed/control boundary AFTER the chunk.
        boundary: SessionState,
    },
    /// Sync deadline changed backend state without receiving output. A wire driver
    /// must propagate this ordered checkpoint, not manufacture VT or a byte offset.
    Fence(SessionState),
    /// Read-only ordered boundary. Never flushes sync or feeds parser bytes.
    Marker(SessionState),
    /// Exact screen-only replay captured at this output boundary; not PTY Data.
    Repair {
        /// Capture boundary, not an output byte offset increase.
        boundary: SessionState,
        /// Exact bounded synchronized replay including pending parser tail.
        bytes: Vec<u8>,
        /// Single repair per lease, held through the final wire/render ACK.
        hold: Option<tokio::sync::OwnedSemaphorePermit>,
    },
}
/// Lease outcome, independent of its bounded output queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEnd {
    /// Stream still belongs to this subscriber.
    Open,
    /// A newer successful subscription replaced this lease.
    Replaced,
    /// Geometry/query controls changed: obtain a fresh generation.
    Reconfigured,
    /// Receiver was dropped or explicitly closed.
    Detached,
    /// Session teardown, including genuine EOF or explicit failure.
    Session(Lifecycle),
}
/// Final state is attached to invalidation; stale queued data is never a new epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamStatus {
    /// Lease reason, NOT a write/ACK receipt.
    pub end: StreamEnd,
    /// Actor serialization/termination point.
    pub boundary: SessionState,
}
pub(super) struct Lease {
    pub id: u64,
    pub repair: std::sync::Arc<tokio::sync::Semaphore>,
    pub output: mpsc::Sender<StreamRecord>,
    pub status: watch::Sender<StreamStatus>,
}
pub(super) fn create(lease_id: u64, bootstrap: SessionBootstrap) -> (Lease, SessionStream) {
    let (output, receiver) = mpsc::channel(super::QUEUE);
    let (status, watch) = watch::channel(StreamStatus {
        end: StreamEnd::Open,
        boundary: bootstrap.boundary.clone(),
    });
    (
        Lease {
            id: lease_id,
            repair: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            output,
            status,
        },
        SessionStream {
            lease_id,
            bootstrap,
            output: receiver,
            status: watch,
        },
    )
}
/// Frozen EOF replay: no actor lease or control permission; the application still
/// supplies a new authenticated generation and bounded ACK supervisor per view.
pub(super) fn completed(bootstrap: SessionBootstrap) -> SessionStream {
    let (tx, output) = mpsc::channel(1);
    drop(tx);
    let (_, status) = watch::channel(StreamStatus {
        end: StreamEnd::Session(Lifecycle::Ended),
        boundary: bootstrap.boundary.clone(),
    });
    SessionStream {
        lease_id: 0,
        bootstrap,
        output,
        status,
    }
}
pub(super) fn invalidate(
    lease: &mut Option<Lease>,
    permit: &mut Option<mpsc::OwnedPermit<StreamRecord>>,
    end: StreamEnd,
    boundary: &SessionState,
) {
    *permit = None;
    if let Some(old) = lease.take() {
        old.status.send_replace(StreamStatus {
            end,
            boundary: boundary.clone(),
        });
    }
}
pub(super) fn prepare(
    terminal: &TerminalProbe,
    boundary: &SessionState,
    rows: usize,
    history_budget: usize,
    restore_budget: usize,
) -> Result<SessionBootstrap, SessionRestoreError> {
    let restore = terminal
        .restore_screen_bounded(restore_budget)
        .map_err(SessionRestoreError::Gap)?;
    if !restore.approximations.is_empty() {
        return Err(SessionRestoreError::Approximate(restore.approximations));
    }
    let history = terminal
        .recent_history_bounded(rows, history_budget)
        .map_err(SessionRestoreError::Gap)?;
    Ok(SessionBootstrap {
        boundary: boundary.clone(),
        digest: terminal.screen_digest(),
        palette: restore.palette,
        history,
        restore: restore.bytes,
    })
}
