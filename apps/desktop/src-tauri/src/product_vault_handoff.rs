//! Current-page metadata tickets and receive-only explicit preview master-password ingress.
#[tauri::command]
pub(in super::super) async fn workspace_vault_submit(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    kind: Kind,
    mode: Mode,
    password: pulse_store::vault_password_ingress::PasswordIngress,
) -> Result<Status> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, token, kind, mode, password);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let scope = page(&window, &state, &nonce)?;
        let id = ticket(&token)?;
        let Mode::MasterPassword = mode;
        let handoff = state
            .0
            .vault_handoff
            .begin(
                id,
                &scope,
                match kind {
                    Kind::Create => pulse_app::vault_handoff::HandoffKind::Create,
                    Kind::Unlock => pulse_app::vault_handoff::HandoffKind::Unlock,
                },
            )
            .map_err(|_| "workspace.vault_denied")?;
        if matches!(kind, Kind::Create) {
            password
                .validate_creation_floor()
                .map_err(|_| "workspace.vault_password_floor")?;
        }
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let intent = match kind {
            Kind::Create => Intent::Create(password.into_password()),
            Kind::Unlock => Intent::Unlock(password.into_password()),
        };
        let result = state
            .0
            .profiles
            .vault_ticket(
                state.0.registry.clone(),
                scope.clone(),
                directory,
                state.0.vault.clone(),
                intent,
                handoff,
            )
            .await;
        if result.is_err() {
            let _cancel = state.0.vault_handoff.cancel(id, &scope);
        }
        page(&window, &state, &nonce)?;
        #[cfg(debug_assertions)]
        match &result {
            Ok(value) if value.state == "locked" => eprintln!("workspace.vault_created_locked"),
            Ok(value) if value.state == "pending" => eprintln!("workspace.vault_unlock_ready"),
            _ => eprintln!("workspace.vault_operation_rejected"),
        }
        result
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in super::super) enum Mode {
    MasterPassword,
}
#[cfg(all(test, unix))]
#[path = "product_vault_handoff_tests.rs"]
mod tests;
use super::*;
#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(in super::super) enum Kind {
    Create,
    Unlock,
}
#[derive(Serialize)]
pub(in super::super) struct Ticket {
    version: u32,
    token: String,
    kind: &'static str,
}
#[cfg(any(unix, windows))]
fn page(
    window: &WebviewWindow,
    state: &Workspace,
    nonce: &str,
) -> Result<Arc<pulse_app::registry::PageLaunchScope>> {
    if window.label() != state.native_label()? {
        return Err("workspace.denied");
    }
    let url = window.url().map_err(|_| "workspace.denied")?;
    current_document(&state.0.documents, state.0.owner, &url, nonce).map(|(_, scope)| scope)
}
#[cfg(any(unix, windows))]
fn ticket(value: &str) -> Result<Uuid> {
    if value.len() != 36 {
        return Err("workspace.vault_denied");
    }
    let id = Uuid::parse_str(value).map_err(|_| "workspace.vault_denied")?;
    if id.is_nil() || id.get_version_num() != 7 || id.to_string() != value {
        return Err("workspace.vault_denied");
    }
    Ok(id)
}
#[tauri::command]
pub(in super::super) async fn workspace_vault_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    kind: Kind,
) -> Result<Ticket> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, kind);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let root = (*state).clone();
        let status = run(window.clone(), state, nonce.clone(), Intent::Status).await?;
        let (mode, name) = match (kind, status.state) {
            (Kind::Create, "missing") => (pulse_app::vault_handoff::HandoffKind::Create, "create"),
            (Kind::Unlock, "locked") => (pulse_app::vault_handoff::HandoffKind::Unlock, "unlock"),
            _ => return Err("workspace.vault_denied"),
        };
        let scope = page(&window, &root, &nonce)?;
        let id = root
            .0
            .vault_handoff
            .prepare(scope, mode)
            .map_err(|_| "workspace.vault_denied")?;
        Ok(Ticket {
            version: 1,
            token: id.to_string(),
            kind: name,
        })
    }
}
#[tauri::command]
pub(in super::super) fn workspace_vault_confirm(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<Status> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, token);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let scope = page(&window, &state, &nonce)?;
        state
            .0
            .vault_handoff
            .confirm(ticket(&token)?, &scope)
            .map_err(|_| "workspace.vault_denied")?;
        #[cfg(debug_assertions)]
        eprintln!("workspace.vault_front_confirmed");
        Ok(Status::from_key(Some(
            pulse_store::vault_lifecycle::KeyStatus::Available,
        )))
    }
}
#[tauri::command]
pub(in super::super) fn workspace_vault_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, token);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let scope = page(&window, &state, &nonce)?;
        state
            .0
            .vault_handoff
            .cancel(ticket(&token)?, &scope)
            .map_err(|_| "workspace.vault_denied")?;
        #[cfg(debug_assertions)]
        eprintln!("workspace.vault_ticket_cancelled");
        Ok(())
    }
}
