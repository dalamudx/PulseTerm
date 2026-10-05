//! Settings controller lifetime is owned by the exact authorized terminal Binding.
#[cfg(test)]
#[path = "registry_settings_tests.rs"]
mod tests;
use super::*;
use crate::settings_context::{
    PublishedSettings, SettingsCandidate, SettingsCapture, SettingsContextError,
    SettingsContextOwner, SettingsContextTarget,
};
use pulse_store::{StateStore, settings::SettingsPatch};
use std::sync::Weak;
/// Static boundary errors, without terminal/settings contents or raw paths.
#[derive(Debug, thiserror::Error)]
pub enum RegistrySettingsError {
    /// Existing exact runtime-window/session/binding authorization failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Context lifetime, admission, validation or bounded store read failed.
    #[error(transparent)]
    Context(#[from] SettingsContextError),
    /// Trusted adapter has not configured this binding; do not use old-generation defaults.
    #[error("view_settings.unconfigured")]
    Unconfigured,
}
type Result<T> = std::result::Result<T, RegistrySettingsError>;
impl SessionRegistry {
    fn settings_binding(
        state: &State,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<&Binding> {
        Self::entry(state, owner, id)?;
        state
            .bindings
            .get(&id)
            .filter(|b| b.generation == generation)
            .ok_or_else(|| RegistryError::Stale.into())
    }
    /// Backend-only: resolve profile/host permissions BEFORE constructing target.
    /// Reconfiguration reuses the controller and cannot reset its pending slot or counters.
    /// Never accept WindowToken or target scope IDs directly from frontend input.
    pub fn configure_view_settings(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        target: SettingsContextTarget,
        user: SettingsPatch,
    ) -> Result<()> {
        let mut state = self.lock()?;
        Self::settings_binding(&state, owner, id, generation)?;
        let binding = state.bindings.get_mut(&id).ok_or(RegistryError::Stale)?;
        if let Some(controller) = &binding.settings {
            controller.select(target, user)?;
        } else {
            binding.settings = Some(SettingsContextOwner::new(target, user)?);
        }
        Ok(())
    }
    /// Explicit trusted data signal. Not a file watcher or frontend arbitrary invalidation RPC.
    pub fn invalidate_view_settings(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<()> {
        let state = self.lock()?;
        let binding = Self::settings_binding(&state, owner, id, generation)?;
        binding
            .settings
            .as_ref()
            .ok_or(RegistrySettingsError::Unconfigured)?
            .invalidate()?;
        Ok(())
    }
    /// Reserve the current view context before issuing any database request; no registry lock crosses await.
    pub fn begin_view_settings(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<RegisteredSettingsCapture> {
        let state = self.lock()?;
        let binding = Self::settings_binding(&state, owner, id, generation)?;
        let context = binding
            .settings
            .as_ref()
            .ok_or(RegistrySettingsError::Unconfigured)?
            .handle();
        let capture = context.begin()?;
        Ok(RegisteredSettingsCapture {
            registry: Arc::downgrade(&self.state),
            owner,
            id,
            generation,
            capture,
        })
    }
    /// Atomically reauthorize the same window/session/view and confirm this controller's candidate.
    /// Wrong binding/candidate cannot enable settings or consume another context's pending slot.
    pub fn confirm_view_settings(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        candidate: RegisteredSettingsCandidate,
    ) -> Result<RegisteredSettingsSnapshot> {
        let state = self.lock()?;
        let binding = Self::settings_binding(&state, owner, id, generation)?;
        if !Weak::ptr_eq(&Arc::downgrade(&self.state), &candidate.registry)
            || candidate.owner != owner
            || candidate.id != id
            || candidate.generation != generation
        {
            return Err(RegistryError::Forbidden.into());
        }
        let settings = binding
            .settings
            .as_ref()
            .ok_or(RegistrySettingsError::Unconfigured)?
            .handle()
            .confirm(candidate.candidate)?;
        Ok(RegisteredSettingsSnapshot {
            registry: Arc::downgrade(&self.state),
            owner,
            id,
            generation,
            settings,
        })
    }
    /// Authorized last-good observation, NOT permission to apply a stale snapshot or ACK output.
    pub fn last_view_settings(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<Option<RegisteredSettingsSnapshot>> {
        let state = self.lock()?;
        let binding = Self::settings_binding(&state, owner, id, generation)?;
        let last = binding
            .settings
            .as_ref()
            .ok_or(RegistrySettingsError::Unconfigured)?
            .handle()
            .last_good()?;
        Ok(last.map(|settings| RegisteredSettingsSnapshot {
            registry: Arc::downgrade(&self.state),
            owner,
            id,
            generation,
            settings,
        }))
    }
    /// Point-in-time view and context check. Frontend application still needs its active-view guard.
    pub fn view_settings_current(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        snapshot: &RegisteredSettingsSnapshot,
    ) -> Result<bool> {
        let state = self.lock()?;
        let binding = Self::settings_binding(&state, owner, id, generation)?;
        if !Weak::ptr_eq(&Arc::downgrade(&self.state), &snapshot.registry)
            || snapshot.owner != owner
            || snapshot.id != id
            || snapshot.generation != generation
        {
            return Ok(false);
        }
        Ok(binding
            .settings
            .as_ref()
            .ok_or(RegistrySettingsError::Unconfigured)?
            .handle()
            .is_current(&snapshot.settings))
    }
}
fn verify(
    registry: &Weak<Mutex<State>>,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
) -> Result<()> {
    let state = registry.upgrade().ok_or(RegistryError::Unavailable)?;
    let state = state.lock().map_err(|_| RegistryError::Unavailable)?;
    SessionRegistry::settings_binding(&state, owner, id, generation)?;
    Ok(())
}
/// Non-clone, non-serializable authorized read ticket. No raw source/context constructor.
pub struct RegisteredSettingsCapture {
    registry: Weak<Mutex<State>>,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    capture: SettingsCapture,
}
impl RegisteredSettingsCapture {
    /// Read consistent DB layers through the shared bounded worker. Reauthorize before/after await.
    /// Dropping a future cancels its response, not a store transaction already executing.
    pub async fn read(self, store: &StateStore) -> Result<RegisteredSettingsCandidate> {
        verify(&self.registry, self.owner, self.id, self.generation)?;
        let candidate = self.capture.read(store).await?;
        verify(&self.registry, self.owner, self.id, self.generation)?;
        Ok(RegisteredSettingsCandidate {
            registry: self.registry,
            owner: self.owner,
            id: self.id,
            generation: self.generation,
            candidate,
        })
    }
}
/// Read completion is not publication or permission to send input, repair or ACK.
pub struct RegisteredSettingsCandidate {
    registry: Weak<Mutex<State>>,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    candidate: SettingsCandidate,
}
/// Immutable Rust-only receipt for one exact runtime-window/view/context incarnation.
#[derive(Clone)]
pub struct RegisteredSettingsSnapshot {
    registry: Weak<Mutex<State>>,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    settings: PublishedSettings,
}
impl RegisteredSettingsSnapshot {
    /// Non-secret values/source metadata only; no host selection or authorization payload.
    pub fn settings(&self) -> &PublishedSettings {
        &self.settings
    }
}
