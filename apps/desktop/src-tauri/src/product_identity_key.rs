//! Receive-only saved-identity key import; no plaintext response, network or session authority.
use super::State;
use super::{
    Workspace,
    identity_password::{canonical, page},
};
use pulse_store::{
    vault_password_ingress::PasswordIngress, vault_private_key_ingress::PrivateKeyIngress,
};
use serde::Serialize;
#[cfg(all(unix, feature = "workspace-ssh"))]
use std::sync::Arc;
#[cfg(all(unix, feature = "workspace-ssh"))]
use tauri::Manager;
use tauri::WebviewWindow;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Serialize)]
pub(super) struct Availability {
    version: u8,
    enabled: bool,
}
#[derive(Serialize)]
pub(super) struct Ticket {
    version: u8,
    ticket: String,
}
#[derive(Serialize)]
pub(super) struct Saved {
    version: u8,
}
#[tauri::command]
pub(super) fn workspace_identity_key_available(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Availability> {
    let _ = page(&window, &state, &nonce)?;
    Ok(Availability {
        version: 1,
        enabled: cfg!(all(unix, feature = "workspace-ssh")),
    })
}
#[tauri::command]
pub(super) async fn workspace_identity_key_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    identity: String,
) -> Result<Ticket> {
    #[cfg(not(all(unix, feature = "workspace-ssh")))]
    {
        let _ = (window, state, nonce, identity);
        Err("workspace.unavailable")
    }
    #[cfg(all(unix, feature = "workspace-ssh"))]
    {
        let scope = page(&window, &state, &nonce)?;
        let identity = canonical(&identity)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let prepared = state
            .0
            .profiles
            .key_prepare(
                state.0.registry.clone(),
                scope.clone(),
                directory,
                state.0.vault.clone(),
                state.0.private_key_writes.clone(),
                identity,
            )
            .await;
        #[cfg(debug_assertions)]
        if prepared.is_err() {
            eprintln!("workspace.identity_key_prepare_rejected");
        }
        let id = prepared?;
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
pub(super) async fn workspace_identity_key_submit(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
    private_key: PrivateKeyIngress,
    passphrase: Option<PasswordIngress>,
) -> Result<Saved> {
    #[cfg(not(all(unix, feature = "workspace-ssh")))]
    {
        let _ = (window, state, nonce, ticket, private_key, passphrase);
        Err("workspace.unavailable")
    }
    #[cfg(all(unix, feature = "workspace-ssh"))]
    {
        let scope = page(&window, &state, &nonce)?;
        let id = canonical(&ticket)?;
        let key = private_key
            .into_credential()
            .map_err(|_| "workspace.invalid_private_key")?;
        let pass = passphrase
            .map(PasswordIngress::into_credential)
            .transpose()
            .map_err(|_| "workspace.invalid_passphrase")?;
        let permit = state
            .0
            .private_key_writes
            .take(&scope, id)
            .map_err(|_| "workspace.vault_denied")?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let submitted = state
            .0
            .profiles
            .key_submit(
                state.0.registry.clone(),
                scope.clone(),
                directory,
                permit,
                key,
                pass,
            )
            .await;
        #[cfg(debug_assertions)]
        if submitted.is_err() {
            eprintln!("workspace.identity_key_submit_rejected");
            if matches!(submitted, Err("workspace.private_key_denied")) {
                eprintln!("workspace.identity_key_validation_rejected");
            } else {
                eprintln!("workspace.identity_key_write_rejected");
            }
        }
        submitted?;
        let current = page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&scope, &current) {
            return Err("workspace.denied");
        }
        #[cfg(debug_assertions)]
        eprintln!("workspace.identity_key_saved");
        Ok(Saved { version: 1 })
    }
}
#[tauri::command]
pub(super) fn workspace_identity_key_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    ticket: String,
) -> Result<()> {
    let scope = page(&window, &state, &nonce)?;
    state
        .0
        .private_key_writes
        .cancel(&scope, canonical(&ticket)?)
        .map_err(|_| "workspace.vault_denied")
}
