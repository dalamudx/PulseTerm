//! Read-only captured colors, NOT theme-application or session-control authority.
use super::*;
#[cfg(test)]
#[path = "registry_palette_tests.rs"]
mod tests;

/// Drawing metadata from an exact currently authorized attachment capture.
/// Retaining this value does not authorize later application or publication.
#[derive(Clone, Debug)]
pub struct CapturedQueryPalette {
    /// Exact binding generation used for admission.
    pub generation: u64,
    /// Capture boundary, not latest state or the last frame ACK.
    pub boundary: SessionState,
    /// Base colors; remote OSC overrides are retained in restore bytes separately.
    pub palette: pulse_term::QueryPalette,
}
impl SessionRegistry {
    /// Validate window/session/generation and read the immutable captured base
    /// under one short metadata lock. Does not query the actor, consume credit,
    /// create another subscription, or authorize asynchronous theme application.
    pub fn captured_query_palette(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<CapturedQueryPalette, RegistryError> {
        let state = self.lock()?;
        Self::entry(&state, owner, id)?;
        let binding = state
            .bindings
            .get(&id)
            .filter(|b| b.generation == generation)
            .ok_or(RegistryError::Stale)?;
        let (boundary, palette) = binding
            .handle
            .bootstrap_palette()
            .ok_or(RegistryError::Stale)?;
        Ok(CapturedQueryPalette {
            generation,
            boundary: boundary.clone(),
            palette: palette.clone(),
        })
    }
}
