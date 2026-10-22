//! Read-only config preview and one-use source confirmation, never an import or SSH grant.
use super::*;
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConfigPreview {
    version: u8,
    token: String,
    host: String,
    port: u16,
    username: String,
    identity_file_count: usize,
}
#[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
fn token(value: &str) -> Result<Uuid> {
    let token = Uuid::parse_str(value).map_err(|_| "workspace.denied")?;
    if token.get_version_num() != 4
        || token.get_variant() != uuid::Variant::RFC4122
        || token.to_string() != value
    {
        return Err("workspace.denied");
    }
    Ok(token)
}
#[tauri::command]
pub(super) async fn workspace_ssh_config_preview(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    alias: String,
) -> Result<ConfigPreview> {
    #[cfg(not(all(any(unix, windows), feature = "workspace-ssh")))]
    {
        let _ = (window, state, nonce, alias);
        Err("workspace.unavailable")
    }
    #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let directory = match std::env::var_os("PULSE_WORKSPACE_SSH_CONFIG_DIRECTORY") {
            Some(path) => std::path::PathBuf::from(path),
            None => window
                .app_handle()
                .path()
                .home_dir()
                .map_err(|_| "workspace.config_unavailable")?
                .join(".ssh"),
        };
        let local_user = std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" })
            .map_err(|_| "workspace.config_unavailable")?;
        // Include discovery remains bounded and backend-owned; the page supplies only an alias.
        let capture = state
            .0
            .profiles
            .capture_config(
                state.0.registry.clone(),
                page.clone(),
                profiles::ConfigReadRequest {
                    directory,
                    includes: Vec::new(),
                    alias,
                    local_user,
                },
            )
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        let receipt = state
            .0
            .configs
            .publish(&state.0.registry, page, capture.clone())?;
        #[cfg(debug_assertions)]
        eprintln!("workspace.config_previewed");
        let value = capture.resolved();
        Ok(ConfigPreview {
            version: 1,
            token: receipt.to_string(),
            host: value.target().host().to_owned(),
            port: value.target().port(),
            username: value.user().to_owned(),
            identity_file_count: value.identity_files().len(),
        })
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_config_confirm(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token_value: String,
) -> Result<()> {
    #[cfg(not(all(any(unix, windows), feature = "workspace-ssh")))]
    {
        let _ = (window, state, nonce, token_value);
        Err("workspace.unavailable")
    }
    #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let capture = state
            .0
            .configs
            .take(&state.0.registry, &page, token(&token_value)?)?;
        if let Err(error) = state.0.profiles.confirm_config(capture).await {
            #[cfg(debug_assertions)]
            eprintln!("workspace.config_confirm_denied");
            return Err(error);
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        #[cfg(debug_assertions)]
        eprintln!("workspace.config_confirmed");
        Ok(())
    }
}

#[derive(serde::Serialize)]
pub(super) struct ConfigSourceStatus {
    version: u8,
    status: &'static str,
}
#[tauri::command]
pub(super) async fn workspace_ssh_config_status(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token_value: String,
) -> Result<ConfigSourceStatus> {
    #[cfg(not(all(any(unix, windows), feature = "workspace-ssh")))]
    {
        let _ = (window, state, nonce, token_value);
        Err("workspace.unavailable")
    }
    #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let id = token(&token_value)?;
        let capture = state.0.configs.borrow(&state.0.registry, &page, id)?;
        let status = match state.0.profiles.confirm_config(capture).await {
            Ok(()) => "unchanged",
            Err("workspace.config_conflict") => "changed",
            Err(error) => return Err(error),
        };
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        let _ = state.0.configs.borrow(&state.0.registry, &page, id)?;
        #[cfg(debug_assertions)]
        if status == "changed" {
            eprintln!("workspace.config_source_changed");
        } else {
            eprintln!("workspace.config_source_unchanged");
        }
        Ok(ConfigSourceStatus { version: 1, status })
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConfigImportPreview {
    version: u8,
    token: String,
    label: String,
    host: String,
    port: u16,
    username: String,
    identity_file_count: usize,
}
#[tauri::command]
pub(super) async fn workspace_ssh_config_import_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token_value: String,
    label: String,
) -> Result<ConfigImportPreview> {
    #[cfg(not(all(any(unix, windows), feature = "workspace-ssh")))]
    {
        let _ = (window, state, nonce, token_value, label);
        Err("workspace.unavailable")
    }
    #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let source = state
            .0
            .configs
            .take(&state.0.registry, &page, token(&token_value)?)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let capture = state
            .0
            .profiles
            .prepare_config_import(source, directory, label)
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        let receipt = state
            .0
            .config_imports
            .publish(&state.0.registry, capture.clone())?;
        let value = capture.endpoint();
        #[cfg(debug_assertions)]
        eprintln!("workspace.config_import_prepared");
        Ok(ConfigImportPreview {
            version: 1,
            token: receipt.to_string(),
            label: value.label().to_owned(),
            host: value.target().host().to_owned(),
            port: value.target().port(),
            username: value.username().to_owned(),
            identity_file_count: capture.identity_file_count(),
        })
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_config_import_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token_value: String,
) -> Result<String> {
    #[cfg(not(all(any(unix, windows), feature = "workspace-ssh")))]
    {
        let _ = (window, state, nonce, token_value);
        Err("workspace.unavailable")
    }
    #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let (capture, control) =
            state
                .0
                .config_imports
                .claim(&state.0.registry, &page, token(&token_value)?)?;
        let saved = state.0.profiles.import_config(capture, control).await;
        let snapshot = match saved {
            Ok(value) => value,
            Err(error) => {
                #[cfg(debug_assertions)]
                eprintln!("workspace.config_import_denied");
                return Err(error);
            }
        };
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        let id = snapshot
            .records()
            .last()
            .ok_or("workspace.hosts_failed")?
            .id()
            .to_string();
        #[cfg(debug_assertions)]
        eprintln!("workspace.config_import_saved");
        Ok(id)
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_config_import_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token_value: String,
) -> Result<()> {
    #[cfg(not(all(any(unix, windows), feature = "workspace-ssh")))]
    {
        let _ = (window, state, nonce, token_value);
        Err("workspace.unavailable")
    }
    #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, page) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        state
            .0
            .config_imports
            .cancel(&state.0.registry, &page, token(&token_value)?)
    }
}
