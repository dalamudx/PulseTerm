//! Rust-only publication state. No window/host authority, watcher or UI event wiring.
//! Independent user and SQLite snapshots are not a distributed atomic capture.
#[cfg(test)]
#[path = "settings_context_tests.rs"]
mod tests;
use pulse_store::{
    StateStore, StoreError,
    settings::{ResolvedSettings, SettingsPatch},
    settings_scope::SettingsScope,
};
use std::sync::{Arc, Mutex, Weak};
/// Stable errors do not echo settings text, paths or entity identifiers.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingsContextError {
    /// Lifecycle owner is closed or no longer exists.
    #[error("settings_context.closed")]
    Closed,
    /// Existing capture retains its context slot until completion/drop.
    #[error("settings_context.busy")]
    Busy,
    /// Context invalidated after admission; not an invalid-settings diagnosis.
    #[error("settings_context.stale")]
    Stale,
    /// Candidate belongs to another controller, even if context IDs are identical.
    #[error("settings_context.foreign_candidate")]
    Foreign,
    /// Misclassified/nil scope or invalid sparse user patch.
    #[error("settings_context.invalid_input")]
    Invalid,
    /// Counters fail closed rather than wrap and revive an old token.
    #[error("settings_context.counter_exhausted")]
    Exhausted,
    /// Mutex poisoning fails closed.
    #[error("settings_context.state_failed")]
    State,
    /// Bounded store request failed; no implicit retry.
    #[error("settings_context.store: {0}")]
    Store(#[from] StoreError),
}
type Result<T> = std::result::Result<T, SettingsContextError>;
/// Optional group and host selection. Not a membership or host existence proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsContextTarget {
    group: Option<SettingsScope>,
    host: Option<SettingsScope>,
}
impl SettingsContextTarget {
    /// Enforce namespace positions and non-nil identities before state mutation.
    pub fn new(group: Option<SettingsScope>, host: Option<SettingsScope>) -> Result<Self> {
        if !matches!(group, None | Some(SettingsScope::Group(_)))
            || !matches!(host, None | Some(SettingsScope::Host(_)))
        {
            return Err(SettingsContextError::Invalid);
        }
        for scope in [group, host].into_iter().flatten() {
            let id = match scope {
                SettingsScope::Group(id) | SettingsScope::Host(id) => id,
            };
            if id.is_nil() {
                return Err(SettingsContextError::Invalid);
            }
        }
        Ok(Self { group, host })
    }
}
struct State {
    target: SettingsContextTarget,
    user: SettingsPatch,
    epoch: u64,
    next_token: u64,
    pending: Option<u64>,
    closed: bool,
    published: Option<PublishedSettings>,
}
impl State {
    fn advance(&mut self) -> Result<()> {
        let Some(epoch) = self.epoch.checked_add(1) else {
            self.closed = true;
            self.published = None;
            return Err(SettingsContextError::Exhausted);
        };
        self.epoch = epoch;
        Ok(())
    }
}
/// Non-clone lifecycle authority, created only by trusted backend composition.
/// Dropping/closing it never closes the shared StateStore or terminal session.
pub struct SettingsContextOwner {
    state: Arc<Mutex<State>>,
}
impl SettingsContextOwner {
    /// No file reads or SQLite access; user layer must be an explicitly chosen backend patch.
    pub fn new(target: SettingsContextTarget, user: SettingsPatch) -> Result<Self> {
        user.validate().map_err(|_| SettingsContextError::Invalid)?;
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                target,
                user,
                epoch: 1,
                next_token: 0,
                pending: None,
                closed: false,
                published: None,
            })),
        })
    }
    /// Weak handle cannot keep a destroyed owner alive.
    pub fn handle(&self) -> SettingsContextHandle {
        SettingsContextHandle {
            state: Arc::downgrade(&self.state),
        }
    }
    /// Trusted selection/user change revokes old captures but does not free their slot early.
    /// Different targets clear old-host values; same-target refresh retains last-good values.
    pub fn select(&self, target: SettingsContextTarget, user: SettingsPatch) -> Result<()> {
        user.validate().map_err(|_| SettingsContextError::Invalid)?;
        let mut state = self.state.lock().map_err(|_| SettingsContextError::State)?;
        if state.closed {
            return Err(SettingsContextError::Closed);
        }
        if state.target == target && state.user == user {
            return Ok(());
        }
        state.advance()?;
        if state.target != target {
            state.published = None;
        }
        state.target = target;
        state.user = user;
        Ok(())
    }
    /// Explicit trusted data-change signal. No automatic file/database freshness observation.
    pub fn invalidate(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| SettingsContextError::State)?;
        if state.closed {
            return Err(SettingsContextError::Closed);
        }
        state.advance()
    }
    /// Revokes publication now; outstanding captures retain their slot until actual return/drop.
    pub fn close(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| SettingsContextError::State)?;
        state.closed = true;
        state.published = None;
        Ok(())
    }
}
impl Drop for SettingsContextOwner {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
/// Backend weak observation/capture handle. Not serializable or a UI permission.
#[derive(Clone)]
pub struct SettingsContextHandle {
    state: Weak<Mutex<State>>,
}
impl SettingsContextHandle {
    /// Take one context slot before issuing the bounded store read.
    pub fn begin(&self) -> Result<SettingsCapture> {
        let owner = self.state.upgrade().ok_or(SettingsContextError::Closed)?;
        let mut state = owner.lock().map_err(|_| SettingsContextError::State)?;
        if state.closed {
            return Err(SettingsContextError::Closed);
        }
        if state.pending.is_some() {
            return Err(SettingsContextError::Busy);
        }
        let Some(token) = state.next_token.checked_add(1) else {
            state.closed = true;
            state.published = None;
            return Err(SettingsContextError::Exhausted);
        };
        state.next_token = token;
        state.pending = Some(token);
        Ok(SettingsCapture {
            owner: self.state.clone(),
            token,
            epoch: state.epoch,
            target: state.target,
            user: state.user.clone(),
        })
    }
    /// Atomically publish only this controller's current completed capture.
    pub fn confirm(&self, candidate: SettingsCandidate) -> Result<PublishedSettings> {
        if !Weak::ptr_eq(&self.state, &candidate.capture.owner) {
            return Err(SettingsContextError::Foreign);
        }
        let owner = self.state.upgrade().ok_or(SettingsContextError::Closed)?;
        let mut state = owner.lock().map_err(|_| SettingsContextError::State)?;
        candidate.capture.check_locked(&state)?;
        let published = PublishedSettings {
            owner: self.state.clone(),
            epoch: state.epoch,
            token: candidate.capture.token,
            target: state.target,
            resolved: candidate.resolved,
            group_revision: candidate.group_revision,
            host_revision: candidate.host_revision,
        };
        state.pending = None;
        state.published = Some(published.clone());
        drop(state);
        Ok(published)
    }
    /// Retained same-context values may be stale; callers must check is_current before application.
    pub fn last_good(&self) -> Result<Option<PublishedSettings>> {
        let owner = self.state.upgrade().ok_or(SettingsContextError::Closed)?;
        let state = owner.lock().map_err(|_| SettingsContextError::State)?;
        if state.closed {
            return Err(SettingsContextError::Closed);
        }
        Ok(state.published.clone())
    }
    /// Point-in-time stale-result check, NOT an atomic external UI/application authorization.
    pub fn is_current(&self, snapshot: &PublishedSettings) -> bool {
        if !Weak::ptr_eq(&self.state, &snapshot.owner) {
            return false;
        }
        let Some(owner) = self.state.upgrade() else {
            return false;
        };
        let Ok(state) = owner.lock() else {
            return false;
        };
        !state.closed
            && state.epoch == snapshot.epoch
            && state.target == snapshot.target
            && state
                .published
                .as_ref()
                .is_some_and(|p| p.token == snapshot.token)
    }
}
/// Owned capture ticket, with no public constructor, Clone or serializer.
/// ```compile_fail
/// use pulse_app::settings_context::SettingsCapture;
/// fn duplicate(capture: SettingsCapture) { let _ = capture.clone(); }
/// ```
/// Dropping a cancelled future discards its reply; an executing store transaction still runs
/// under the store's single worker/16-request quota. No new blocking task is spawned here.
pub struct SettingsCapture {
    owner: Weak<Mutex<State>>,
    token: u64,
    epoch: u64,
    target: SettingsContextTarget,
    user: SettingsPatch,
}
impl SettingsCapture {
    fn check_locked(&self, state: &State) -> Result<()> {
        if state.closed {
            return Err(SettingsContextError::Closed);
        }
        if state.epoch != self.epoch
            || state.target != self.target
            || state.pending != Some(self.token)
        {
            return Err(SettingsContextError::Stale);
        }
        Ok(())
    }
    fn check(&self) -> Result<()> {
        let owner = self.owner.upgrade().ok_or(SettingsContextError::Closed)?;
        let state = owner.lock().map_err(|_| SettingsContextError::State)?;
        self.check_locked(&state)
    }
    /// Read consistent group/host DB layers, resolve owned user layer, and return a candidate.
    /// Result is not enabled merely by read completion. Store changes require explicit invalidate.
    pub async fn read(self, store: &StateStore) -> Result<SettingsCandidate> {
        self.check()?;
        let group = self.target.group.and_then(|s| {
            if let SettingsScope::Group(id) = s {
                Some(id)
            } else {
                None
            }
        });
        let host = self.target.host.and_then(|s| {
            if let SettingsScope::Host(id) = s {
                Some(id)
            } else {
                None
            }
        });
        let layers = store.load_settings_layers(group, host).await?;
        self.check()?;
        let resolved = layers
            .resolve(Some(&self.user))
            .map_err(|_| SettingsContextError::Invalid)?;
        let group_revision = layers.group().map(|s| s.revision());
        let host_revision = layers.host().map(|s| s.revision());
        Ok(SettingsCandidate {
            capture: self,
            resolved,
            group_revision,
            host_revision,
        })
    }
}
impl Drop for SettingsCapture {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade()
            && let Ok(mut state) = owner.lock()
            && state.pending == Some(self.token)
        {
            state.pending = None;
        }
    }
}
/// Opaque read result; discard clears its slot without publication.
pub struct SettingsCandidate {
    capture: SettingsCapture,
    resolved: ResolvedSettings,
    group_revision: Option<u64>,
    host_revision: Option<u64>,
}
/// Immutable backend snapshot. Never serialized into an authorization payload.
/// ```compile_fail
/// use pulse_app::settings_context::PublishedSettings;
/// fn forge(snapshot: &mut PublishedSettings) { snapshot.epoch = 0; }
/// ```
#[derive(Clone)]
pub struct PublishedSettings {
    owner: Weak<Mutex<State>>,
    epoch: u64,
    token: u64,
    target: SettingsContextTarget,
    resolved: ResolvedSettings,
    group_revision: Option<u64>,
    host_revision: Option<u64>,
}
impl PublishedSettings {
    /// Context generation, not a database/global freshness claim.
    pub fn generation(&self) -> u64 {
        self.epoch
    }
    /// Non-secret resolved values and per-field provenance.
    pub fn settings(&self) -> &ResolvedSettings {
        &self.resolved
    }
    /// Exact DB capture revisions; user file and DB are separate inputs.
    pub fn database_revisions(&self) -> (Option<u64>, Option<u64>) {
        (self.group_revision, self.host_revision)
    }
}
