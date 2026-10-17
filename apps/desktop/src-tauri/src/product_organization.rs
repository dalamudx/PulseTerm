//! Original-page organization receipts; metadata only, no execution or Vault-use authority.
use super::*;
use pulse_store::catalogue_organization::Edit;
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct View {
    token: String,
    data: serde_json::Value,
}
#[tauri::command]
pub(super) async fn workspace_recent_hosts(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Vec<pulse_store::recent_hosts::RecentHost>> {
    #[cfg(not(unix))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
    #[cfg(unix)]
    {
        let (_, page) = session::page(&window, &state, &nonce)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let rows = state
            .0
            .profiles
            .recent_hosts(state.0.registry.clone(), page.clone(), directory)
            .await?;
        let (_, current) = session::page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        Ok(rows)
    }
}
pub(super) enum OrganizationCapture {
    Directory(Box<pulse_store::catalogue_organization::Snapshot>),
    Behavior(Box<pulse_store::organization_behavior::Snapshot>),
}
#[tauri::command]
pub(super) async fn workspace_organization(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<View> {
    operation(window, state, nonce, None).await
}
#[tauri::command]
pub(super) async fn workspace_organization_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    change: Edit,
) -> Result<View> {
    operation(window, state, nonce, Some((token, change))).await
}
async fn operation(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    change: Option<(String, Edit)>,
) -> Result<View> {
    #[cfg(not(unix))]
    {
        let _ = (window, state, nonce, change);
        Err("workspace.unavailable")
    }
    #[cfg(unix)]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let change = if let Some((token, edit)) = change {
            let ticket = Uuid::parse_str(&token).map_err(|_| "workspace.denied")?;
            if ticket.to_string() != token {
                return Err("workspace.denied");
            }
            Some((
                state
                    .0
                    .organization
                    .take(&state.0.registry, &page, ticket)
                    .and_then(|capture| match capture {
                        OrganizationCapture::Directory(source) => Ok(*source),
                        _ => Err("workspace.denied"),
                    })?,
                edit,
            ))
        } else {
            None
        };
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let snapshot = state
            .0
            .profiles
            .organization(state.0.registry.clone(), page.clone(), directory, change)
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        let token = state.0.organization.publish(
            &state.0.registry,
            page,
            OrganizationCapture::Directory(Box::new(snapshot.clone())),
        )?;
        Ok(View {
            token: token.to_string(),
            data: snapshot.projection(),
        })
    }
}
