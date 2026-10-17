//! Read-only, generation-scoped comparison. NEVER clears actor divergence hints.
use super::*;
use pulse_term::session::Lifecycle;
use std::time::Duration;
use tokio::time::{Instant, timeout};
const MIN_INTERVAL: Duration = Duration::from_millis(500);
const DEADLINE: Duration = Duration::from_millis(250);

/// Renderer projection after a complete write callback at a known wire boundary.
/// Do not infer revisions from raw Data bytes; only Commit/Checkpoint supplies them.
#[derive(Debug, Clone)]
pub struct RenderedScreen {
    /// Must match every actor field, including dimensions and revision.
    pub boundary: SessionState,
    /// FNV visible digest v1: not an authentication or full-state checksum.
    pub digest: String,
}
/// Sampled evidence, NOT permission to apply a restore or clear actor risk bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleComparison {
    /// Visible projection matched at the requested boundary; risk remains untouched.
    Equal,
    /// Visible projection differed at that boundary; recovery may still be deferred.
    Mismatch,
    /// Actor advanced or reconfigured. This is not evidence of parser divergence.
    Stale,
}
pub use pulse_term::session::RepairAdmission;
impl SessionRegistry {
    /// Atomically compare/capture on the actor and queue an exact screen-only repair.
    /// Queued is admission only; it must render/ACK on this generation's ordered stream.
    /// No stale evidence can authorize a future boundary; risk flags remain unchanged.
    pub async fn request_repair(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        rendered: RenderedScreen,
    ) -> Result<RepairAdmission, RegistryError> {
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
        let mut last = entry.repair.try_lock().map_err(|_| RegistryError::Busy)?;
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < MIN_INTERVAL) {
            return Err(RegistryError::Busy);
        }
        *last = Some(now);
        let admission = timeout(
            DEADLINE,
            binding.request_repair(rendered.boundary, rendered.digest),
        )
        .await;
        self.comparison_entry(owner, id, generation)?;
        Ok(admission.map_err(|_| RegistryError::Busy)??)
    }
}
impl SessionRegistry {
    fn comparison_entry(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<Arc<Entry>, RegistryError> {
        let state = self.lock()?;
        let entry = Self::entry(&state, owner, id)?;
        state
            .bindings
            .get(&id)
            .filter(|b| b.generation == generation)
            .ok_or(RegistryError::Stale)?;
        Ok(entry)
    }
    /// Admit a pure boundary Marker into the current lease. No state is returned
    /// here: only the ordered wire callback can establish the renderer boundary.
    /// Busy/full/sync/resize failures are not success and require bounded retry.
    /// One request/session/500ms, including cancellation and replacement.
    pub async fn request_marker(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<(), RegistryError> {
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
        let mut last = entry.marker.try_lock().map_err(|_| RegistryError::Busy)?;
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < MIN_INTERVAL) {
            return Err(RegistryError::Busy);
        }
        *last = Some(now);
        let admitted = timeout(DEADLINE, binding.request_marker()).await;
        self.comparison_entry(owner, id, generation)?;
        admitted.map_err(|_| RegistryError::Busy)??;
        Ok(())
    }
    /// Confirm the captured bootstrap, not the actor's latest screen. This does not
    /// clear risk hints or prove queued/live Data has been rendered. Shares the
    /// per-session comparison quota, including transfer and cancellation.
    pub fn compare_bootstrap(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        rendered: RenderedScreen,
    ) -> Result<VisibleComparison, RegistryError> {
        self.compare_bootstrap_scope(owner, None, id, generation, rendered)
    }
    /// Atomically validate the document which captured this view before comparing
    /// its immutable projection. A current page cannot promote a legacy binding.
    pub fn compare_bootstrap_page(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
        rendered: RenderedScreen,
    ) -> Result<VisibleComparison, RegistryError> {
        self.compare_bootstrap_scope(page.owner, Some(page), id, generation, rendered)
    }
    /// Single-flight accurate capture confirmation at the authoritative quota
    /// deadline. Never retry or relax MIN_INTERVAL; revalidate the same source
    /// after waiting. Only the comparison admission lock crosses this await.
    pub async fn compare_bootstrap_page_when_ready(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
        rendered: RenderedScreen,
    ) -> Result<VisibleComparison, RegistryError> {
        self.compare_bootstrap_page_when_ready_observed(page, id, generation, rendered, || {})
            .await
    }
    /// Read-only progress callback while holding the existing admission; it is
    /// not a confirmation receipt and cannot upgrade this page or generation.
    pub async fn compare_bootstrap_page_when_ready_observed(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
        rendered: RenderedScreen,
        waiting: impl FnOnce() + Send,
    ) -> Result<VisibleComparison, RegistryError> {
        if rendered.digest.len() != 16
            || !rendered
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(SessionError::InvalidDigest.into());
        }
        let (comparison, mut cancelled) = {
            let state = self.lock()?;
            let binding = self.page_binding(&state, page, id, generation)?;
            (
                Self::entry(&state, page.owner, id)?.comparison.clone(),
                binding.handle.cancellation(),
            )
        };
        let mut last = comparison
            .try_lock_owned()
            .map_err(|_| RegistryError::Busy)?;
        if *cancelled.borrow() {
            return Err(RegistryError::Stale);
        }
        if let Some(at) = *last {
            if at + MIN_INTERVAL > Instant::now() {
                waiting();
            }
            tokio::select! { biased;
                _ = cancelled.changed() => return Err(RegistryError::Stale),
                _ = tokio::time::sleep_until(at + MIN_INTERVAL) => {}
            }
        }
        let state = self.lock()?;
        let binding = self.page_binding(&state, page, id, generation)?;
        *last = Some(Instant::now());
        let Some((boundary, digest)) = binding.handle.bootstrap_projection() else {
            return Ok(VisibleComparison::Stale);
        };
        if boundary != &rendered.boundary {
            return Ok(VisibleComparison::Stale);
        }
        Ok(if digest == rendered.digest {
            VisibleComparison::Equal
        } else {
            VisibleComparison::Mismatch
        })
    }
    fn compare_bootstrap_scope(
        &self,
        owner: WindowToken,
        page: Option<&PageLaunchScope>,
        id: SessionId,
        generation: u64,
        rendered: RenderedScreen,
    ) -> Result<VisibleComparison, RegistryError> {
        let state = self.lock()?;
        let entry = Self::entry(&state, owner, id)?;
        let binding = if let Some(page) = page {
            self.page_binding(&state, page, id, generation)?
        } else {
            state
                .bindings
                .get(&id)
                .filter(|b| b.generation == generation)
                .ok_or(RegistryError::Stale)?
        };
        if rendered.digest.len() != 16
            || !rendered
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(SessionError::InvalidDigest.into());
        }
        let mut last = entry
            .comparison
            .try_lock()
            .map_err(|_| RegistryError::Busy)?;
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < MIN_INTERVAL) {
            return Err(RegistryError::Busy);
        }
        *last = Some(now);
        let Some((boundary, digest)) = binding.handle.bootstrap_projection() else {
            return Ok(VisibleComparison::Stale);
        };
        if boundary != &rendered.boundary {
            return Ok(VisibleComparison::Stale);
        }
        Ok(if digest == rendered.digest {
            VisibleComparison::Equal
        } else {
            VisibleComparison::Mismatch
        })
    }

    /// At most one comparison per session and one admission per 500ms, including
    /// cancelled/failed calls and binding replacements. Foreign/stale requests
    /// cannot consume this allowance. No terminal or registry metadata mutex crosses
    /// await; only the per-session comparison admission guard is held.
    /// A 250ms deadline bounds an unresponsive actor. Comparison is read-only so a
    /// revoked in-flight request cannot clear hints in a replacement generation.
    pub async fn compare_visible(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        rendered: RenderedScreen,
    ) -> Result<VisibleComparison, RegistryError> {
        let entry = self.comparison_entry(owner, id, generation)?;
        if rendered.digest.len() != 16
            || !rendered
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(SessionError::InvalidDigest.into());
        }
        let mut last = entry
            .comparison
            .try_lock()
            .map_err(|_| RegistryError::Busy)?;
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < MIN_INTERVAL) {
            return Err(RegistryError::Busy);
        }
        *last = Some(now);
        // Cheap rejection avoids hashing an already-obsolete or finished screen.
        if rendered.boundary.lifecycle != Lifecycle::Running
            || *entry.handle.state.borrow() != rendered.boundary
        {
            return Ok(VisibleComparison::Stale);
        }
        let captured = timeout(DEADLINE, entry.handle.diagnostic_digest()).await;
        // Revocation/replacement wins even if the actor failed at the same time.
        self.comparison_entry(owner, id, generation)?;
        let captured = captured.map_err(|_| RegistryError::Busy)??;
        if captured.boundary != rendered.boundary
            || *entry.handle.state.borrow() != captured.boundary
        {
            return Ok(VisibleComparison::Stale);
        }
        Ok(if captured.digest == rendered.digest {
            VisibleComparison::Equal
        } else {
            VisibleComparison::Mismatch
        })
    }
}
#[cfg(test)]
#[path = "registry_bootstrap_tests.rs"]
mod bootstrap_tests;
#[cfg(test)]
#[path = "registry_comparison_tests.rs"]
mod tests;
