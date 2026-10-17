//! Bounded ownership and supervised teardown, never authentication or forwarding authority.
use super::{PeerError, PinnedTransport};

/// Matches eight configured jumps plus the explicit final entrance.
const MAX_TRANSPORTS: usize = 9;

/// Backend-owned transports ordered from outermost to innermost.
///
/// Only the intended route length is frozen here. The driver must independently bind each
/// target, verify its actual peer and authorize its identity before opening any channel.
/// Drop closes peer evidence but is NOT SDK/native completion; use `begin_close` under
/// the application's exit supervisor. Drop every external channel handle before closing.
#[must_use]
pub struct RouteTransports {
    runtime: tokio::runtime::Handle,
    expected: usize,
    transports: Vec<PinnedTransport>,
    leaf_taken: bool,
}

/// A rejected insertion returns the original owned transport, requiring explicit cleanup.
/// No failure here can silently lose its native-completion observer or claim its exit.
#[must_use]
pub struct RejectedRouteTransport {
    error: PeerError,
    transport: PinnedTransport,
}
impl RejectedRouteTransport {
    /// Retrieve both the static refusal and the original native resource.
    pub fn into_parts(self) -> (PeerError, PinnedTransport) {
        (self.error, self.transport)
    }
}

/// Result waiter for an independently owned cleanup task. Drop never aborts that task.
/// Successful receipt proves SDK join and owned stream exit for every collected transport.
/// Caller must retain its separate route/session reservation until this result is known.
#[must_use]
pub struct RouteTransportCleanup {
    result: tokio::sync::oneshot::Receiver<Result<(), PeerError>>,
}
impl RouteTransportCleanup {
    /// A lost supervisor is an unknown cleanup state, never successful completion.
    pub async fn wait(self) -> Result<(), PeerError> {
        self.result.await.map_err(|_| PeerError::Transport)?
    }
}
impl RouteTransports {
    /// Supply the retained application runtime, including when called outside Tokio context.
    pub fn new(expected: usize, runtime: tokio::runtime::Handle) -> Result<Self, PeerError> {
        if !(1..=MAX_TRANSPORTS).contains(&expected) {
            return Err(PeerError::Transport);
        }
        Ok(Self {
            runtime,
            expected,
            transports: Vec::with_capacity(expected),
            leaf_taken: false,
        })
    }
    /// Collect one live pinned transport, preserving ownership on all refusals.
    /// Pins prove a connection's key only; collection does not authorize credentials.
    pub fn push(&mut self, transport: PinnedTransport) -> Result<(), RejectedRouteTransport> {
        let error = if self.leaf_taken || self.transports.len() >= self.expected {
            Some(PeerError::Transport)
        } else {
            transport.peer().err()
        };
        if let Some(error) = error {
            return Err(RejectedRouteTransport { error, transport });
        }
        self.transports.push(transport);
        Ok(())
    }
    /// Transfer only the explicit final transport after collecting the full route once.
    /// The final actor/sink must close and join it before the remaining intermediates.
    /// This is resource ownership only, not proof of authentication or session registration.
    pub fn take_final(&mut self) -> Result<PinnedTransport, PeerError> {
        if self.leaf_taken || self.transports.len() != self.expected {
            return Err(PeerError::Transport);
        }
        let leaf = self.transports.pop().ok_or(PeerError::Transport)?;
        self.leaf_taken = true;
        Ok(leaf)
    }
    /// Start reverse-order teardown of every collected resource, even an incomplete route.
    /// Continue after a failure so an uncertain inner exit does not strand outer transports.
    /// Await the returned receipt from the root exit supervisor; never infer cleanup from Drop.
    pub fn begin_close(mut self) -> RouteTransportCleanup {
        let (sender, result) = tokio::sync::oneshot::channel();
        let runtime = self.runtime.clone();
        runtime.spawn(async move {
            let mut failure = None;
            while let Some(transport) = self.transports.pop() {
                if let Err(error) = transport.close_and_join().await {
                    failure.get_or_insert(error);
                }
            }
            let outcome = failure.map_or(Ok(()), Err);
            let _sent = sender.send(outcome);
        });
        RouteTransportCleanup { result }
    }
}
