//! Current-page identity metadata; never password input, raw reference selection or SSH authority.
#[cfg(all(test, unix))]
#[path = "product_identity_admission_tests.rs"]
mod tests;
use super::*;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::ssh_identities::{IdentityEdit, IdentitySnapshot};
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SecretDto {
    id: String,
    kind: &'static str,
    has_value: bool,
}
#[derive(serde::Serialize)]
pub(super) struct IdentityDto {
    id: String,
    label: String,
    username: String,
    secret: SecretDto,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CatalogueDto {
    version: u8,
    token: String,
    revision: String,
    vault_present: bool,
    identities: Vec<IdentityDto>,
}
#[derive(serde::Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    Create {
        label: String,
        username: String,
    },
    Update {
        id: String,
        label: String,
        username: String,
    },
    Remove {
        id: String,
    },
    RemoveMany {
        ids: Vec<String>,
    },
}
fn ticket(value: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| "workspace.denied")?;
    if id.get_version_num() != 4
        || id.get_variant() != uuid::Variant::RFC4122
        || id.to_string() != value
    {
        return Err("workspace.denied");
    }
    Ok(id)
}
impl Change {
    fn edit(self, snapshot: &IdentitySnapshot) -> Result<IdentityEdit> {
        let id = |value: &str| {
            let id = Uuid::parse_str(value).map_err(|_| "workspace.invalid_identity")?;
            if id.get_version_num() != 7
                || id.get_variant() != uuid::Variant::RFC4122
                || id.to_string() != value
            {
                return Err("workspace.invalid_identity");
            }
            Ok(id)
        };
        let identity = |label: &str, username: &str, secret: SecretRef| {
            SshPasswordIdentity::new(label, username, secret)
                .map_err(|_| "workspace.invalid_identity")
        };
        match self {
            Self::Create { label, username } => {
                let vault = snapshot.vault_id().ok_or("workspace.vault_missing")?;
                let secret = SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)
                    .map_err(|_| "workspace.invalid_identity")?;
                Ok(IdentityEdit::Create(identity(&label, &username, secret)?))
            }
            Self::Update {
                id: value,
                label,
                username,
            } => {
                let id = id(&value)?;
                let row = snapshot
                    .records()
                    .find(|row| row.id() == id)
                    .ok_or("workspace.invalid_identity")?;
                Ok(IdentityEdit::Update(
                    id,
                    identity(&label, &username, row.identity().password())?,
                ))
            }
            Self::RemoveMany { ids } => {
                let ids = super::hosts::batch_ids(ids)?;
                if ids
                    .iter()
                    .any(|id| !snapshot.records().any(|row| row.id() == *id))
                {
                    return Err("workspace.invalid_identity");
                }
                Ok(IdentityEdit::RemoveMany(ids))
            }
            Self::Remove { id: value } => {
                let id = id(&value)?;
                if !snapshot.records().any(|row| row.id() == id) {
                    return Err("workspace.invalid_identity");
                }
                Ok(IdentityEdit::Remove(id))
            }
        }
    }
}
fn projection(token: Uuid, snapshot: IdentitySnapshot) -> CatalogueDto {
    CatalogueDto {
        version: 1,
        token: token.to_string(),
        revision: snapshot.revision().to_string(),
        vault_present: snapshot.vault_id().is_some(),
        identities: snapshot
            .records()
            .map(|row| {
                let value = row.identity();
                let secret = value.password();
                IdentityDto {
                    id: row.id().to_string(),
                    label: value.label().to_owned(),
                    username: value.username().to_owned(),
                    secret: SecretDto {
                        id: secret.id().to_string(),
                        kind: secret.kind().as_str(),
                        has_value: secret.has_value(),
                    },
                }
            })
            .collect(),
    }
}
#[tauri::command]
pub(super) async fn workspace_identities(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<CatalogueDto> {
    #[cfg(not(unix))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
    #[cfg(unix)]
    {
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
        let snapshot = state
            .0
            .profiles
            .identities(state.0.registry.clone(), scope.clone(), directory)
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&scope, &current) {
            return Err("workspace.denied");
        }
        let token = state
            .0
            .identities
            .publish(&state.0.registry, scope, snapshot.clone())?;
        eprintln!("workspace.identities_operation_confirmed");
        Ok(projection(token, snapshot))
    }
}
#[tauri::command]
pub(super) async fn workspace_identity_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    change: Change,
) -> Result<CatalogueDto> {
    #[cfg(not(unix))]
    {
        let _ = (window, state, nonce, token, change);
        Err("workspace.unavailable")
    }
    #[cfg(unix)]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, scope) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let token = ticket(&token)?;
        let snapshot = state.0.identities.take(&state.0.registry, &scope, token)?;
        let edit = change.edit(&snapshot)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let snapshot = state
            .0
            .profiles
            .edit_identity(
                state.0.registry.clone(),
                scope.clone(),
                directory,
                snapshot,
                edit,
            )
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&scope, &current) {
            return Err("workspace.denied");
        }
        let token = state
            .0
            .identities
            .publish(&state.0.registry, scope, snapshot.clone())?;
        eprintln!("workspace.identities_operation_confirmed");
        Ok(projection(token, snapshot))
    }
}
