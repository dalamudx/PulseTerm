//! Native-only Profile discovery inputs, current-document startup and root listing.
use super::*;
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    kind: String,
}
#[cfg(any(unix, windows))]
pub(super) fn kind(value: &str) -> Result<pulse_pty::profiles::ShellKind> {
    use pulse_pty::profiles::ShellKind;
    match value {
        "bash" => Ok(ShellKind::Bash),
        "zsh" => Ok(ShellKind::Zsh),
        "fish" => Ok(ShellKind::Fish),
        "sh" => Ok(ShellKind::Sh),
        "pwsh" => Ok(ShellKind::Pwsh),
        "powershell" => Ok(ShellKind::PowerShell),
        "cmd" => Ok(ShellKind::Cmd),
        "wsl" => Ok(ShellKind::Wsl),
        _ => Err("workspace.denied"),
    }
}
pub(super) fn page(
    window: &WebviewWindow,
    state: &Workspace,
    nonce: &str,
) -> Result<(Uuid, Arc<pulse_app::registry::PageLaunchScope>)> {
    if !local_workspace(window) {
        return Err("workspace.denied");
    }
    current_document(
        &state.0.documents,
        state.0.owner,
        &window.url().map_err(|_| "workspace.denied")?,
        nonce,
    )
}
#[tauri::command]
pub(super) async fn workspace_profile_editor_read(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    shell: pulse_store::profiles::AutomaticShell,
) -> Result<serde_json::Value> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, shell);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        let offer = state
            .0
            .profiles
            .editor_read(
                state.0.registry.clone(),
                scope,
                profile_source(&window)?,
                shell,
            )
            .await?;
        page(&window, &state, &nonce)?;
        serde_json::to_value(offer).map_err(|_| "workspace.profile_editor_failed")
    }
}
#[tauri::command]
pub(super) async fn workspace_profile_editor_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: Uuid,
    config: pulse_store::profile_editor::ProfileConfig,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket, config);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        state
            .0
            .profiles
            .editor_save(state.0.registry.clone(), scope, ticket, config)
            .await?;
        page(&window, &state, &nonce)?;
        eprintln!("workspace.profile_editor_saved");
        Ok(())
    }
}
#[tauri::command]
pub(super) fn workspace_profile_editor_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: Uuid,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        state
            .0
            .profiles
            .editor_cancel(&state.0.registry, &scope, ticket)
    }
}
#[tauri::command]
pub(super) async fn workspace_custom_profile_open(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    id: Uuid,
) -> Result<String> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, id);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        let source = profile_source(&window)?;
        let startup = state
            .0
            .settings
            .terminal_startup(
                state.0.registry.clone(),
                scope.clone(),
                window
                    .app_handle()
                    .path()
                    .app_config_dir()
                    .map_err(|_| "workspace.settings_unavailable")?,
                settings::system_dark(&window),
            )
            .await?;
        let id = state
            .0
            .profiles
            .custom_start(
                state.0.registry.clone(),
                scope,
                state.0.sessions.clone(),
                profiles::custom_service::CustomStart {
                    cwd: None,
                    source,
                    id,
                    startup,
                    interrupt: None,
                },
            )
            .await?;
        page(&window, &state, &nonce)?;
        Ok(id.get().to_string())
    }
}
#[tauri::command]
pub(super) async fn workspace_custom_profile_list(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<serde_json::Value> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        let result = state
            .0
            .profiles
            .custom_list(state.0.registry.clone(), scope, profile_source(&window)?)
            .await?;
        serde_json::to_value(result).map_err(|_| "workspace.unavailable")
    }
}
#[tauri::command]
pub(super) async fn workspace_custom_profile_read(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<serde_json::Value> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        let offer = state
            .0
            .profiles
            .custom_read(state.0.registry.clone(), scope, profile_source(&window)?)
            .await?;
        page(&window, &state, &nonce)?;
        serde_json::to_value(offer).map_err(|_| "workspace.profile_editor_failed")
    }
}
#[tauri::command]
pub(super) async fn workspace_custom_profile_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: Uuid,
    change: profiles::custom_service::CustomAction,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket, change);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        state
            .0
            .profiles
            .custom_save(state.0.registry.clone(), scope, ticket, change)
            .await?;
        page(&window, &state, &nonce)?;
        Ok(())
    }
}
#[tauri::command]
pub(super) fn workspace_custom_profile_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: Uuid,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        state
            .0
            .profiles
            .custom_cancel(&state.0.registry, &scope, ticket)
    }
}
/// Explicit default choice metadata. No process is started by reading or saving it.
#[tauri::command]
pub(super) async fn workspace_profile_default_read(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<serde_json::Value> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        let offer = state
            .0
            .profiles
            .default_read(
                state.0.registry.clone(),
                scope,
                window
                    .app_handle()
                    .path()
                    .app_data_dir()
                    .map_err(|_| "workspace.unavailable")?,
                std::env::var_os("PATH").unwrap_or_default(),
                std::env::var_os("SHELL").map(std::path::PathBuf::from),
            )
            .await?;
        serde_json::to_value(offer).map_err(|_| "workspace.unavailable")
    }
}
#[tauri::command]
pub(super) async fn workspace_profile_default_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: Uuid,
    selected: Option<pulse_store::profile_default::DefaultProfileSelection>,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket, selected);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        let source = (
            window
                .app_handle()
                .path()
                .app_data_dir()
                .map_err(|_| "workspace.unavailable")?,
            std::env::var_os("PATH").unwrap_or_default(),
            std::env::var_os("SHELL").map(std::path::PathBuf::from),
        );
        state
            .0
            .profiles
            .default_save_choice(state.0.registry.clone(), scope, source, ticket, selected)
            .await?;
        #[cfg(debug_assertions)]
        eprintln!("workspace.profile_default_saved");
        Ok(())
    }
}
#[tauri::command]
pub(super) fn workspace_profile_default_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: Uuid,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        state
            .0
            .profiles
            .default_cancel(&state.0.registry, &scope, ticket)
    }
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum LifecycleAction {
    Delete,
    Restore,
}
#[cfg(any(unix, windows))]
fn profile_source(
    window: &WebviewWindow,
) -> Result<(
    std::path::PathBuf,
    std::ffi::OsString,
    Option<std::path::PathBuf>,
)> {
    Ok((
        window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?,
        std::env::var_os("PATH").unwrap_or_default(),
        std::env::var_os("SHELL").map(std::path::PathBuf::from),
    ))
}
/// Read tombstone identities only; this never prepares or performs a restoration.
#[tauri::command]
pub(super) async fn workspace_profile_deleted(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Vec<pulse_store::profiles::AutomaticShell>> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        state
            .0
            .profiles
            .lifecycle_deleted(state.0.registry.clone(), scope, profile_source(&window)?)
            .await
    }
}
#[tauri::command]
pub(super) async fn workspace_profile_lifecycle_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    shell: pulse_store::profiles::AutomaticShell,
    action: LifecycleAction,
) -> Result<serde_json::Value> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, shell, action);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        use pulse_store::profile_default::ProfileLifecycleEdit;
        let edit = match action {
            LifecycleAction::Delete => ProfileLifecycleEdit::Delete(shell),
            LifecycleAction::Restore => ProfileLifecycleEdit::Restore(shell),
        };
        let offer = state
            .0
            .profiles
            .lifecycle_prepare(
                state.0.registry.clone(),
                scope,
                profile_source(&window)?,
                edit,
            )
            .await?;
        serde_json::to_value(offer).map_err(|_| "workspace.unavailable")
    }
}
#[tauri::command]
pub(super) async fn workspace_profile_lifecycle_commit(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: Uuid,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        state
            .0
            .profiles
            .lifecycle_commit(state.0.registry.clone(), scope, ticket)
            .await?;
        #[cfg(debug_assertions)]
        eprintln!("workspace.profile_lifecycle_committed");
        Ok(())
    }
}
#[tauri::command]
pub(super) fn workspace_profile_lifecycle_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: Uuid,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        state
            .0
            .profiles
            .lifecycle_cancel(&state.0.registry, &scope, ticket)
    }
}

/// Fixed automatic Profile visibility preference only, not a launch/settings permission.
#[tauri::command]
pub(super) async fn workspace_profile_visibility(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    profile: String,
    hidden: bool,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, profile, hidden);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, scope) = page(&window, &state, &nonce)?;
        let source = super::profiles::ProfileStart {
            cwd: None,
            preset: pulse_store::settings::TerminalPreset::Classic.into(),
            directory: window
                .app_handle()
                .path()
                .app_data_dir()
                .map_err(|_| "workspace.unavailable")?,
            path: std::env::var_os("PATH").ok_or("workspace.unavailable")?,
            shell: std::env::var_os("SHELL").map(std::path::PathBuf::from),
            kind: kind(&profile)?,
        };
        state
            .0
            .profiles
            .set_visibility(state.0.registry.clone(), scope, source, hidden)
            .await?;
        page(&window, &state, &nonce)?;
        eprintln!("workspace.profile_visibility_saved");
        Ok(())
    }
}

#[tauri::command]
pub(super) async fn workspace_session_open(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: Selection,
) -> Result<String> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, selection);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (nonce, scope) = page(&window, &state, &nonce)?;
        let kind = kind(&selection.kind)?;
        let mut source = profiles::ProfileStart {
            cwd: None,
            preset: pulse_store::settings::TerminalPreset::Classic.into(),
            directory: window
                .app_handle()
                .path()
                .app_data_dir()
                .map_err(|_| "workspace.unavailable")?,
            path: std::env::var_os("PATH").ok_or("workspace.unavailable")?,
            shell: std::env::var_os("SHELL").map(std::path::PathBuf::from),
            kind,
        };
        source.preset = state
            .0
            .settings
            .terminal_startup(
                state.0.registry.clone(),
                scope.clone(),
                window
                    .app_handle()
                    .path()
                    .app_config_dir()
                    .map_err(|_| "workspace.settings_unavailable")?,
                settings::system_dark(&window),
            )
            .await?;
        let id = state
            .0
            .profiles
            .start(
                state.0.registry.clone(),
                scope,
                state.0.sessions.clone(),
                source,
            )
            .await?;
        // A registered session survives a lost response or navigation. The next
        // current page can list and reattach it; never auto-retry process creation.
        page(&window, &state, &nonce.to_string())?;
        eprintln!("workspace.session_opened");
        eprintln!("workspace.profile_selected.{}", kind.executable_name());
        Ok(id.get().to_string())
    }
}
#[tauri::command]
pub(super) fn workspace_sessions(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Vec<String>> {
    let (_, scope) = page(&window, &state, &nonce)?;
    let candidates = state
        .0
        .sessions
        .lock()
        .map_err(|_| "workspace.unavailable")?
        .ids();
    let ids = state
        .0
        .registry
        .owned_page_sessions(&scope, &candidates)
        .map_err(|_| "workspace.denied")?;
    eprintln!("workspace.sessions_listed");
    Ok(ids.into_iter().map(|id| id.get().to_string()).collect())
}
#[tauri::command]
pub(super) async fn workspace_session_close(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    session: String,
) -> Result<()> {
    let (_, scope) = page(&window, &state, &nonce)?;
    let id = state
        .0
        .sessions
        .lock()
        .map_err(|_| "workspace.unavailable")?
        .ids()
        .into_iter()
        .find(|id| id.get().to_string() == session)
        .ok_or("workspace.denied")?;
    // Close effect is admitted under the same metadata lock as page validation.
    // Already-admitted close cannot be retracted by subsequent navigation.
    state
        .0
        .registry
        .request_close_page_session(&scope, id)
        .map_err(|_| "workspace.denied")?;
    enum Cleanup {
        Local(pulse_app::local_session_owner::LocalSessionCleanup),
        Protocol(pulse_app::protocol_session_owner::ProtocolSessionCleanup),
    }
    let cleanup = {
        let mut root = state
            .0
            .sessions
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if root.is_protocol(id) {
            Cleanup::Protocol(root.close_protocol(id).map_err(|_| "workspace.denied")?)
        } else {
            Cleanup::Local(root.close(id).map_err(|_| "workspace.denied")?)
        }
    };
    match cleanup {
        Cleanup::Local(cleanup) => {
            let receipt = cleanup.join().await.map_err(|_| "workspace.unavailable")?;
            eprintln!(
                "workspace.session_exit.{}.{}",
                receipt.native.exit_code, receipt.native.cancelled
            );
        }
        Cleanup::Protocol(cleanup) => {
            cleanup.join().await.map_err(|_| "workspace.unavailable")?;
            // Physical resources joined, NOT a remote native process exit-code receipt.
            eprintln!("workspace.protocol_session_resources_joined");
        }
    }
    state
        .0
        .sessions
        .lock()
        .map_err(|_| "workspace.unavailable")?
        .collect_reaped();
    eprintln!("workspace.session_closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_has_no_path_arguments_environment_or_mode() {
        assert!(serde_json::from_str::<Selection>("{\"kind\":\"sh\"}").is_ok());
        for field in ["program", "args", "env", "cwd", "mode"] {
            let mut value = serde_json::json!({"kind":"sh"});
            value[field] = serde_json::json!("unexpected");
            assert!(serde_json::from_value::<Selection>(value).is_err());
        }
    }
    #[cfg(any(unix, windows))]
    #[test]
    fn literal_discovery_kind_is_not_a_command_or_path() {
        for value in ["/bin/sh", "sh -c", "bash;true", "SH", "", "unknown"] {
            assert!(kind(value).is_err());
        }
        for value in ["bash", "zsh", "fish", "sh", "pwsh"] {
            assert!(kind(value).is_ok());
        }
    }
}
