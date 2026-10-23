//! Current-page action metadata. Fixed native CLI focus is separate; no WebView cap_invoke.
use super::State;
use super::{Result, Workspace, session};
use pulse_app::registry::{PageLaunchScope, SessionRegistry};
use pulse_cap::{ActionArgs, ActionDef, Builder, Catalogue, Risk, Scope, Surface};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use tauri::WebviewWindow;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}
impl ActionArgs for EmptyArgs {
    fn validate(&self) -> std::result::Result<(), pulse_cap::Error> {
        Ok(())
    }
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NewTabArgs {
    #[serde(default, deserialize_with = "optional_cli_profile")]
    #[schemars(with = "String")]
    profile: Option<String>,
    #[serde(default, deserialize_with = "optional_cli_profile")]
    #[schemars(with = "String")]
    cwd: Option<String>,
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SelectionArgs {
    session: String,
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
impl ActionArgs for SelectionArgs {
    fn validate(&self) -> std::result::Result<(), pulse_cap::Error> {
        let id = self
            .session
            .parse::<u64>()
            .map_err(|_| pulse_cap::Error::Args)?;
        if id == 0 || id.to_string() != self.session {
            return Err(pulse_cap::Error::Args);
        }
        Ok(())
    }
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
impl ActionArgs for NewTabArgs {
    fn validate(&self) -> std::result::Result<(), pulse_cap::Error> {
        let valid = match (self.profile.as_deref(), self.cwd.as_deref()) {
            (Some(profile), None) => {
                pulse_store::profile_default::DefaultProfileSelection::parse(profile).is_some()
            }
            (None, Some(cwd)) => pulse_app::local_launch::LaunchDirectory::parse(cwd).is_some(),
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(pulse_cap::Error::Args)
        }
    }
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SplitDirection {
    Row,
    Column,
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
fn optional_cli_profile<'de, D: serde::Deserializer<'de>>(
    value: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(value).map(Some)
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SplitArgs {
    #[serde(default, deserialize_with = "optional_cli_profile")]
    #[schemars(with = "String")]
    profile: Option<String>,
    direction: SplitDirection,
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
impl ActionArgs for SplitArgs {
    fn validate(&self) -> std::result::Result<(), pulse_cap::Error> {
        let _ = &self.direction;
        match self.profile.as_deref() {
            Some(profile) => pulse_store::profile_default::DefaultProfileSelection::parse(profile)
                .map(|_| ())
                .ok_or(pulse_cap::Error::Args),
            None => Ok(()),
        }
    }
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SshAuthentication {
    Interactive,
    #[serde(rename = "key-interactive")]
    KeyInteractive,
    #[serde(rename = "agent-interactive")]
    AgentInteractive,
    #[serde(rename = "password-interactive")]
    PasswordInteractive,
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
impl SshAuthentication {
    fn present<'de, D: serde::Deserializer<'de>>(
        value: D,
    ) -> std::result::Result<Option<Self>, D::Error> {
        Self::deserialize(value).map(Some)
    }
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
#[derive(Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
#[schemars(transform = closed_saved_ssh_schema)]
enum SavedSshArgs {
    Ids {
        host: String,
        identity: String,
        #[serde(default, deserialize_with = "SshAuthentication::present")]
        authentication: Option<SshAuthentication>,
    },
    Alias {
        alias: String,
    },
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
fn closed_saved_ssh_schema(schema: &mut schemars::Schema) {
    // The catalogue requires an object root. Preserve the generated closed alternatives
    // while declaring their full property union on the equally closed object envelope.
    let _ = schema.insert("type".into(), serde_json::json!("object"));
    let _ = schema.insert("additionalProperties".into(), serde_json::json!(false));
    let _ = schema.insert(
        "properties".into(),
        serde_json::json!({
            "host": {"type": "string"},
            "identity": {"type": "string"},
            "authentication": {"type": "string", "enum": ["interactive", "key-interactive", "agent-interactive", "password-interactive"]},
            "alias": {"type": "string"},
        }),
    );
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
impl ActionArgs for SavedSshArgs {
    fn validate(&self) -> std::result::Result<(), pulse_cap::Error> {
        match self {
            Self::Ids {
                host,
                identity,
                authentication,
            } => {
                if !matches!(
                    authentication,
                    None | Some(
                        SshAuthentication::Interactive
                            | SshAuthentication::KeyInteractive
                            | SshAuthentication::AgentInteractive
                            | SshAuthentication::PasswordInteractive
                    )
                ) {
                    return Err(pulse_cap::Error::Args);
                }
                for text in [host, identity] {
                    let id = uuid::Uuid::parse_str(text).map_err(|_| pulse_cap::Error::Args)?;
                    if id.get_version_num() != 7
                        || id.get_variant() != uuid::Variant::RFC4122
                        || id.to_string() != *text
                    {
                        return Err(pulse_cap::Error::Args);
                    }
                }
            }
            Self::Alias { alias } => {
                pulse_core::ssh_endpoint::SshEndpoint::new(alias, "localhost", 22, "metadata")
                    .map_err(|_| pulse_cap::Error::Args)?;
            }
        }
        Ok(())
    }
}
pub(super) fn catalogue(ssh_enabled: bool) -> Result<Catalogue> {
    let mut builder = Builder::default();
    for (id, title, risk) in [
        ("profiles", "workspace.commands.profiles", Risk::Safe),
        ("settings", "workspace.commands.settings", Risk::Safe),
        ("find", "workspace.commands.find", Risk::Safe),
        ("hosts", "workspace.commands.hosts", Risk::Safe),
        ("identities", "workspace.commands.identities", Risk::Safe),
        ("vault", "workspace.commands.vault", Risk::Safe),
        ("ssh", "workspace.commands.ssh", Risk::Safe),
        (
            "close-session",
            "workspace.commands.close-session",
            Risk::Confirm,
        ),
    ] {
        if id == "ssh" && !ssh_enabled {
            continue;
        }
        builder
            .register::<EmptyArgs>(ActionDef {
                id,
                title,
                scope: Scope::Window,
                risk,
                surfaces: &[Surface::Palette, Surface::Keybinding],
            })
            .map_err(|_| "workspace.cap_unavailable")?;
    }
    for (id, title, ssh_only) in [
        ("keybindings", "workspace.commands.keybindings", false),
        ("organization", "workspace.commands.organization", false),
        ("private-keys", "workspace.commands.private-keys", true),
        (
            "agent-identities",
            "workspace.commands.agent-identities",
            true,
        ),
        ("quick-connect", "workspace.commands.quick-connect", true),
        ("ssh-config", "workspace.commands.ssh-config", true),
        ("split-row", "workspace.commands.split-row", false),
        ("split-column", "workspace.commands.split-column", false),
        (
            "duplicate-session",
            "workspace.commands.duplicate-session",
            false,
        ),
    ] {
        if ssh_only && !ssh_enabled {
            continue;
        }
        builder
            .register::<EmptyArgs>(ActionDef {
                id,
                title,
                scope: Scope::Window,
                risk: Risk::Safe,
                surfaces: &[Surface::Palette],
            })
            .map_err(|_| "workspace.cap_unavailable")?;
    }
    builder
        .register::<EmptyArgs>(ActionDef {
            id: "palette",
            title: "workspace.commands.palette",
            scope: Scope::Window,
            risk: Risk::Safe,
            surfaces: &[Surface::Keybinding],
        })
        .map_err(|_| "workspace.cap_unavailable")?;
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    builder
        .register::<EmptyArgs>(ActionDef {
            id: "workspace.windows",
            title: "workspace.commands.windows",
            scope: Scope::Global,
            risk: Risk::Safe,
            surfaces: &[Surface::Cli],
        })
        .map_err(|_| "workspace.cap_unavailable")?;
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    for (id, title) in [
        ("workspace.focus", "workspace.commands.focus"),
        ("workspace.profiles", "workspace.commands.profiles"),
    ] {
        builder
            .register::<EmptyArgs>(ActionDef {
                id,
                title,
                scope: Scope::Window,
                risk: Risk::Safe,
                surfaces: &[Surface::Cli],
            })
            .map_err(|_| "workspace.cap_unavailable")?;
    }
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    builder
        .register::<EmptyArgs>(ActionDef {
            id: "workspace.new-window",
            title: "workspace.commands.new-window",
            scope: Scope::Window,
            risk: Risk::Confirm,
            surfaces: &[Surface::Cli],
        })
        .map_err(|_| "workspace.cap_unavailable")?;
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    builder
        .register::<SelectionArgs>(ActionDef {
            id: "workspace.focus-tab",
            title: "workspace.commands.focus-tab",
            scope: Scope::Window,
            risk: Risk::Safe,
            surfaces: &[Surface::Cli],
        })
        .map_err(|_| "workspace.cap_unavailable")?;
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    builder
        .register::<NewTabArgs>(ActionDef {
            id: "workspace.new-tab",
            title: "workspace.commands.new-tab",
            scope: Scope::Window,
            risk: Risk::Confirm,
            surfaces: &[Surface::Cli],
        })
        .map_err(|_| "workspace.cap_unavailable")?;
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    builder
        .register::<SplitArgs>(ActionDef {
            id: "workspace.split",
            title: "workspace.commands.split",
            scope: Scope::Window,
            risk: Risk::Confirm,
            surfaces: &[Surface::Cli],
        })
        .map_err(|_| "workspace.cap_unavailable")?;
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    if ssh_enabled {
        builder
            .register::<SavedSshArgs>(ActionDef {
                id: "workspace.ssh",
                title: "workspace.commands.ssh",
                scope: Scope::Window,
                risk: Risk::Confirm,
                surfaces: &[Surface::Cli],
            })
            .map_err(|_| "workspace.cap_unavailable")?;
    }
    builder.finish().map_err(|_| "workspace.cap_unavailable")
}
fn project_current(
    registry: &SessionRegistry,
    page: &PageLaunchScope,
    catalogue: &Catalogue,
) -> Result<Value> {
    registry
        .check_launch_page(page)
        .map_err(|_| "workspace.cap_forbidden")?;
    let value = catalogue
        .projection(Surface::Palette)
        .map_err(|_| "workspace.cap_unavailable")?;
    registry
        .check_launch_page(page)
        .map_err(|_| "workspace.cap_forbidden")?;
    Ok(value)
}
#[tauri::command]
pub(super) fn workspace_cap_list(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Value> {
    let (_, page) = session::page(&window, &state, &nonce)?;
    let catalogue = catalogue(cfg!(feature = "workspace-ssh"))?;
    let value = project_current(&state.0.registry, &page, &catalogue)?;
    eprintln!(
        "workspace.cap_list_returned.{}",
        value["actions"].as_array().map_or(0, Vec::len)
    );
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    use pulse_app::product_document::ProductDocuments;
    #[test]
    fn default_and_ssh_catalogues_expose_only_existing_ui_selection_actions()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        for (enabled, count) in [(false, 12), (true, 17)] {
            let catalogue = catalogue(enabled).map_err(std::io::Error::other)?;
            let value = catalogue.projection(Surface::Palette)?;
            assert_eq!(value["actions"].as_array().map(Vec::len), Some(count));
            let keybindings = catalogue.projection(Surface::Keybinding)?;
            assert_eq!(
                keybindings["actions"].as_array().map(Vec::len),
                Some(8 + usize::from(enabled))
            );
            for (id, ssh_only) in [
                ("keybindings", false),
                ("organization", false),
                ("private-keys", true),
                ("agent-identities", true),
                ("quick-connect", true),
                ("ssh-config", true),
                ("split-row", false),
                ("split-column", false),
                ("duplicate-session", false),
            ] {
                if !ssh_only || enabled {
                    assert_eq!(
                        catalogue.policy(id, Surface::Palette)?,
                        (Scope::Window, Risk::Safe)
                    );
                    catalogue.validate_args(id, Surface::Palette, serde_json::json!({}))?;
                    assert!(
                        catalogue
                            .validate_args(
                                id,
                                Surface::Palette,
                                serde_json::json!({"path":"/bin/sh"})
                            )
                            .is_err()
                    );
                } else {
                    assert!(catalogue.policy(id, Surface::Palette).is_err());
                }
                assert!(catalogue.policy(id, Surface::Cli).is_err());
                assert!(catalogue.policy(id, Surface::Keybinding).is_err());
            }
            let cli = catalogue.projection(Surface::Cli)?;
            #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
            {
                // This factory test explicitly supplies availability; production
                // callers instead pass cfg!(feature = "workspace-ssh").
                let cli_count = 7 + usize::from(enabled);
                assert_eq!(cli["actions"].as_array().map(Vec::len), Some(cli_count));
                let mut expected = [
                    "workspace.focus",
                    "workspace.profiles",
                    "workspace.focus-tab",
                    "workspace.new-tab",
                    "workspace.split",
                    "workspace.windows",
                    "workspace.new-window",
                ]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>();
                if enabled {
                    expected.insert("workspace.ssh");
                }
                let ids = cli["actions"]
                    .as_array()
                    .ok_or("fixture.actions")?
                    .iter()
                    .map(|entry| entry["id"].as_str().ok_or("fixture.id"))
                    .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()?;
                assert_eq!(ids, expected);
                assert_eq!(
                    catalogue.policy("workspace.windows", Surface::Cli)?,
                    (Scope::Global, Risk::Safe)
                );
                assert_eq!(
                    catalogue.policy("workspace.new-window", Surface::Cli)?,
                    (Scope::Window, Risk::Confirm)
                );
                assert_eq!(
                    catalogue.policy("workspace.profiles", Surface::Cli)?,
                    (Scope::Window, Risk::Safe)
                );
                assert_eq!(
                    catalogue.validate_args(
                        "workspace.profiles",
                        Surface::Cli,
                        serde_json::json!({"program":"/bin/sh"})
                    ),
                    Err(pulse_cap::Error::Args)
                );
                assert_eq!(
                    catalogue.validate_args(
                        "workspace.profiles",
                        Surface::Palette,
                        serde_json::json!({})
                    ),
                    Err(pulse_cap::Error::Unavailable)
                );
                assert_eq!(
                    catalogue.policy("workspace.focus", Surface::Cli)?,
                    (Scope::Window, Risk::Safe)
                );
                assert_eq!(
                    catalogue.validate_args(
                        "workspace.focus",
                        Surface::Cli,
                        serde_json::json!({"path":"/bin/sh"})
                    ),
                    Err(pulse_cap::Error::Args)
                );
                assert_eq!(
                    catalogue.validate_args(
                        "workspace.focus",
                        Surface::Palette,
                        serde_json::json!({})
                    ),
                    Err(pulse_cap::Error::Unavailable)
                );
            }
            #[cfg(not(all(feature = "workspace-cli", any(target_os = "linux", windows))))]
            assert_eq!(cli["actions"], serde_json::json!([]));
            assert_eq!(
                catalogue.validate_args("settings", Surface::Cli, serde_json::json!({})),
                Err(pulse_cap::Error::Unavailable)
            );
            assert_eq!(
                catalogue.validate_args(
                    "settings",
                    Surface::Palette,
                    serde_json::json!({"path":"/bin/sh"})
                ),
                Err(pulse_cap::Error::Args)
            );
            assert_eq!(
                catalogue.policy("close-session", Surface::Palette)?,
                (Scope::Window, Risk::Confirm)
            );
        }
        Ok(())
    }
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    #[test]
    fn actual_build_catalogue_has_no_bootstrap_capability_or_disabled_ssh_action()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let enabled = cfg!(feature = "workspace-ssh");
        let catalogue = catalogue(enabled).map_err(std::io::Error::other)?;
        let cli = catalogue.projection(Surface::Cli)?;
        assert_eq!(
            cli["actions"].as_array().map(Vec::len),
            Some(7 + usize::from(enabled))
        );
        for surface in [Surface::Cli, Surface::Palette, Surface::Keybinding] {
            assert_eq!(
                catalogue.policy("workspace.bootstrap", surface),
                Err(pulse_cap::Error::Unavailable)
            );
        }
        if !enabled {
            assert_eq!(
                catalogue.policy("workspace.ssh", Surface::Cli),
                Err(pulse_cap::Error::Unavailable)
            );
        }
        Ok(())
    }

    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    #[test]
    fn saved_alias_is_confirm_only_closed_metadata_with_no_identity_or_raw_target_mix()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let catalogue = catalogue(true).map_err(std::io::Error::other)?;
        assert_eq!(
            catalogue.policy("workspace.ssh", Surface::Cli)?,
            (Scope::Window, Risk::Confirm)
        );
        let host = uuid::Uuid::now_v7().to_string();
        let identity = uuid::Uuid::now_v7().to_string();
        for args in [
            serde_json::json!({"alias":"入口 Alpha"}),
            serde_json::json!({"host":host,"identity":identity}),
        ] {
            catalogue.validate_args("workspace.ssh", Surface::Cli, args)?;
        }
        for args in [
            serde_json::json!({"alias":"GUI_HOST","host":host,"identity":identity}),
            serde_json::json!({"alias":"GUI_HOST","address":"127.0.0.1"}),
            serde_json::json!({"alias":"GUI_HOST","confirm":true}),
            serde_json::json!({"alias":""}),
            serde_json::json!({"alias":" GUI_HOST"}),
            serde_json::json!({"alias":"GUI\nHOST"}),
            serde_json::json!({"alias":"x".repeat(257)}),
            serde_json::json!({"alias":"中".repeat(86)}),
        ] {
            assert_eq!(
                catalogue.validate_args("workspace.ssh", Surface::Cli, args),
                Err(pulse_cap::Error::Args)
            );
        }
        for surface in [Surface::Palette, Surface::Keybinding] {
            assert_eq!(
                catalogue.policy("workspace.ssh", surface),
                Err(pulse_cap::Error::Unavailable)
            );
        }
        Ok(())
    }
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    #[test]
    fn cli_interactive_is_explicit_saved_ids_only_and_keeps_confirmation()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let catalogue = catalogue(true).map_err(std::io::Error::other)?;
        let host = uuid::Uuid::now_v7().to_string();
        let identity = uuid::Uuid::now_v7().to_string();
        assert_eq!(
            catalogue.policy("workspace.ssh", Surface::Cli)?,
            (Scope::Window, Risk::Confirm)
        );
        catalogue.validate_args(
            "workspace.ssh",
            Surface::Cli,
            serde_json::json!({"host":host,"identity":identity,"authentication":"interactive"}),
        )?;
        catalogue.validate_args(
            "workspace.ssh",
            Surface::Cli,
            serde_json::json!({"host":host,"identity":identity,"authentication":"key-interactive"}),
        )?;
        for authentication in ["agent-interactive", "password-interactive"] {
            catalogue.validate_args(
                "workspace.ssh", Surface::Cli,
                serde_json::json!({"host":host,"identity":identity,"authentication":authentication}),
            )?;
        }
        for args in [
            serde_json::json!({"alias":"GUI_HOST","authentication":"password-interactive"}),
            serde_json::json!({"host":host,"identity":identity,"authentication":"password-interactive","password":"x"}),
            serde_json::json!({"host":host,"identity":identity,"authentication":"password-interactive","answers":[]}),
            serde_json::json!({"alias":"GUI_HOST","authentication":"key-interactive"}),
            serde_json::json!({"host":host,"identity":identity,"authentication":"key-interactive","privateKey":true}),
            serde_json::json!({"host":host,"identity":identity,"authentication":"key-interactive","answers":["synthetic"]}),
        ] {
            assert_eq!(
                catalogue.validate_args("workspace.ssh", Surface::Cli, args),
                Err(pulse_cap::Error::Args)
            );
        }
        for args in [
            serde_json::json!({"host":host,"identity":identity,"authentication":"auto"}),
            serde_json::json!({"host":host,"identity":identity,"authentication":false}),
            serde_json::json!({"host":host,"identity":identity,"authentication":null}),
            serde_json::json!({"host":host,"identity":identity,"authentication":"interactive","password":"x"}),
            serde_json::json!({"alias":"GUI_HOST","authentication":"interactive"}),
        ] {
            assert_eq!(
                catalogue.validate_args("workspace.ssh", Surface::Cli, args),
                Err(pulse_cap::Error::Args)
            );
        }
        Ok(())
    }
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    #[test]
    fn focus_tab_is_cli_safe_canonical_target_only_and_never_launch_authority()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let catalogue = catalogue(false).map_err(std::io::Error::other)?;
        assert_eq!(
            catalogue.policy("workspace.focus-tab", Surface::Cli)?,
            (Scope::Window, Risk::Safe)
        );
        for args in [
            serde_json::json!({"session":"1"}),
            serde_json::json!({"session":u64::MAX.to_string()}),
        ] {
            catalogue.validate_args("workspace.focus-tab", Surface::Cli, args)?;
        }
        for args in [
            serde_json::json!({}),
            serde_json::json!({"session":1}),
            serde_json::json!({"session":"0"}),
            serde_json::json!({"session":"01"}),
            serde_json::json!({"session":"+1"}),
            serde_json::json!({"session":" 1"}),
            serde_json::json!({"session":"18446744073709551616"}),
            serde_json::json!({"session":"1","profile":"sh"}),
            serde_json::json!({"session":"1","path":"/bin/sh"}),
            serde_json::json!({"session":"1","confirm":true}),
        ] {
            assert_eq!(
                catalogue.validate_args("workspace.focus-tab", Surface::Cli, args),
                Err(pulse_cap::Error::Args)
            );
        }
        for surface in [Surface::Palette, Surface::Keybinding] {
            assert_eq!(
                catalogue.policy("workspace.focus-tab", surface),
                Err(pulse_cap::Error::Unavailable)
            );
        }
        Ok(())
    }
    #[test]
    fn actual_document_navigation_and_foreign_registry_reject_old_metadata_reads()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let registry = SessionRegistry::new();
        let documents = ProductDocuments::new(registry.clone());
        let owner = registry.open_window()?;
        documents.install(owner)?;
        documents.started(owner)?;
        let nonce = documents.finished(owner)?;
        documents.confirm(owner, nonce)?;
        let old = documents.current(owner, nonce)?;
        let catalogue = catalogue(false).map_err(std::io::Error::other)?;
        assert!(project_current(&registry, &old, &catalogue).is_ok());
        assert_eq!(
            project_current(&SessionRegistry::new(), &old, &catalogue),
            Err("workspace.cap_forbidden")
        );
        documents.started(owner)?;
        assert_eq!(
            project_current(&registry, &old, &catalogue),
            Err("workspace.cap_forbidden")
        );
        let nonce = documents.finished(owner)?;
        documents.confirm(owner, nonce)?;
        let current = documents.current(owner, nonce)?;
        assert!(project_current(&registry, &current, &catalogue).is_ok());
        documents.remove(owner)?;
        assert_eq!(
            project_current(&registry, &current, &catalogue),
            Err("workspace.cap_forbidden")
        );
        Ok(())
    }
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    #[test]
    #[allow(clippy::unwrap_used)]
    fn new_window_requires_separate_confirmation_and_accepts_no_launch_or_transfer_parameters() {
        let catalogue = catalogue(false).unwrap();
        assert_eq!(
            catalogue
                .policy("workspace.new-window", Surface::Cli)
                .unwrap(),
            (Scope::Window, Risk::Confirm)
        );
        assert!(
            catalogue
                .validate_args("workspace.new-window", Surface::Cli, serde_json::json!({}))
                .is_ok()
        );
        for args in [
            serde_json::json!({"session":"1"}),
            serde_json::json!({"profile":"sh"}),
            serde_json::json!({"url":"https://example.com"}),
            serde_json::json!({"label":"workspace"}),
        ] {
            assert!(
                catalogue
                    .validate_args("workspace.new-window", Surface::Cli, args)
                    .is_err()
            );
        }
        assert!(
            catalogue
                .policy("workspace.new-window", Surface::Palette)
                .is_err()
        );
    }
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    #[test]
    #[allow(clippy::unwrap_used)]
    fn split_is_separate_confirm_action_with_closed_profile_direction() {
        let catalogue = catalogue(false).unwrap();
        assert_eq!(
            catalogue.policy("workspace.split", Surface::Cli).unwrap(),
            (Scope::Window, Risk::Confirm)
        );
        for direction in ["row", "column"] {
            assert!(
                catalogue
                    .validate_args(
                        "workspace.split",
                        Surface::Cli,
                        serde_json::json!({"profile":"sh","direction":direction})
                    )
                    .is_ok()
            );
            assert!(
                catalogue
                    .validate_args(
                        "workspace.split",
                        Surface::Cli,
                        serde_json::json!({"direction":direction})
                    )
                    .is_ok()
            );
        }
        for args in [
            serde_json::json!({"profile":"sh"}),
            serde_json::json!({"profile":null,"direction":"row"}),
            serde_json::json!({"profile":"","direction":"row"}),
            serde_json::json!({"profile":"/bin/sh","direction":"row"}),
            serde_json::json!({"profile":"sh","direction":"diagonal"}),
            serde_json::json!({"profile":"sh","direction":"row","program":"sh"}),
        ] {
            assert_eq!(
                catalogue.validate_args("workspace.split", Surface::Cli, args),
                Err(pulse_cap::Error::Args)
            );
        }
        assert_eq!(
            catalogue.validate_args(
                "workspace.new-tab",
                Surface::Cli,
                serde_json::json!({"profile":"sh","direction":"row"})
            ),
            Err(pulse_cap::Error::Args)
        );
        assert_eq!(
            catalogue.policy("workspace.split", Surface::Palette),
            Err(pulse_cap::Error::Unavailable)
        );
    }
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    #[test]
    fn cli_ssh_requires_enabled_surface_explicit_confirmation_and_only_saved_ids()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let c = catalogue(true).map_err(std::io::Error::other)?;
        assert_eq!(
            c.policy("workspace.ssh", Surface::Cli)?,
            (Scope::Window, Risk::Confirm)
        );
        assert_eq!(
            c.policy("workspace.ssh", Surface::Palette),
            Err(pulse_cap::Error::Unavailable)
        );
        assert_eq!(
            catalogue(false)
                .map_err(std::io::Error::other)?
                .policy("workspace.ssh", Surface::Cli),
            Err(pulse_cap::Error::Unavailable)
        );
        let args = serde_json::json!({"host":"11111111-1111-7111-8111-111111111111","identity":"22222222-2222-7222-8222-222222222222"});
        assert!(
            c.validate_args("workspace.ssh", Surface::Cli, args.clone())
                .is_ok()
        );
        for (key, value) in [
            ("program", "/bin/sh"),
            ("password", "secret"),
            ("address", "127.0.0.1"),
            ("identity", "00000000-0000-4000-8000-000000000000"),
            ("host", "127.0.0.1"),
        ] {
            let mut bad = args.clone();
            bad[key] = serde_json::json!(value);
            assert_eq!(
                c.validate_args("workspace.ssh", Surface::Cli, bad),
                Err(pulse_cap::Error::Args)
            );
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "workspace-cli", any(target_os = "linux", windows)))]
#[test]
#[allow(clippy::unwrap_used)]
fn current_directory_requires_cli_confirmation_and_cannot_expand_launch_authority() {
    let c = catalogue(false).unwrap();
    let path = std::env::current_dir().unwrap();
    let cwd = path.to_str().unwrap();
    assert_eq!(
        c.policy("workspace.new-tab", Surface::Cli).unwrap(),
        (Scope::Window, Risk::Confirm)
    );
    assert!(
        c.validate_args(
            "workspace.new-tab",
            Surface::Cli,
            serde_json::json!({"cwd":cwd})
        )
        .is_ok()
    );
    for value in [
        serde_json::json!({}),
        serde_json::json!({"cwd":null}),
        serde_json::json!({"cwd":cwd,"profile":"sh"}),
        serde_json::json!({"cwd":cwd,"profile":null}),
        serde_json::json!({"cwd":cwd,"executable":"sh"}),
        serde_json::json!({"cwd":cwd,"args":[]}),
        serde_json::json!({"cwd":"relative"}),
        serde_json::json!({"cwd":format!("{cwd}\n")}),
        serde_json::json!({"cwd":format!("{cwd}\u{202e}")}),
    ] {
        assert!(
            c.validate_args("workspace.new-tab", Surface::Cli, value)
                .is_err()
        );
    }
    assert!(
        c.validate_args(
            "workspace.split",
            Surface::Cli,
            serde_json::json!({"cwd":cwd,"direction":"row"})
        )
        .is_err()
    );
}
#[cfg(test)]
#[test]
fn find_is_closed_window_ui_metadata_without_cli_authority()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let catalogue = catalogue(false).map_err(std::io::Error::other)?;
    for surface in [Surface::Palette, Surface::Keybinding] {
        assert_eq!(
            catalogue.policy("find", surface)?,
            (Scope::Window, Risk::Safe)
        );
        catalogue.validate_args("find", surface, serde_json::json!({}))?;
        for args in [
            serde_json::json!({"session":"other"}),
            serde_json::json!({"command":"sh"}),
        ] {
            assert_eq!(
                catalogue.validate_args("find", surface, args),
                Err(pulse_cap::Error::Args)
            );
        }
    }
    assert_eq!(
        catalogue.policy("find", Surface::Cli),
        Err(pulse_cap::Error::Unavailable)
    );
    Ok(())
}
