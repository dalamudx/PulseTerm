//! Current-page Vault metadata/handoff only; password commands are not registered yet.
#[path = "product_vault_handoff.rs"]
pub(super) mod handoff;
use super::*;
use serde::Serialize;
#[cfg(any(unix, windows))]
pub(super) enum Intent {
    Status,
    Lock,
    Create(pulse_store::vault_unlock::PasswordInput),
    Unlock(pulse_store::vault_unlock::PasswordInput),
}
#[derive(Serialize)]
pub(super) struct Status {
    version: u32,
    state: &'static str,
}
impl Status {
    #[cfg(any(unix, windows))]
    pub(super) fn from_key(key: Option<pulse_store::vault_lifecycle::KeyStatus>) -> Self {
        use pulse_store::vault_lifecycle::KeyStatus;
        Self {
            version: 1,
            state: match key {
                None => "missing",
                Some(KeyStatus::Locked) => "locked",
                Some(KeyStatus::Pending) => "pending",
                Some(KeyStatus::Available) => "available",
            },
        }
    }
}
#[cfg(any(unix, windows))]
async fn run(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    intent: Intent,
) -> Result<Status> {
    if window.label() != state.native_label()? {
        return Err("workspace.denied");
    }
    let url = window.url().map_err(|_| "workspace.denied")?;
    let (_, scope) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
    let directory = window
        .app_handle()
        .path()
        .app_data_dir()
        .map_err(|_| "workspace.unavailable")?;
    let value = state
        .0
        .profiles
        .vault(
            state.0.registry.clone(),
            scope.clone(),
            directory,
            state.0.vault.clone(),
            intent,
        )
        .await?;
    let url = window.url().map_err(|_| "workspace.denied")?;
    let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
    if !Arc::ptr_eq(&scope, &current) {
        return Err("workspace.denied");
    }
    #[cfg(debug_assertions)]
    eprintln!("workspace.vault_state_{}", value.state);
    Ok(value)
}
#[tauri::command]
pub(super) async fn workspace_vault_status(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Status> {
    #[cfg(any(unix, windows))]
    {
        run(window, state, nonce, Intent::Status).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
}
#[tauri::command]
pub(super) async fn workspace_vault_lock(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Status> {
    #[cfg(any(unix, windows))]
    {
        run(window, state, nonce, Intent::Lock).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
}
