//! Source verification and view confirmation have one backend metadata linearization point.
use super::*;
use pulse_store::settings_source::{
    SettingsSourceError, SettingsSourceHandle, UserSettingsSnapshot,
};
/// Immutable parsed source/view/context receipt; not a host permission or GUI apply capability.
#[derive(Clone)]
pub struct SourceBoundSettingsSnapshot {
    view: RegisteredSettingsSnapshot,
    source: UserSettingsSnapshot,
}
impl SourceBoundSettingsSnapshot {
    /// Non-secret resolved values and per-field provenance only.
    pub fn settings(&self) -> &PublishedSettings {
        self.view.settings()
    }
    /// Semantic source revision; comments keep it, verification identity still changes.
    pub fn user_revision(&self) -> u64 {
        self.source.revision()
    }
}
impl SessionRegistry {
    /// Confirm this candidate only while the exact supplied user verification is current.
    /// Source lock wraps short registry/context metadata confirmation, never DB/file I/O or await.
    /// Scope/host authorization is still the trusted adapter's responsibility.
    pub fn confirm_view_settings_from_source(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        source: &SettingsSourceHandle,
        user: &UserSettingsSnapshot,
        candidate: RegisteredSettingsCandidate,
    ) -> Result<SourceBoundSettingsSnapshot> {
        {
            let state = self.lock()?;
            Self::settings_binding(&state, owner, id, generation)?;
        }
        let view = source.with_current(user, || {
            if candidate.candidate.user_patch() != user.patch() {
                return Err(RegistrySettingsError::SourceMismatch);
            }
            self.confirm_view_settings(owner, id, generation, candidate)
        })??;
        Ok(SourceBoundSettingsSnapshot {
            view,
            source: user.clone(),
        })
    }
    /// Capture non-secret values while source verification and view authorization stay locked.
    /// Fixed lock order: source -> registry -> context. No file/DB I/O, callbacks or await.
    /// Values are valid at capture, not permission to apply later, ACK, or select a host.
    /// DB updates still require trusted invalidation; this is not a cross-file/DB transaction.
    pub fn read_view_settings_from_source(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        source: &SettingsSourceHandle,
        snapshot: &SourceBoundSettingsSnapshot,
    ) -> Result<PublishedSettings> {
        {
            let state = self.lock()?;
            Self::settings_binding(&state, owner, id, generation)?;
        }
        source.with_current(&snapshot.source, || {
            let state = self.lock()?;
            let binding = Self::settings_binding(&state, owner, id, generation)?;
            let view = &snapshot.view;
            if !Weak::ptr_eq(&Arc::downgrade(&self.state), &view.registry)
                || view.owner != owner
                || view.id != id
                || view.generation != generation
            {
                return Err(RegistryError::Forbidden.into());
            }
            let handle = binding
                .settings
                .as_ref()
                .ok_or(RegistrySettingsError::Unconfigured)?
                .handle();
            if !handle.is_current(&view.settings) {
                return Err(RegistryError::Stale.into());
            }
            Ok(view.settings.clone())
        })?
    }
    /// Joint point-in-time source + authorized view/context check; not an atomic DOM application.
    /// Explicit source invalidation makes a previous receipt noncurrent even before reconfiguration.
    pub fn source_view_settings_current(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        source: &SettingsSourceHandle,
        snapshot: &SourceBoundSettingsSnapshot,
    ) -> Result<bool> {
        if !self.view_settings_current(owner, id, generation, &snapshot.view)? {
            return Ok(false);
        }
        match source.with_current(&snapshot.source, || {
            self.view_settings_current(owner, id, generation, &snapshot.view)
        }) {
            Ok(result) => result,
            Err(SettingsSourceError::Stale | SettingsSourceError::Closed) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}
