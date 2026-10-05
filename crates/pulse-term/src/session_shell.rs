//! Lease-scoped lossy advisory hints; not ordered rendering evidence.
use super::*;
/// An atomic drain of bounded untrusted hints, not a command log or wire checkpoint.
#[derive(Debug, PartialEq, Eq)]
pub struct SessionShellMarkers {
    /// Actor state at sampling; NOT an individual marker's boundary or line coordinate.
    pub sampled_at: SessionState,
    /// Stream-order hints; may precede synchronized rendering and are not restored.
    pub hints: crate::shell_markers::ShellMarkerBatch,
}
/// Actor supervision receipt. Hints are leftover lossy metadata, not trusted exit evidence.
#[derive(Debug)]
pub struct SessionReceipt {
    /// Final actor boundary and declared lifecycle, independent of process status.
    pub state: SessionState,
    /// Diagnostic final projection, not full historical/restoration state.
    pub screen: ObservedScreen,
    /// At most 128 undrained hints; explicit dropped count, no command strings.
    pub shell_markers: crate::shell_markers::ShellMarkerBatch,
}

impl SessionHandle {
    /// Drain advisory hints only for this active lease and noncancelled view.
    /// Queue admission is bounded; callers must impose their own response deadline.
    /// A reply successfully delivered before later revocation may be lost by its caller.
    pub async fn diagnostic_shell_markers(
        &self,
        lease_id: u64,
        cancelled: watch::Receiver<bool>,
    ) -> Result<SessionShellMarkers, SessionError> {
        if *cancelled.borrow() {
            return Err(SessionError::StaleLease);
        }
        let (reply, result) = oneshot::channel();
        self.enqueue(Command::ShellMarkers {
            lease_id,
            cancelled,
            reply,
        })?;
        result.await.map_err(|_| SessionError::Closed)?
    }
}
/// Called without await or feed between drain and delivery, preserving failed replies.
pub(super) fn deliver(
    terminal: &mut TerminalProbe,
    reply: oneshot::Sender<Result<SessionShellMarkers, SessionError>>,
    result: Result<SessionShellMarkers, SessionError>,
) {
    if let Err(Ok(sampled)) = reply.send(result) {
        terminal.shell_markers.restore(sampled.hints);
    }
}

#[cfg(test)]
#[path = "session_shell_eof_tests.rs"]
mod eof_tests;
#[cfg(test)]
#[path = "session_shell_tests.rs"]
mod tests;
