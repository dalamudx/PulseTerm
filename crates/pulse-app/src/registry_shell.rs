//! Independently rate-limited advisory metadata; never rendering or authorization evidence.
use super::*;
use std::time::Duration;
use tokio::time::{Instant, timeout};
const MIN_INTERVAL: Duration = Duration::from_millis(500);
const DEADLINE: Duration = Duration::from_millis(250);
impl SessionRegistry {
    /// Drain at most 128 untrusted hints from the current owner/generation's live lease.
    /// Sampling does not ACK, flush, clear risk, or prove that the renderer consumed hints.
    /// One admission per session/500ms, preserved over detach, replacement and transfer.
    /// A successfully sampled old view's late result is discarded after reauthorization;
    /// hints are lossy, not an exactly-once journal. No terminal bytes are returned.
    pub async fn take_shell_markers(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<pulse_term::session::SessionShellMarkers, RegistryError> {
        let (entry, binding) = {
            let state = self.lock()?;
            let entry = Self::entry(&state, owner, id)?;
            let binding = state
                .bindings
                .get(&id)
                .filter(|b| b.generation == generation)
                .ok_or(RegistryError::Stale)?
                .handle
                .clone();
            (entry, binding)
        };
        let mut last = entry
            .shell_markers
            .try_lock()
            .map_err(|_| RegistryError::Busy)?;
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < MIN_INTERVAL) {
            return Err(RegistryError::Busy);
        }
        *last = Some(now);
        let sampled = timeout(DEADLINE, binding.take_shell_markers()).await;
        self.check_binding(owner, id, generation)?;
        Ok(sampled.map_err(|_| RegistryError::Busy)??)
    }
}
#[cfg(test)]
#[path = "registry_shell_tests.rs"]
mod tests;
