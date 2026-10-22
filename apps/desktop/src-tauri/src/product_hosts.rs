//! Metadata editing only; never credentials, key approval or connection authority.
use super::*;
#[cfg(all(test, unix))]
#[path = "product_catalogue_batch_tests.rs"]
mod batch_tests;
#[path = "product_host_captures.rs"]
pub(super) mod captures;
use pulse_core::ssh_endpoint::SshEndpoint;
use pulse_store::ssh_hosts::{HostEdit, HostSnapshot};
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HostDto {
    id: String,
    label: String,
    host: String,
    port: u16,
    username: String,
    machine_id: Option<String>,
    profile: Option<ProfileDto>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileDto {
    id: String,
    label: String,
    identity_id: Option<String>,
    via: Option<String>,
}
#[derive(serde::Serialize)]
struct MachineDto {
    id: String,
    label: String,
}
#[derive(serde::Serialize)]
pub(super) struct CatalogueDto {
    version: u8,
    token: String,
    revision: String,
    hosts: Vec<HostDto>,
    machines: Vec<MachineDto>,
    migrated: bool,
}
#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    MigrateCatalogue,
    RenameMachine {
        id: String,
        label: String,
    },
    Attach {
        #[serde(rename = "machineId")]
        machine_id: String,
        label: String,
        host: String,
        port: u16,
        username: String,
    },
    Create {
        label: String,
        host: String,
        port: u16,
        username: String,
    },
    Update {
        id: String,
        label: String,
        host: String,
        port: u16,
        username: String,
    },
    Remove {
        id: String,
    },
    RemoveMany {
        ids: Vec<String>,
    },
}
impl Change {
    fn edit(self) -> Result<HostEdit> {
        let endpoint = |label: &str, host: &str, port: u16, username: &str| {
            SshEndpoint::new(label, host, port, username).map_err(|_| "workspace.invalid_host")
        };
        let identity = |id: &str| {
            let uuid = Uuid::parse_str(id).map_err(|_| "workspace.invalid_host")?;
            if uuid.get_version_num() != 7
                || uuid.get_variant() != uuid::Variant::RFC4122
                || uuid.to_string() != id
            {
                return Err("workspace.invalid_host");
            }
            Ok(uuid)
        };
        match self {
            Self::MigrateCatalogue => Ok(HostEdit::MigrateCatalogue),
            Self::RenameMachine { id, label } => Ok(HostEdit::RenameMachine(identity(&id)?, label)),
            Self::Attach {
                machine_id,
                label,
                host,
                port,
                username,
            } => Ok(HostEdit::AttachSshEndpoint(
                identity(&machine_id)?,
                endpoint(&label, &host, port, &username)?,
            )),
            Self::Create {
                label,
                host,
                port,
                username,
            } => Ok(HostEdit::Create(endpoint(&label, &host, port, &username)?)),
            Self::Update {
                id,
                label,
                host,
                port,
                username,
            } => Ok(HostEdit::Update(
                identity(&id)?,
                endpoint(&label, &host, port, &username)?,
            )),
            Self::Remove { id } => Ok(HostEdit::Remove(identity(&id)?)),
            Self::RemoveMany { ids } => Ok(HostEdit::RemoveMany(batch_ids(ids)?)),
        }
    }
}
pub(super) fn batch_ids(ids: Vec<String>) -> Result<Vec<Uuid>> {
    if ids.is_empty() || ids.len() > 256 {
        return Err("workspace.invalid_selection");
    }
    let mut unique = std::collections::BTreeSet::new();
    ids.into_iter()
        .map(|value| {
            let id = Uuid::parse_str(&value).map_err(|_| "workspace.invalid_selection")?;
            if id.get_version_num() != 7
                || id.get_variant() != uuid::Variant::RFC4122
                || id.to_string() != value
                || !unique.insert(id)
            {
                return Err("workspace.invalid_selection");
            }
            Ok(id)
        })
        .collect()
}
fn projection(token: Uuid, snapshot: HostSnapshot) -> CatalogueDto {
    CatalogueDto {
        version: 4,
        machines: snapshot.catalogue().map_or_else(Vec::new, |g| {
            g.hosts()
                .iter()
                .filter(|h| !h.metadata().deleted())
                .map(|h| MachineDto {
                    id: h.metadata().id().to_string(),
                    label: h.label().to_owned(),
                })
                .collect()
        }),
        migrated: snapshot.catalogue().is_some(),
        token: token.to_string(),
        revision: snapshot.revision().to_string(),
        hosts: snapshot
            .records()
            .iter()
            .map(|r| HostDto {
                profile: snapshot
                    .catalogue()
                    .and_then(|g| {
                        g.profiles()
                            .iter()
                            .find(|p| !p.metadata().deleted() && p.endpoint_id() == r.id())
                    })
                    .map(|p| ProfileDto {
                        id: p.metadata().id().to_string(),
                        label: p.label().to_owned(),
                        identity_id: p.identity_id().map(|id| id.to_string()),
                        via: p.jump_profile_id().map(|id| id.to_string()),
                    }),
                id: r.id().to_string(),
                label: r.endpoint().label().to_owned(),
                host: r.endpoint().target().host().to_owned(),
                port: r.endpoint().target().port(),
                username: r.endpoint().username().to_owned(),
                machine_id: snapshot
                    .catalogue()
                    .and_then(|g| g.endpoints().iter().find(|e| e.metadata().id() == r.id()))
                    .map(|e| e.host_id().to_string()),
            })
            .collect(),
    }
}
#[tauri::command]
pub(super) async fn workspace_hosts(
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
            .hosts(state.0.registry.clone(), scope.clone(), directory)
            .await?;
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (_, current) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        if !Arc::ptr_eq(&scope, &current) {
            return Err("workspace.denied");
        }
        let token = state
            .0
            .hosts
            .publish(&state.0.registry, scope, snapshot.clone())?;
        eprintln!("workspace.hosts_operation_confirmed");
        Ok(projection(token, snapshot))
    }
}
#[tauri::command]
pub(super) async fn workspace_host_save(
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
        let edit = change.edit()?;
        let ticket = Uuid::parse_str(&token).map_err(|_| "workspace.denied")?;
        let expected = state.0.hosts.take(&state.0.registry, &scope, ticket)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let snapshot = state
            .0
            .profiles
            .edit_host(
                state.0.registry.clone(),
                scope.clone(),
                directory,
                expected,
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
            .hosts
            .publish(&state.0.registry, scope, snapshot.clone())?;
        eprintln!("workspace.hosts_operation_confirmed");
        Ok(projection(token, snapshot))
    }
}
