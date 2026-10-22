use super::*;
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Selection {
    host: String,
    identity: String,
    profile: String,
    private_key: bool,
    #[serde(default)]
    agent: bool,
}
#[tauri::command]
pub(crate) fn workspace_ssh_route_begin(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    selection: Selection,
) -> Result<Ticket> {
    #[cfg(all(feature = "workspace-ssh", any(unix, windows)))]
    {
        let page = session::page(&window, &state, &nonce)?.1;
        if selection.agent && selection.private_key {
            return Err("workspace.denied");
        }
        let host = canonical(&selection.host, 7)?;
        let identity = canonical(&selection.identity, 7)?;
        let profile = canonical(&selection.profile, 7)?;
        let owner = state.0.vault.clone();
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let issued = state
            .0
            .ssh_sessions
            .routes
            .begin(&owner, page.clone(), host, identity)?;
        let ticket = Ticket {
            version: 1,
            token: issued.id.to_string(),
        };
        let registry = state.0.registry.clone();
        let root = state.0.sessions.clone();
        let profiles = state.0.profiles.clone();
        let private_key = selection.private_key;
        let agent = selection.agent;
        let app = window.app_handle().clone();
        let workspace = state.inner().clone();
        let broker = issued.broker();
        issued.launch(
            tauri::async_runtime::handle().inner(),
            move |capture, confirm| async move {
                let selected = if agent {
                    pulse_store::ssh_route_source::SshRouteIdentity::Agent(identity)
                } else if private_key {
                    pulse_store::ssh_route_source::SshRouteIdentity::PrivateKey(identity)
                } else {
                    pulse_store::ssh_route_source::SshRouteIdentity::Password(identity)
                };
                let (store, route) = profiles
                    .ssh_route(
                        registry.clone(),
                        page.clone(),
                        directory,
                        capture,
                        profile,
                        selected,
                    )
                    .await?;
                let palette =
                    super::startup_palette(&app, &workspace, page.clone(), Some(host)).await?;
                crate::ssh_session_adapter::route::run(
                    crate::ssh_session_adapter::route::Context {
                        store,
                        registry,
                        page,
                        owner,
                        root,
                        size: pulse_core::TermSize::new(92, 31).map_err(|_| "workspace.denied")?,
                        history_rows: 2000,
                        palette,
                        broker: Some(broker),
                    },
                    route,
                    confirm,
                )
                .await
            },
        );
        Ok(ticket)
    }
    #[cfg(not(all(feature = "workspace-ssh", any(unix, windows))))]
    {
        let _ = (window, state, nonce, selection);
        Err("workspace.ssh_unavailable")
    }
}
#[tauri::command]
pub(crate) async fn workspace_ssh_route_wait(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<serde_json::Value> {
    #[cfg(all(feature = "workspace-ssh", any(unix, windows)))]
    {
        let page = session::page(&window, &state, &nonce)?.1;
        let reply = state
            .0
            .ssh_sessions
            .routes
            .wait(&page, canonical(&token, 4)?)
            .await?;
        serde_json::to_value(reply).map_err(|_| "workspace.ssh_failed")
    }
    #[cfg(not(all(feature = "workspace-ssh", any(unix, windows))))]
    {
        let _ = (window, state, nonce, token);
        Err("workspace.ssh_unavailable")
    }
}
#[tauri::command]
pub(crate) async fn workspace_ssh_route_confirm(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    challenge: String,
    approved: serde_json::Value,
) -> Result<serde_json::Value> {
    #[cfg(all(feature = "workspace-ssh", any(unix, windows)))]
    {
        let page = session::page(&window, &state, &nonce)?.1;
        let token = canonical(&token, 4)?;
        state.0.ssh_sessions.routes.answer(
            &page,
            token,
            canonical(&challenge, 4)?,
            serde_json::from_value::<crate::ssh_session_adapter::route::Decision>(approved)
                .map_err(|_| "workspace.denied")?,
        )?;
        let reply = state.0.ssh_sessions.routes.wait(&page, token).await?;
        serde_json::to_value(reply).map_err(|_| "workspace.ssh_failed")
    }
    #[cfg(not(all(feature = "workspace-ssh", any(unix, windows))))]
    {
        let _ = (window, state, nonce, token, challenge, approved);
        Err("workspace.ssh_unavailable")
    }
}
#[tauri::command]
pub(crate) fn workspace_ssh_route_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    #[cfg(all(feature = "workspace-ssh", any(unix, windows)))]
    {
        let page = session::page(&window, &state, &nonce)?.1;
        state
            .0
            .ssh_sessions
            .routes
            .cancel(&page, canonical(&token, 4)?)
    }
    #[cfg(not(all(feature = "workspace-ssh", any(unix, windows))))]
    {
        let _ = (window, state, nonce, token);
        Err("workspace.ssh_unavailable")
    }
}
#[tauri::command]
pub(crate) async fn workspace_ssh_route_answer(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    round: String,
    answers: Vec<String>,
) -> Result<serde_json::Value> {
    let answers = zeroize::Zeroizing::new(answers);
    #[cfg(all(feature = "workspace-ssh", any(unix, windows)))]
    {
        let page = session::page(&window, &state, &nonce)?.1;
        let token = canonical(&token, 4)?;
        state.0.ssh_sessions.routes.answer_interactive(
            &page,
            token,
            canonical(&round, 4)?,
            answers,
        )?;
        let reply = state.0.ssh_sessions.routes.wait(&page, token).await?;
        serde_json::to_value(reply).map_err(|_| "workspace.ssh_failed")
    }
    #[cfg(not(all(feature = "workspace-ssh", any(unix, windows))))]
    {
        let _ = (window, state, nonce, token, round, answers);
        Err("workspace.ssh_unavailable")
    }
}
