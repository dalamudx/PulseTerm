//! Saved host/identity single-use connection entry. Never accepts address, key, password or executable.
use super::*;
#[path = "product_ssh_route.rs"]
pub(super) mod route;
#[cfg(all(feature = "workspace-ssh", unix))]
use crate::ssh_session_adapter::requests::{SessionRequests, StartTarget};
pub(super) struct SessionFactory {
    #[cfg(all(feature = "workspace-ssh", unix))]
    requests: Arc<SessionRequests>,
    #[cfg(all(feature = "workspace-ssh", unix))]
    routes: Arc<crate::ssh_session_adapter::route::requests::Bank>,
}

/// Freeze a display-only preference from the exact original page before dialing.
/// A settings failure rejects startup; it never falls back to a different base.
#[cfg(all(feature = "workspace-ssh", unix))]
pub(super) async fn startup_palette(
    app: &tauri::AppHandle,
    state: &Workspace,
    page: Arc<pulse_app::registry::PageLaunchScope>,
    host: Option<Uuid>,
) -> Result<pulse_app::TerminalStartConfig> {
    state
        .0
        .registry
        .check_launch_page(&page)
        .map_err(|_| "workspace.denied")?;
    let window = app
        .get_webview_window(state.native_label()?)
        .ok_or("workspace.denied")?;
    let system_dark = settings::system_dark(&window);
    let config_directory = app
        .path()
        .app_config_dir()
        .map_err(|_| "workspace.unavailable")?;
    let mut resolved = state
        .0
        .settings
        .resolved_terminal(
            state.0.registry.clone(),
            page.clone(),
            config_directory,
            system_dark,
        )
        .await?;
    if let Some(host) = host {
        let directory = app
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let snapshot = state
            .0
            .profiles
            .behavior(
                state.0.registry.clone(),
                page.clone(),
                directory,
                pulse_store::settings_scope::SettingsScope::Host(host),
                None,
            )
            .await?;
        resolved = snapshot.resolve(resolved);
    }
    state
        .0
        .registry
        .check_launch_page(&page)
        .map_err(|_| "workspace.denied")?;
    Ok(resolved.terminal_startup(system_dark).into())
}
impl SessionFactory {
    pub(super) fn new(_registry: SessionRegistry, _owner: WindowToken) -> Self {
        #[cfg(all(feature = "workspace-ssh", unix))]
        let requests = SessionRequests::new(
            _registry,
            _owner,
            tauri::async_runtime::handle().inner().clone(),
        );
        Self {
            #[cfg(all(feature = "workspace-ssh", unix))]
            routes: crate::ssh_session_adapter::route::requests::Bank::new(
                requests.startup_capacity(),
                tauri::async_runtime::handle().inner().clone(),
            ),
            #[cfg(all(feature = "workspace-ssh", unix))]
            requests,
        }
    }
    #[cfg(all(feature = "workspace-cli", feature = "workspace-ssh", unix))]
    pub(super) fn native_requests(&self) -> Arc<SessionRequests> {
        self.requests.clone()
    }
    #[cfg(all(feature = "workspace-cli", feature = "workspace-ssh", unix))]
    pub(super) fn native_routes(&self) -> Arc<crate::ssh_session_adapter::route::requests::Bank> {
        self.routes.clone()
    }
    pub(super) fn retire(&self) {
        #[cfg(all(feature = "workspace-ssh", unix))]
        {
            self.routes.retire();
            self.requests.retire();
        }
    }
    pub(super) fn close(&self) {
        #[cfg(all(feature = "workspace-ssh", unix))]
        {
            self.routes.close();
            self.requests.close();
        }
    }
    pub(super) async fn shutdown(&self) -> Result<()> {
        self.close();
        #[cfg(all(feature = "workspace-ssh", unix))]
        self.requests.shutdown().await?;
        Ok(())
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedSelection {
    host: String,
    identity: String,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Ticket {
    version: u8,
    token: String,
}
#[cfg(all(feature = "workspace-ssh", unix))]
fn canonical(text: &str, version: usize) -> Result<Uuid> {
    let uuid = Uuid::parse_str(text).map_err(|_| "workspace.denied")?;
    if text.len() != 36
        || uuid.to_string() != text
        || uuid.get_version_num() != version
        || uuid.get_variant() != uuid::Variant::RFC4122
    {
        return Err("workspace.denied");
    };
    Ok(uuid)
}
#[tauri::command]
pub(super) fn workspace_ssh_agent_interactive_session_begin(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: SavedSelection,
    challenges: tauri::ipc::Channel<serde_json::Value>,
) -> Result<Ticket> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, selection, challenges);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let host = canonical(&selection.host, 7)?;
        let identity = canonical(&selection.identity, 7)?;
        let gesture = state
            .0
            .vault
            .begin_agent_interactive_session_gesture_authorized(
                page.clone(),
                host,
                identity,
                Arc::new(|| true),
            )
            .map_err(|_| "workspace.ssh_denied")?;
        let broker = crate::ssh_session_adapter::interactive::Broker::new(move |prompt| {
            challenges
                .send(prompt)
                .map_err(|_| pulse_app::vault_owner::VaultOwnerError::Denied)
        });
        let token = state
            .0
            .ssh_sessions
            .requests
            .begin_agent_interactive_gesture(
                page,
                host,
                identity,
                &state.0.vault,
                gesture,
                broker,
            )?;
        Ok(Ticket {
            version: 1,
            token: token.to_string(),
        })
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_password_interactive_session_begin(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: SavedSelection,
    challenges: tauri::ipc::Channel<serde_json::Value>,
) -> Result<Ticket> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, selection, challenges);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let host = canonical(&selection.host, 7)?;
        let identity = canonical(&selection.identity, 7)?;
        let gesture = state
            .0
            .vault
            .begin_password_interactive_session_gesture_authorized(
                page.clone(),
                host,
                identity,
                Arc::new(|| true),
            )
            .map_err(|_| "workspace.ssh_denied")?;
        let broker = crate::ssh_session_adapter::interactive::Broker::new(move |prompt| {
            challenges
                .send(prompt)
                .map_err(|_| pulse_app::vault_owner::VaultOwnerError::Denied)
        });
        let token = state
            .0
            .ssh_sessions
            .requests
            .begin_password_interactive_gesture(
                page,
                host,
                identity,
                &state.0.vault,
                gesture,
                broker,
            )?;
        Ok(Ticket {
            version: 1,
            token: token.to_string(),
        })
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_private_key_interactive_session_begin(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: SavedSelection,
    challenges: tauri::ipc::Channel<serde_json::Value>,
) -> Result<Ticket> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, selection, challenges);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let host = canonical(&selection.host, 7)?;
        let identity = canonical(&selection.identity, 7)?;
        let gesture = state
            .0
            .vault
            .begin_private_key_interactive_session_gesture_authorized(
                page.clone(),
                host,
                identity,
                Arc::new(|| true),
            )
            .map_err(|_| "workspace.ssh_denied")?;
        let broker = crate::ssh_session_adapter::interactive::Broker::new(move |prompt| {
            challenges
                .send(prompt)
                .map_err(|_| pulse_app::vault_owner::VaultOwnerError::Denied)
        });
        let token = state
            .0
            .ssh_sessions
            .requests
            .begin_key_interactive_gesture(page, host, identity, &state.0.vault, gesture, broker)?;
        Ok(Ticket {
            version: 1,
            token: token.to_string(),
        })
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_interactive_session_begin(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: SavedSelection,
    challenges: tauri::ipc::Channel<serde_json::Value>,
) -> Result<Ticket> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, selection, challenges);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let host = canonical(&selection.host, 7)?;
        let identity = canonical(&selection.identity, 7)?;
        let gesture = state
            .0
            .vault
            .begin_interactive_session_gesture_authorized(
                page.clone(),
                host,
                identity,
                Arc::new(|| true),
            )
            .map_err(|_| "workspace.ssh_denied")?;
        let broker = crate::ssh_session_adapter::interactive::Broker::new(move |prompt| {
            challenges
                .send(prompt)
                .map_err(|_| pulse_app::vault_owner::VaultOwnerError::Denied)
        });
        let token = state.0.ssh_sessions.requests.begin_interactive_gesture(
            page,
            host,
            identity,
            &state.0.vault,
            gesture,
            broker,
        )?;
        Ok(Ticket {
            version: 1,
            token: token.to_string(),
        })
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_interactive_session_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let token = canonical(&token, 4)?;
        let requests = state.0.ssh_sessions.requests.clone();
        let guard = requests.authorization(&page, token)?;
        let temporary = state.0.quick_targets.resolve(
            &state.0.registry,
            &page,
            requests.selected_host(&page, token)?,
        )?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let reader = state.0.profiles.clone();
        let registry = state.0.registry.clone();
        let source_page = page.clone();
        requests
            .prepare_interactive(page.clone(), token, async move {
                reader
                    .ssh_key_source(registry, source_page, directory, guard, temporary)
                    .await
            })
            .await?;
        let (_, current) = session::page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&current, &page) {
            return Err("workspace.denied");
        }
        Ok(())
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_interactive_session_answer(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    round: String,
    answers: Vec<String>,
) -> Result<()> {
    let answers = zeroize::Zeroizing::new(answers);
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token, round, answers);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        state.0.ssh_sessions.requests.answer_interactive(
            &page,
            canonical(&token, 4)?,
            canonical(&round, 4)?,
            answers,
        )
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_key_session_begin(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: SavedSelection,
) -> Result<Ticket> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, selection.host, selection.identity);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let host = canonical(&selection.host, 7)?;
        let identity = canonical(&selection.identity, 7)?;
        let gesture = state
            .0
            .vault
            .begin_private_key_session_gesture_authorized(
                page.clone(),
                host,
                identity,
                Arc::new(|| true),
            )
            .map_err(|_| "workspace.ssh_denied")?;
        let ticket = state.0.ssh_sessions.requests.begin_key_gesture(
            page,
            host,
            identity,
            &state.0.vault,
            gesture,
        )?;
        eprintln!("workspace.ssh_key_session_begun");
        Ok(Ticket {
            version: 1,
            token: ticket.to_string(),
        })
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_key_session_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let token = canonical(&token, 4)?;
        let requests = state.0.ssh_sessions.requests.clone();
        let guard = requests.authorization(&page, token)?;
        let temporary = state.0.quick_targets.resolve(
            &state.0.registry,
            &page,
            requests.selected_host(&page, token)?,
        )?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let reader = state.0.profiles.clone();
        let registry = state.0.registry.clone();
        let source_page = page.clone();
        requests
            .prepare_key(page.clone(), token, async move {
                reader
                    .ssh_key_source(registry, source_page, directory, guard, temporary)
                    .await
            })
            .await?;
        let (_, current) = session::page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&current, &page) {
            return Err("workspace.denied");
        }
        eprintln!("workspace.ssh_key_session_prepared");
        Ok(())
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_agent_session_begin(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: SavedSelection,
) -> Result<Ticket> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, selection);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let host = canonical(&selection.host, 7)?;
        let identity = canonical(&selection.identity, 7)?;
        let gesture = state
            .0
            .vault
            .begin_agent_session_gesture_authorized(page.clone(), host, identity, Arc::new(|| true))
            .map_err(|_| "workspace.ssh_denied")?;
        let ticket = state.0.ssh_sessions.requests.begin_agent_gesture(
            page,
            host,
            identity,
            &state.0.vault,
            gesture,
        )?;
        Ok(Ticket {
            version: 1,
            token: ticket.to_string(),
        })
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_agent_session_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let token = canonical(&token, 4)?;
        let requests = state.0.ssh_sessions.requests.clone();
        let guard = requests.authorization(&page, token)?;
        let temporary = state.0.quick_targets.resolve(
            &state.0.registry,
            &page,
            requests.selected_host(&page, token)?,
        )?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let reader = state.0.profiles.clone();
        let registry = state.0.registry.clone();
        let source_page = page.clone();
        requests
            .prepare_agent(page.clone(), token, async move {
                reader
                    .ssh_key_source(registry, source_page, directory, guard, temporary)
                    .await
            })
            .await?;
        let (_, current) = session::page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&current, &page) {
            return Err("workspace.denied");
        }
        Ok(())
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_agent_session_start(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<String> {
    workspace_ssh_session_start(window, state, nonce, token).await
}
#[tauri::command]
pub(super) fn workspace_ssh_agent_session_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    workspace_ssh_session_cancel(window, state, nonce, token)
}
#[tauri::command]
pub(super) fn workspace_ssh_session_begin(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: SavedSelection,
) -> Result<Ticket> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, selection.host, selection.identity);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let host = canonical(&selection.host, 7)?;
        let identity = canonical(&selection.identity, 7)?;
        // The original user/page action cannot borrow a later unlock while metadata reads run.
        let gesture = state
            .0
            .vault
            .begin_password_session_gesture_authorized(
                page.clone(),
                host,
                identity,
                Arc::new(|| true),
            )
            .map_err(|_| "workspace.ssh_denied")?;
        let ticket = state.0.ssh_sessions.requests.begin_gesture(
            page,
            host,
            identity,
            &state.0.vault,
            gesture,
        )?;
        eprintln!("workspace.ssh_session_begun");
        Ok(Ticket {
            version: 1,
            token: ticket.to_string(),
        })
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_session_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let token = canonical(&token, 4)?;
        let requests = state.0.ssh_sessions.requests.clone();
        let guard = requests.authorization(&page, token)?;
        let temporary = state.0.quick_targets.resolve(
            &state.0.registry,
            &page,
            requests.selected_host(&page, token)?,
        )?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let reader = state.0.profiles.clone();
        let registry = state.0.registry.clone();
        let source_page = page.clone();
        requests
            .prepare(page.clone(), token, async move {
                reader
                    .ssh_trust_source(registry, source_page, directory, guard, temporary)
                    .await
            })
            .await?;
        let (_, current) = session::page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&current, &page) {
            return Err("workspace.denied");
        };
        eprintln!("workspace.ssh_session_prepared");
        Ok(())
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_session_start(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<String> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let token = canonical(&token, 4)?;
        let selected = state.0.ssh_sessions.requests.selected_host(&page, token)?;
        let temporary = state
            .0
            .quick_targets
            .resolve(&state.0.registry, &page, selected)?;
        let target = StartTarget {
            root: state.0.sessions.clone(),
            size: pulse_core::TermSize::new(80, 24).map_err(|_| "workspace.unavailable")?,
            history_rows: 10000,
            palette: startup_palette(
                window.app_handle(),
                &state,
                page.clone(),
                if temporary.is_some() {
                    None
                } else {
                    Some(selected)
                },
            )
            .await?,
        };
        let id = state
            .0
            .ssh_sessions
            .requests
            .start(page.clone(), token, target)
            .await?;
        if temporary.is_some()
            && state
                .0
                .quick_targets
                .registered(&state.0.registry, &page, selected, id)
                .is_err()
        {
            // Metadata loss never turns an owned session into retryable startup failure.
            eprintln!("workspace.quick_save_receipt_discarded");
        }
        // Root handoff persists through lost/retired page responses; metadata publication does not.
        let (_, current) = session::page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&current, &page) {
            return Err("workspace.denied");
        };
        eprintln!("workspace.ssh_session_opened");
        Ok(id.get().to_string())
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_session_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_disabled")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        state
            .0
            .ssh_sessions
            .requests
            .cancel(&page, canonical(&token, 4)?)?;
        eprintln!("workspace.ssh_session_cancelled");
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_selection_rejects_execution_and_credential_fields() {
        assert!(
            serde_json::from_str::<SavedSelection>(
                r#"{"host":"x","identity":"y","password":"not-accepted"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<SavedSelection>(
                r#"{"host":"x","identity":"y","program":"not-accepted"}"#
            )
            .is_err()
        );
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    #[test]
    fn opaque_and_saved_namespaces_are_not_interchangeable() {
        let ticket = Uuid::new_v4().to_string();
        let saved = Uuid::now_v7().to_string();
        assert!(canonical(&ticket, 4).is_ok());
        assert!(canonical(&ticket, 7).is_err());
        assert!(canonical(&saved, 4).is_err());
        assert!(canonical(&saved, 7).is_ok());
        assert!(canonical(&ticket.to_uppercase(), 4).is_err());
    }
}
