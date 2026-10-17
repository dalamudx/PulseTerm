//! One current-page temporary target, no credential-use or process authority.
#[cfg(all(test, unix, feature = "workspace-ssh"))]
#[path = "product_quick_targets_tests.rs"]
mod tests;
use super::State;
use super::{Result, Workspace, session};
use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;
#[cfg(all(unix, feature = "workspace-ssh"))]
use {
    pulse_app::registry::{PageLaunchScope, SessionRegistry},
    pulse_store::ssh_target_source::TemporarySshTarget,
    std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    },
    tauri::Manager,
    uuid::Uuid,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetArgs {
    label: String,
    hostname: String,
    port: u16,
    username: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TargetDto {
    version: u8,
    token: String,
    target: EndpointDto,
}
#[derive(Serialize)]
pub(super) struct EndpointDto {
    id: String,
    label: String,
    host: String,
    port: u16,
    username: String,
}
#[cfg(all(unix, feature = "workspace-ssh"))]
struct Entry {
    registered: Option<pulse_app::registry::SessionId>,
    saving: bool,
    token: Uuid,
    page: Arc<PageLaunchScope>,
    target: TemporarySshTarget,
    expires: Instant,
}
#[cfg(all(unix, feature = "workspace-ssh"))]
pub(super) struct QuickTargets {
    entry: Mutex<Option<Entry>>,
}
#[cfg(all(unix, feature = "workspace-ssh"))]
impl QuickTargets {
    pub(super) fn new() -> Self {
        Self {
            entry: Mutex::new(None),
        }
    }
    pub(super) fn retire(&self) {
        let mut slot = self
            .entry
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(entry) = slot.take() {
            entry.target.revoke();
        }
    }
    pub(super) fn publish(
        &self,
        registry: &SessionRegistry,
        page: Arc<PageLaunchScope>,
        target: TemporarySshTarget,
    ) -> Result<Uuid> {
        let mut slot = self.entry.lock().map_err(|_| "workspace.quick_failed")?;
        registry
            .check_launch_page(&page)
            .map_err(|_| "workspace.denied")?;
        if let Some(previous) = slot.take() {
            previous.target.revoke();
        }
        let token = Uuid::new_v4();
        *slot = Some(Entry {
            registered: None,
            saving: false,
            token,
            page,
            target,
            expires: Instant::now() + Duration::from_secs(300),
        });
        Ok(token)
    }
    pub(super) fn resolve(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
    ) -> Result<Option<TemporarySshTarget>> {
        let slot = self.entry.lock().map_err(|_| "workspace.quick_failed")?;
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let Some(entry) = slot.as_ref().filter(|entry| entry.target.id() == id) else {
            return Ok(None);
        };
        if !Arc::ptr_eq(&entry.page, page) || Instant::now() >= entry.expires {
            return Err("workspace.denied");
        }
        Ok(Some(entry.target.clone()))
    }
    /// Only the original startup command calls this after Root adoption returns SessionId.
    pub(super) fn registered(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        host: Uuid,
        session: pulse_app::registry::SessionId,
    ) -> Result<()> {
        let mut slot = self.entry.lock().map_err(|_| "workspace.quick_failed")?;
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let entry = slot.as_mut().ok_or("workspace.denied")?;
        if entry.target.id() != host
            || !Arc::ptr_eq(&entry.page, page)
            || Instant::now() >= entry.expires
            || !entry.target.is_current()
            || entry.registered.is_some()
        {
            return Err("workspace.denied");
        }
        entry.registered = Some(session);
        Ok(())
    }
    pub(super) fn claim_save(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        token: Uuid,
    ) -> Result<TemporarySshTarget> {
        let mut slot = self.entry.lock().map_err(|_| "workspace.quick_failed")?;
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let entry = slot.as_mut().ok_or("workspace.denied")?;
        if entry.token != token
            || !Arc::ptr_eq(&entry.page, page)
            || Instant::now() >= entry.expires
            || !entry.target.is_current()
            || entry.registered.is_none()
            || entry.saving
        {
            return Err("workspace.denied");
        }
        entry.saving = true;
        Ok(entry.target.clone())
    }

    pub(super) fn cancel(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        token: Uuid,
    ) -> Result<()> {
        let mut slot = self.entry.lock().map_err(|_| "workspace.quick_failed")?;
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let entry = slot.as_ref().ok_or("workspace.denied")?;
        if entry.token != token || !Arc::ptr_eq(&entry.page, page) {
            return Err("workspace.denied");
        }
        if let Some(entry) = slot.take() {
            entry.target.revoke();
        }
        Ok(())
    }
}
#[cfg(all(unix, feature = "workspace-ssh"))]
impl Drop for QuickTargets {
    fn drop(&mut self) {
        self.retire();
    }
}

#[tauri::command]
pub(super) async fn workspace_quick_target_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    value: TargetArgs,
) -> Result<TargetDto> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(unix, feature = "workspace-ssh")))]
    {
        let _ = (page, value);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(unix, feature = "workspace-ssh"))]
    {
        let endpoint = pulse_core::ssh_endpoint::SshEndpoint::new(
            &value.label,
            &value.hostname,
            value.port,
            &value.username,
        )
        .map_err(|_| "workspace.quick_invalid")?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let target = state
            .0
            .profiles
            .temporary_ssh_target(
                state.0.registry.clone(),
                page.clone(),
                directory,
                endpoint.clone(),
            )
            .await?;
        let id = target.id();
        let targets = &state.0.quick_targets;
        let token = targets.publish(&state.0.registry, page, target)?;
        Ok(TargetDto {
            version: 1,
            token: token.to_string(),
            target: EndpointDto {
                id: id.to_string(),
                label: endpoint.label().to_owned(),
                host: endpoint.target().host().to_owned(),
                port: endpoint.target().port(),
                username: endpoint.username().to_owned(),
            },
        })
    }
}
#[derive(Serialize)]
pub(super) struct SavedTargetDto {
    version: u8,
    host: String,
}
#[tauri::command]
pub(super) async fn workspace_quick_target_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    label: String,
) -> Result<SavedTargetDto> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(unix, feature = "workspace-ssh")))]
    {
        let _ = (page, token, label);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(unix, feature = "workspace-ssh"))]
    {
        let parsed = Uuid::parse_str(&token).map_err(|_| "workspace.denied")?;
        if parsed.to_string() != token
            || parsed.get_version_num() != 4
            || parsed.get_variant() != uuid::Variant::RFC4122
        {
            return Err("workspace.denied");
        }
        let target = state
            .0
            .quick_targets
            .claim_save(&state.0.registry, &page, parsed)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let saved = state
            .0
            .profiles
            .save_temporary_host(
                state.0.registry.clone(),
                page.clone(),
                directory,
                target,
                label,
            )
            .await;
        let _ = state
            .0
            .quick_targets
            .cancel(&state.0.registry, &page, parsed);
        Ok(SavedTargetDto {
            version: 1,
            host: saved?.to_string(),
        })
    }
}

#[tauri::command]
pub(super) fn workspace_quick_target_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(unix, feature = "workspace-ssh")))]
    {
        let _ = (page, token);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(unix, feature = "workspace-ssh"))]
    {
        let parsed = Uuid::parse_str(&token).map_err(|_| "workspace.denied")?;
        if parsed.to_string() != token
            || parsed.get_version_num() != 4
            || parsed.get_variant() != uuid::Variant::RFC4122
        {
            return Err("workspace.denied");
        }
        state
            .0
            .quick_targets
            .cancel(&state.0.registry, &page, parsed)
    }
}
