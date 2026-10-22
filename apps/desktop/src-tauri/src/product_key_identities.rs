//! Original-page private-key metadata only; no key bytes, paths, raw references, unlock or SSH permission.
#[cfg(all(test, unix))]
#[path = "product_key_identity_admission_tests.rs"]
mod tests;
use super::*;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_key_identity::SshPrivateKeyIdentity,
};
use pulse_store::ssh_key_identities::{KeyIdentityEdit, KeyIdentitySnapshot};
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SecretDto {
    id: String,
    kind: &'static str,
    has_value: bool,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IdentityDto {
    id: String,
    label: String,
    username: String,
    secret: SecretDto,
    passphrase: Option<SecretDto>,
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
        #[serde(default, rename = "withPassphrase")]
        with_passphrase: bool,
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
    fn edit(self, snapshot: &KeyIdentitySnapshot) -> Result<KeyIdentityEdit> {
        let id = |value: &str| {
            let id = Uuid::parse_str(value).map_err(|_| "workspace.invalid_key_identity")?;
            if id.get_version_num() != 7
                || id.get_variant() != uuid::Variant::RFC4122
                || id.to_string() != value
            {
                return Err("workspace.invalid_key_identity");
            }
            Ok(id)
        };
        let identity = |label: &str, username: &str, key: SecretRef, pass: Option<SecretRef>| {
            SshPrivateKeyIdentity::new(label, username, key, pass)
                .map_err(|_| "workspace.invalid_key_identity")
        };
        match self {
            Self::Create {
                label,
                username,
                with_passphrase,
            } => {
                let vault = snapshot.vault_id().ok_or("workspace.vault_missing")?;
                let key = SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPrivateKey, false)
                    .map_err(|_| "workspace.invalid_key_identity")?;
                let pass = with_passphrase
                    .then(|| {
                        SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshKeyPassphrase, false)
                    })
                    .transpose()
                    .map_err(|_| "workspace.invalid_key_identity")?;
                Ok(KeyIdentityEdit::Create(identity(
                    &label, &username, key, pass,
                )?))
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
                    .ok_or("workspace.invalid_key_identity")?;
                Ok(KeyIdentityEdit::Update(
                    id,
                    identity(
                        &label,
                        &username,
                        row.identity().private_key(),
                        row.identity().passphrase(),
                    )?,
                ))
            }
            Self::RemoveMany { ids } => {
                let ids = super::hosts::batch_ids(ids)?;
                if ids
                    .iter()
                    .any(|id| !snapshot.records().any(|row| row.id() == *id))
                {
                    return Err("workspace.invalid_key_identity");
                }
                Ok(KeyIdentityEdit::RemoveMany(ids))
            }
            Self::Remove { id: value } => {
                let id = id(&value)?;
                if !snapshot.records().any(|row| row.id() == id) {
                    return Err("workspace.invalid_key_identity");
                }
                Ok(KeyIdentityEdit::Remove(id))
            }
        }
    }
}
fn projection(token: Uuid, snapshot: KeyIdentitySnapshot) -> CatalogueDto {
    let secret = |value: SecretRef| SecretDto {
        id: value.id().to_string(),
        kind: value.kind().as_str(),
        has_value: value.has_value(),
    };
    CatalogueDto {
        version: 1,
        token: token.to_string(),
        revision: snapshot.revision().to_string(),
        vault_present: snapshot.vault_id().is_some(),
        identities: snapshot
            .records()
            .map(|row| {
                let value = row.identity();
                IdentityDto {
                    id: row.id().to_string(),
                    label: value.label().to_owned(),
                    username: value.username().to_owned(),
                    secret: secret(value.private_key()),
                    passphrase: value.passphrase().map(secret),
                }
            })
            .collect(),
    }
}
#[tauri::command]
pub(super) async fn workspace_key_identities(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<CatalogueDto> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
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
            .key_identities(state.0.registry.clone(), scope.clone(), directory)
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&scope, &current) {
            return Err("workspace.denied");
        }
        let token = state
            .0
            .key_identities
            .publish(&state.0.registry, scope, snapshot.clone())?;
        Ok(projection(token, snapshot))
    }
}
#[tauri::command]
pub(super) async fn workspace_key_identity_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    change: Change,
) -> Result<CatalogueDto> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, token, change);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        if window.label() != state.native_label()? {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, scope) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let snapshot = state
            .0
            .key_identities
            .take(&state.0.registry, &scope, ticket(&token)?)?;
        let change = change.edit(&snapshot)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let snapshot = state
            .0
            .profiles
            .edit_key_identity(
                state.0.registry.clone(),
                scope.clone(),
                directory,
                snapshot,
                change,
            )
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&scope, &current) {
            return Err("workspace.denied");
        }
        let token = state
            .0
            .key_identities
            .publish(&state.0.registry, scope, snapshot.clone())?;
        Ok(projection(token, snapshot))
    }
}
