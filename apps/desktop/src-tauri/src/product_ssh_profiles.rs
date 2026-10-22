//! Original-page SSH configuration receipts. Saving identity references is metadata only.
use super::*;
use pulse_store::ssh_profiles::{ProfileEdit, ProfileSnapshot};
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Change {
    Routed(RoutedChange),
    Legacy(LegacyChange),
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LegacyChange {
    id: String,
    label: String,
    username: String,
    identity_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RoutedChange {
    version: u8,
    id: String,
    label: String,
    username: String,
    identity_id: Option<String>,
    via: serde_json::Value,
}
fn id(value: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| "workspace.ssh_profiles_invalid")?;
    if id.to_string() != value || id.get_version_num() != 7 {
        return Err("workspace.ssh_profiles_invalid");
    }
    Ok(id)
}
impl Change {
    fn edit(self) -> Result<(ProfileEdit, Option<Option<Uuid>>)> {
        let (row, jump) = match self {
            Self::Legacy(row) => (row, None),
            Self::Routed(row) => {
                if row.version != 2 {
                    return Err("workspace.ssh_profiles_invalid");
                }
                let jump = match row.via {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(value) => Some(id(&value)?),
                    _ => return Err("workspace.ssh_profiles_invalid"),
                };
                (
                    LegacyChange {
                        id: row.id,
                        label: row.label,
                        username: row.username,
                        identity_id: row.identity_id,
                    },
                    Some(jump),
                )
            }
        };
        Ok((
            ProfileEdit {
                id: id(&row.id)?,
                label: row.label,
                username: row.username,
                identity: row.identity_id.as_deref().map(id).transpose()?,
            },
            jump,
        ))
    }
}
#[derive(Serialize)]
pub(super) struct View {
    token: String,
    data: serde_json::Value,
}
fn projection(token: Uuid, source: &ProfileSnapshot) -> View {
    let hosts = source.hosts();
    let graph = hosts.catalogue();
    let machines: Vec<_> = graph
        .into_iter()
        .flat_map(|g| g.hosts())
        .filter(|h| !h.metadata().deleted())
        .map(|h| serde_json::json!({"id":h.metadata().id(),"label":h.label()}))
        .collect();
    let entrances: Vec<_> = hosts.records().iter().map(|r| serde_json::json!({
        "id":r.id(),"label":r.endpoint().label(),"host":r.endpoint().target().host(),"port":r.endpoint().target().port(),"username":r.endpoint().username(),
        "machineId":graph.and_then(|g| g.endpoints().iter().find(|e| e.metadata().id()==r.id())).map(|e|e.host_id())
    })).collect();
    let profiles: Vec<_> = source.profiles().map(|p| serde_json::json!({"id":p.metadata().id(),"endpointId":p.endpoint_id(),"label":p.label(),"username":p.username(),"identityId":p.identity_id(),"jumpProfileId":p.jump_profile_id()})).collect();
    let identities: Vec<_> = source.password_identities().map(|r| serde_json::json!({"id":r.id(),"kind":"password","label":r.identity().label(),"username":r.identity().username(),"available":r.identity().password().has_value()}))
        .chain(source.key_identities().map(|r| serde_json::json!({"id":r.id(),"kind":"key","label":r.identity().label(),"username":r.identity().username(),"available":r.identity().private_key().has_value() && r.identity().passphrase().is_none_or(|p| p.has_value())})))
        .chain(source.agent_identities().map(|r|serde_json::json!({"id":r.id(),"kind":"agent","label":r.identity().label(),"username":r.identity().username(),"available":true}))).collect();
    View {
        token: token.to_string(),
        data: serde_json::json!({"version":3,"revision":hosts.revision().to_string(),"migrated":graph.is_some(),"machines":machines,"entrances":entrances,"profiles":profiles,"identities":identities}),
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_profiles(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<View> {
    operation(window, state, nonce, None).await
}
#[tauri::command]
pub(super) async fn workspace_ssh_profile_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    change: Change,
) -> Result<View> {
    operation(window, state, nonce, Some((token, change))).await
}
async fn operation(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    change: Option<(String, Change)>,
) -> Result<View> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (window, state, nonce, change);
        Err("workspace.unavailable")
    }
    #[cfg(any(unix, windows))]
    {
        let (_, page) = session::page(&window, &state, &nonce)?;
        let change = if let Some((token, change)) = change {
            let parsed = Uuid::parse_str(&token).map_err(|_| "workspace.denied")?;
            if parsed.to_string() != token {
                return Err("workspace.denied");
            }
            let (edit, jump) = change.edit()?;
            Some((
                state
                    .0
                    .ssh_profiles
                    .take(&state.0.registry, &page, parsed)?,
                edit,
                jump,
            ))
        } else {
            None
        };
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let source = state
            .0
            .profiles
            .ssh_profiles(state.0.registry.clone(), page.clone(), directory, change)
            .await?;
        let (_, current) = session::page(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&page, &current) {
            return Err("workspace.denied");
        }
        let token = state
            .0
            .ssh_profiles
            .publish(&state.0.registry, page, source.clone())?;
        Ok(projection(token, &source))
    }
}
#[cfg(test)]
#[path = "ssh_jump_profile_wire_tests.rs"]
mod tests;
