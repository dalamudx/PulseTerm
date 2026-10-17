//! Original-page-bound host/group terminal behavior editor.
use super::organization::OrganizationCapture;
use super::{Workspace, current_document, session};
use pulse_store::{organization_behavior::Overrides, settings_scope::SettingsScope};
use tauri::{Manager, State, WebviewWindow};
use uuid::Uuid;
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Target {
    kind: String,
    id: String,
}
impl Target {
    fn scope(&self) -> Result<SettingsScope, &'static str> {
        let id = Uuid::parse_str(&self.id).map_err(|_| "workspace.denied")?;
        if id.is_nil() || id.to_string() != self.id {
            return Err("workspace.denied");
        }
        match self.kind.as_str() {
            "group" => Ok(SettingsScope::Group(id)),
            "host" => Ok(SettingsScope::Host(id)),
            _ => Err("workspace.denied"),
        }
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Change {
    token: String,
    values: Overrides,
}
#[tauri::command]
pub(super) async fn workspace_organization_behavior(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    target: Target,
    change: Option<Change>,
) -> Result<serde_json::Value, &'static str> {
    #[cfg(not(unix))]
    {
        let _ = (window, state, nonce, target, change);
        Err("workspace.unavailable")
    }
    #[cfg(unix)]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let scope = target.scope()?;
        let change = if let Some(change) = change {
            let token = Uuid::parse_str(&change.token).map_err(|_| "workspace.denied")?;
            if token.to_string() != change.token {
                return Err("workspace.denied");
            }
            match state.0.organization.take(&state.0.registry, &page, token)? {
                OrganizationCapture::Behavior(snapshot) => Some((*snapshot, change.values)),
                _ => return Err("workspace.denied"),
            }
        } else {
            None
        };
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let config_directory = window
            .app_handle()
            .path()
            .app_config_dir()
            .map_err(|_| "workspace.unavailable")?;
        let source = state
            .0
            .settings
            .resolved_terminal(
                state.0.registry.clone(),
                page.clone(),
                config_directory,
                crate::product_workspace::settings::system_dark(&window),
            )
            .await?;
        let snapshot = state
            .0
            .profiles
            .behavior(
                state.0.registry.clone(),
                page.clone(),
                directory,
                scope,
                change,
            )
            .await?;
        if !std::sync::Arc::ptr_eq(&page, &session::page(&window, &state, &nonce)?.1) {
            return Err("workspace.denied");
        }
        let data = snapshot.projection(source);
        let bank = &state.0.organization;
        let token = bank.publish(
            &state.0.registry,
            page,
            OrganizationCapture::Behavior(Box::new(snapshot)),
        )?;
        Ok(serde_json::json!({"version":2, "token":token.to_string(), "data":data}))
    }
}
