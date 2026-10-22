//! Original-page, one-use Agent metadata drafts; discovery and saving never authorize signing.
#[cfg(all(test, unix))]
#[path = "product_agent_identities_tests.rs"]
mod tests;
use super::*;
use pulse_core::ssh_agent_identity::SshAgentIdentity;
use pulse_store::ssh_agent_identities::{AgentIdentityEdit, AgentIdentitySnapshot};
use std::sync::{
    Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct KeyDto {
    pub algorithm: String,
    pub fingerprint: String,
}
struct Capture {
    token: Uuid,
    page: Arc<pulse_app::registry::PageLaunchScope>,
    snapshot: AgentIdentitySnapshot,
    keys: Vec<KeyDto>,
    live: AtomicBool,
    expires: Instant,
    generation: Arc<AtomicU64>,
    expected_generation: u64,
}
struct Slots {
    draft: Option<Arc<Capture>>,
    saving: Option<Weak<Capture>>,
}
pub(super) struct AgentBank {
    slots: Mutex<Slots>,
    generation: Arc<AtomicU64>,
}
impl AgentBank {
    pub(super) fn new() -> Self {
        Self {
            slots: Mutex::new(Slots {
                draft: None,
                saving: None,
            }),
            generation: Arc::new(AtomicU64::new(1)),
        }
    }
    pub(super) fn retire(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut s) = self.slots.lock() {
            if let Some(c) = s.draft.take() {
                c.live.store(false, Ordering::Release);
            }
            if let Some(c) = s.saving.as_ref().and_then(Weak::upgrade) {
                c.live.store(false, Ordering::Release);
            }
        }
    }
    fn publish(
        &self,
        registry: &SessionRegistry,
        page: Arc<pulse_app::registry::PageLaunchScope>,
        snapshot: AgentIdentitySnapshot,
        keys: Vec<KeyDto>,
        expected_generation: u64,
    ) -> Result<CatalogueDto> {
        registry
            .check_launch_page(&page)
            .map_err(|_| "workspace.denied")?;
        let c = Arc::new(Capture {
            token: Uuid::new_v4(),
            page,
            snapshot,
            keys,
            generation: self.generation.clone(),
            expected_generation,
            live: AtomicBool::new(true),
            expires: Instant::now() + Duration::from_secs(300),
        });
        let dto = projection(&c);
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| "workspace.agent_identities_failed")?;
        if !c.current(registry) {
            return Err("workspace.denied");
        }
        if slots.saving.as_ref().and_then(Weak::upgrade).is_some() {
            return Err("workspace.busy");
        }
        if let Some(old) = slots.draft.replace(c) {
            old.live.store(false, Ordering::Release);
        }
        Ok(dto)
    }
    fn take(
        &self,
        registry: &SessionRegistry,
        page: &Arc<pulse_app::registry::PageLaunchScope>,
        id: Uuid,
    ) -> Result<Arc<Capture>> {
        let mut s = self
            .slots
            .lock()
            .map_err(|_| "workspace.agent_identities_failed")?;
        if s.saving.as_ref().and_then(Weak::upgrade).is_some() {
            return Err("workspace.busy");
        }
        let c = s.draft.as_ref().ok_or("workspace.denied")?;
        if c.token != id || !Arc::ptr_eq(page, &c.page) || !c.current(registry) {
            return Err("workspace.denied");
        }
        let c = s.draft.take().ok_or("workspace.denied")?;
        s.saving = Some(Arc::downgrade(&c));
        Ok(c)
    }
    fn cancel(
        &self,
        registry: &SessionRegistry,
        page: &Arc<pulse_app::registry::PageLaunchScope>,
        id: Uuid,
    ) -> Result<()> {
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let s = self
            .slots
            .lock()
            .map_err(|_| "workspace.agent_identities_failed")?;
        let c = s
            .draft
            .as_ref()
            .filter(|c| c.token == id)
            .cloned()
            .or_else(|| {
                s.saving
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .filter(|c| c.token == id)
            })
            .ok_or("workspace.denied")?;
        if !Arc::ptr_eq(page, &c.page) {
            return Err("workspace.denied");
        }
        c.live.store(false, Ordering::Release);
        Ok(())
    }
}
impl Capture {
    fn current(&self, registry: &SessionRegistry) -> bool {
        self.live.load(Ordering::Acquire)
            && self.generation.load(Ordering::Acquire) == self.expected_generation
            && Instant::now() < self.expires
            && registry.check_launch_page(&self.page).is_ok()
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityDto {
    id: String,
    label: String,
    username: String,
    algorithm: String,
    fingerprint: String,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CatalogueDto {
    version: u8,
    token: String,
    revision: String,
    identities: Vec<IdentityDto>,
    keys: Vec<KeyDto>,
}
fn projection(c: &Capture) -> CatalogueDto {
    CatalogueDto {
        version: 1,
        token: c.token.to_string(),
        revision: c.snapshot.revision().to_string(),
        keys: c.keys.clone(),
        identities: c
            .snapshot
            .records()
            .map(|r| {
                let i = r.identity();
                IdentityDto {
                    id: r.id().to_string(),
                    label: i.label().to_owned(),
                    username: i.username().to_owned(),
                    algorithm: i.algorithm().to_owned(),
                    fingerprint: i.fingerprint().to_owned(),
                }
            })
            .collect(),
    }
}
fn canonical(value: &str, version: usize) -> Result<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| "workspace.denied")?;
    if id.get_version_num() != version
        || id.get_variant() != uuid::Variant::RFC4122
        || id.to_string() != value
    {
        return Err("workspace.denied");
    }
    Ok(id)
}
#[derive(serde::Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    Create {
        label: String,
        username: String,
        key: usize,
    },
    Update {
        id: String,
        label: String,
        username: String,
    },
    Remove {
        id: String,
    },
}
impl Change {
    fn edit(self, c: &Capture) -> Result<AgentIdentityEdit> {
        match self {
            Self::Create {
                label,
                username,
                key,
            } => {
                let k = c.keys.get(key).ok_or("workspace.invalid_agent_identity")?;
                Ok(AgentIdentityEdit::Create(
                    SshAgentIdentity::new(&label, &username, &k.algorithm, &k.fingerprint)
                        .map_err(|_| "workspace.invalid_agent_identity")?,
                ))
            }
            Self::Update {
                id,
                label,
                username,
            } => {
                let id = canonical(&id, 7)?;
                let old = c
                    .snapshot
                    .records()
                    .find(|r| r.id() == id)
                    .ok_or("workspace.invalid_agent_identity")?
                    .identity();
                Ok(AgentIdentityEdit::Update(
                    id,
                    SshAgentIdentity::new(&label, &username, old.algorithm(), old.fingerprint())
                        .map_err(|_| "workspace.invalid_agent_identity")?,
                ))
            }
            Self::Remove { id } => Ok(AgentIdentityEdit::Remove(canonical(&id, 7)?)),
        }
    }
}
#[tauri::command]
pub(super) async fn workspace_agent_identities(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    discover: bool,
) -> Result<CatalogueDto> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, discover);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        state.0.agent_identities.retire();
        let generation = state.0.agent_identities.generation.load(Ordering::Acquire);
        let root = state.0.clone();
        let gate = Arc::new(move || {
            root.agent_identities.generation.load(Ordering::Acquire) == generation
        });
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let (snapshot, keys) = state
            .0
            .profiles
            .agent_identities(
                state.0.registry.clone(),
                page.clone(),
                directory,
                discover,
                gate,
            )
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&page, &current)
            || state.0.agent_identities.generation.load(Ordering::Acquire) != generation
        {
            return Err("workspace.denied");
        }
        state
            .0
            .agent_identities
            .publish(&state.0.registry, page, snapshot, keys, generation)
    }
}
#[tauri::command]
pub(super) async fn workspace_agent_identity_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    change: Change,
) -> Result<CatalogueDto> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, token, change);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let capture =
            state
                .0
                .agent_identities
                .take(&state.0.registry, &page, canonical(&token, 4)?)?;
        let edit = change.edit(&capture)?;
        let expected = capture.snapshot.clone();
        let keep = capture.clone();
        let registry = state.0.registry.clone();
        let gate = Arc::new(move || keep.current(&registry));
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let snapshot = state
            .0
            .profiles
            .edit_agent_identity(
                state.0.registry.clone(),
                page.clone(),
                directory,
                expected,
                edit,
                gate,
            )
            .await?;
        if !capture.current(&state.0.registry) {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        capture.live.store(false, Ordering::Release);
        let generation = capture.expected_generation;
        drop(capture);
        state
            .0
            .agent_identities
            .publish(&state.0.registry, page, snapshot, Vec::new(), generation)
    }
}
#[tauri::command]
pub(super) fn workspace_agent_identity_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    if window.label() != state.native_label()? {
        return Err("workspace.denied");
    }
    let url = window.url().map_err(|_| "workspace.denied")?;
    let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
    state
        .0
        .agent_identities
        .cancel(&state.0.registry, &page, canonical(&token, 4)?)
}
