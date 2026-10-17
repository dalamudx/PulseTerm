//! Theme selection is owned by the exact authorized Binding, not a guessed theme/window identifier.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "registry_theme_tests.rs"]
mod tests;
use super::*;
use crate::theme_context::{
    CancelledThemeRead, PublishedTheme, ThemeCandidate, ThemeContextError, ThemeContextOwner,
    ThemeReadRequest,
};
use pulse_store::theme_io::ThemeIoHandle;
use std::sync::Weak;
/// Static errors do not expose source contents or paths.
#[derive(Debug, thiserror::Error)]
pub enum RegistryThemeError {
    /// Existing exact runtime-window/session/generation authorization failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Current backend theme-context operation failed.
    #[error(transparent)]
    Context(#[from] ThemeContextError),
    /// Source not configured by a trusted backend.
    #[error("view_theme.unconfigured")]
    Unconfigured,
}
type Result<T> = std::result::Result<T, RegistryThemeError>;
impl SessionRegistry {
    fn theme_binding(
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
    /// Backend-only: authorize selected source/profile BEFORE calling; never accept frontend tokens/paths.
    /// Binding owns its context. Reconfiguration cannot reset counters or bypass a pending read slot.
    pub fn configure_view_theme(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        source: ThemeIoHandle,
    ) -> Result<()> {
        let mut s = self.lock()?;
        Self::theme_binding(&s, owner, id, generation)?;
        let b = s.bindings.get_mut(&id).ok_or(RegistryError::Stale)?;
        if let Some(c) = &b.theme {
            c.select(source)?;
        } else {
            b.theme = Some(ThemeContextOwner::new(source)?);
        }
        Ok(())
    }
    /// Trusted source invalidation; not a watcher, file freshness proof or arbitrary frontend RPC.
    pub fn invalidate_view_theme(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<()> {
        let s = self.lock()?;
        Self::theme_binding(&s, owner, id, generation)?
            .theme
            .as_ref()
            .ok_or(RegistryThemeError::Unconfigured)?
            .invalidate()?;
        Ok(())
    }
    /// Authorize before bounded IO admission. No registry lock crosses await.
    pub fn begin_view_theme(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<RegisteredThemeRead> {
        let s = self.lock()?;
        let b = Self::theme_binding(&s, owner, id, generation)?;
        let request = b
            .theme
            .as_ref()
            .ok_or(RegistryThemeError::Unconfigured)?
            .handle()
            .begin_load()?;
        Ok(RegisteredThemeRead {
            registry: Arc::downgrade(&self.state),
            owner,
            id,
            generation,
            request,
        })
    }
    /// Current view + opaque candidate identity + current selection confirm in a short metadata transaction.
    pub fn confirm_view_theme(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        candidate: RegisteredThemeCandidate,
    ) -> Result<RegisteredThemeSnapshot> {
        let s = self.lock()?;
        let b = Self::theme_binding(&s, owner, id, generation)?;
        if !Weak::ptr_eq(&Arc::downgrade(&self.state), &candidate.registry)
            || candidate.owner != owner
            || candidate.id != id
            || candidate.generation != generation
        {
            return Err(RegistryError::Forbidden.into());
        }
        let theme = b
            .theme
            .as_ref()
            .ok_or(RegistryThemeError::Unconfigured)?
            .handle()
            .confirm(candidate.candidate)?;
        Ok(RegisteredThemeSnapshot {
            registry: Arc::downgrade(&self.state),
            owner,
            id,
            generation,
            theme,
        })
    }
    /// Observational same-source last-good may be stale. Not a GUI apply permission.
    pub fn view_theme_last_good(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<Option<RegisteredThemeSnapshot>> {
        let s = self.lock()?;
        let b = Self::theme_binding(&s, owner, id, generation)?;
        let last = b
            .theme
            .as_ref()
            .ok_or(RegistryThemeError::Unconfigured)?
            .handle()
            .last_good()?;
        Ok(last.map(|theme| RegisteredThemeSnapshot {
            registry: Arc::downgrade(&self.state),
            owner,
            id,
            generation,
            theme,
        }))
    }
    /// Instantaneous authorization/context check, not authorization for deferred GUI work.
    pub fn view_theme_current(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        p: &RegisteredThemeSnapshot,
    ) -> Result<bool> {
        let s = self.lock()?;
        let b = Self::theme_binding(&s, owner, id, generation)?;
        if !Weak::ptr_eq(&Arc::downgrade(&self.state), &p.registry)
            || p.owner != owner
            || p.id != id
            || p.generation != generation
        {
            return Ok(false);
        }
        Ok(b.theme
            .as_ref()
            .ok_or(RegistryThemeError::Unconfigured)?
            .handle()
            .is_current(&p.theme))
    }
    /// Register content-free one-way revocation observation for an exact CURRENT authorized receipt.
    /// The observer retains no registry, view, context, file, IO slot or GUI application permission.
    pub fn observe_view_theme_revocation(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
        p: &RegisteredThemeSnapshot,
    ) -> Result<crate::theme_context::ThemeRevocation> {
        let s = self.lock()?;
        let b = Self::theme_binding(&s, owner, id, generation)?;
        if !Weak::ptr_eq(&Arc::downgrade(&self.state), &p.registry)
            || p.owner != owner
            || p.id != id
            || p.generation != generation
        {
            return Err(RegistryError::Forbidden.into());
        }
        let h = b
            .theme
            .as_ref()
            .ok_or(RegistryThemeError::Unconfigured)?
            .handle();
        if !h.is_current(&p.theme) {
            return Err(RegistryError::Stale.into());
        }
        Ok(p.theme.observe_revocation())
    }
    /// Capture a currently authorized immutable model under the registry/context metadata locks.
    /// No IO or await; returned image is not a lease for external or deferred application.
    pub fn read_current_view_theme(
        &self,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<Option<RegisteredThemeSnapshot>> {
        let s = self.lock()?;
        let b = Self::theme_binding(&s, owner, id, generation)?;
        let h = b
            .theme
            .as_ref()
            .ok_or(RegistryThemeError::Unconfigured)?
            .handle();
        let Some(theme) = h.last_good()? else {
            return Ok(None);
        };
        if !h.is_current(&theme) {
            return Ok(None);
        }
        Ok(Some(RegisteredThemeSnapshot {
            registry: Arc::downgrade(&self.state),
            owner,
            id,
            generation,
            theme,
        }))
    }
}
fn verify(
    registry: &Weak<Mutex<State>>,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
) -> Result<()> {
    let s = registry.upgrade().ok_or(RegistryError::Unavailable)?;
    let s = s.lock().map_err(|_| RegistryError::Unavailable)?;
    SessionRegistry::theme_binding(&s, owner, id, generation)?;
    Ok(())
}
/// Opaque read admission for the exact runtime window and active generation.
pub struct RegisteredThemeRead {
    registry: Weak<Mutex<State>>,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    request: ThemeReadRequest,
}
impl RegisteredThemeRead {
    /// Reauthorize before AND after actual read exit, including failed reads. Read is not publication.
    pub async fn ready(self) -> Result<RegisteredThemeCandidate> {
        if let Err(error) = verify(&self.registry, self.owner, self.id, self.generation) {
            let _completion = self.request.cancel().join().await;
            return Err(error);
        }
        let result = self.request.ready().await;
        verify(&self.registry, self.owner, self.id, self.generation)?;
        let candidate = result?;
        Ok(RegisteredThemeCandidate {
            registry: self.registry,
            owner: self.owner,
            id: self.id,
            generation: self.generation,
            candidate,
        })
    }
    /// Suppress delivery; observation retains no authority and cannot abort native IO.
    pub fn cancel(self) -> CancelledThemeRead {
        self.request.cancel()
    }
}
/// Completion is not input, ACK, repair or GUI authority.
pub struct RegisteredThemeCandidate {
    registry: Weak<Mutex<State>>,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    candidate: ThemeCandidate,
}
/// Immutable window/view/selection receipt, not ongoing file freshness.
#[derive(Clone)]
pub struct RegisteredThemeSnapshot {
    registry: Weak<Mutex<State>>,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    theme: PublishedTheme,
}
impl RegisteredThemeSnapshot {
    /// Validated captured document only; caller still needs a current view guard before application.
    pub fn theme(&self) -> &PublishedTheme {
        &self.theme
    }
    /// Terminal base derived from this *confirmed* immutable document, not from
    /// a frontend-selected palette or CSS. This value is observational: neither
    /// retaining it nor calling this getter authorizes a later actor mutation.
    pub fn terminal_palette(&self) -> pulse_term::QueryPalette {
        crate::terminal_palette::query_palette_from_theme(self.theme.document())
    }
}
