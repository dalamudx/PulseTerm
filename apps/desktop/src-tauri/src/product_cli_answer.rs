//! Fixed GUI response only; frontend cannot choose/replace action or launch parameters.
use super::*;
#[tauri::command]
pub(super) fn workspace_cli_answer(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
    accept: bool,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(all(
        feature = "workspace-cli",
        any(target_os = "linux", target_os = "macos", windows)
    ))]
    {
        cli::answer(&state, &page, &ticket, accept)
    }
    #[cfg(not(all(
        feature = "workspace-cli",
        any(target_os = "linux", target_os = "macos", windows)
    )))]
    {
        let _ = (page, ticket, accept);
        Err("workspace.unavailable")
    }
}

#[tauri::command]
pub(super) fn workspace_cli_selected(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
    session: String,
    selected: bool,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(all(
        feature = "workspace-cli",
        any(target_os = "linux", target_os = "macos", windows)
    ))]
    {
        let id = state
            .0
            .sessions
            .lock()
            .map_err(|_| "workspace.denied")?
            .ids()
            .into_iter()
            .find(|id| id.get().to_string() == session)
            .ok_or("workspace.denied")?;
        let parsed = uuid::Uuid::parse_str(&ticket).map_err(|_| "workspace.denied")?;
        if parsed.to_string() != ticket {
            return Err("workspace.denied");
        }
        state
            .0
            .selections
            .complete(&page, parsed, id.get(), selected)
            .map_err(|_| "workspace.denied")
    }
    #[cfg(not(all(
        feature = "workspace-cli",
        any(target_os = "linux", target_os = "macos", windows)
    )))]
    {
        let _ = (page, ticket, session, selected);
        Err("workspace.unavailable")
    }
}

#[tauri::command]
pub(super) fn workspace_cli_select_admit(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
    session: String,
) -> Result<()> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    #[cfg(all(
        feature = "workspace-cli",
        any(target_os = "linux", target_os = "macos", windows)
    ))]
    {
        let id = state
            .0
            .sessions
            .lock()
            .map_err(|_| "workspace.denied")?
            .ids()
            .into_iter()
            .find(|id| id.get().to_string() == session)
            .ok_or("workspace.denied")?;
        let parsed = uuid::Uuid::parse_str(&ticket).map_err(|_| "workspace.denied")?;
        if parsed.to_string() != ticket {
            return Err("workspace.denied");
        }
        state
            .0
            .selections
            .admit(&page, parsed, id.get())
            .map_err(|_| "workspace.denied")
    }
    #[cfg(not(all(
        feature = "workspace-cli",
        any(target_os = "linux", target_os = "macos", windows)
    )))]
    {
        let _ = (page, ticket, session);
        Err("workspace.unavailable")
    }
}
