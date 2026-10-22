//! Preview receive-only saved-identity password edit; no credential read or SSH target authorization.
use super::*;
use pulse_app::registry::PageLaunchScope;
use pulse_store::vault_password_ingress::PasswordIngress;
use std::sync::Arc;
#[derive(serde::Serialize)]
pub(super) struct Ticket {
    version: u8,
    ticket: String,
}
#[derive(serde::Serialize)]
pub(super) struct Saved {
    version: u8,
}
pub(super) fn canonical(value: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| "workspace.invalid_ticket")?;
    if id.is_nil() || id.to_string() != value {
        return Err("workspace.invalid_ticket");
    }
    Ok(id)
}
pub(super) fn page(
    window: &WebviewWindow,
    state: &Workspace,
    nonce: &str,
) -> Result<Arc<PageLaunchScope>> {
    if window.label() != state.native_label()? {
        return Err("workspace.denied");
    }
    let url = window.url().map_err(|_| "workspace.denied")?;
    let (_, scope) = current_document(&state.0.documents, state.0.owner, &url, nonce)?;
    Ok(scope)
}
#[tauri::command]
pub(super) async fn workspace_identity_password_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    identity: String,
) -> Result<Ticket> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, identity);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let scope = page(&window, &state, &nonce)?;
        let id = canonical(&identity)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let id = state
            .0
            .profiles
            .password_prepare(
                state.0.registry.clone(),
                scope.clone(),
                directory,
                state.0.vault.clone(),
                state.0.password_writes.clone(),
                id,
            )
            .await
            .inspect_err(|_error| {
                #[cfg(debug_assertions)]
                if page(&window, &state, &nonce)
                    .ok()
                    .is_some_and(|current| Arc::ptr_eq(&scope, &current))
                {
                    eprintln!("workspace.identity_password_prepare_rejected");
                }
            })?;
        let current = page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&scope, &current) {
            return Err("workspace.denied");
        }
        Ok(Ticket {
            version: 1,
            ticket: id.to_string(),
        })
    }
}
#[tauri::command]
pub(super) async fn workspace_identity_password_submit(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
    password: PasswordIngress,
) -> Result<Saved> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, ticket, password);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let scope = page(&window, &state, &nonce)?;
        let id = canonical(&ticket)?;
        let plain = password
            .into_credential()
            .map_err(|_| "workspace.invalid_password")?;
        let permit = state
            .0
            .password_writes
            .take(&scope, id)
            .map_err(|_| "workspace.vault_denied")?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        state
            .0
            .profiles
            .password_submit(
                state.0.registry.clone(),
                scope.clone(),
                directory,
                permit,
                plain,
            )
            .await?;
        let current = page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&scope, &current) {
            return Err("workspace.denied");
        }
        #[cfg(debug_assertions)]
        eprintln!("workspace.identity_password_saved");
        Ok(Saved { version: 1 })
    }
}
#[tauri::command]
pub(super) fn workspace_identity_password_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
) -> Result<()> {
    let scope = page(&window, &state, &nonce)?;
    state
        .0
        .password_writes
        .cancel(&scope, canonical(&ticket)?)
        .map_err(|_| "workspace.vault_denied")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_canonical_non_nil_metadata_ticket_ids() {
        for bad in [
            "",
            "FFFFFFFF-FFFF-4FFF-8FFF-FFFFFFFFFFFF",
            "00000000-0000-0000-0000-000000000000",
            "credential/path",
        ] {
            assert!(canonical(bad).is_err());
        }
        let id = Uuid::new_v4();
        assert_eq!(canonical(&id.to_string()), Ok(id));
    }
}
