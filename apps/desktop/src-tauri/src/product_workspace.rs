//! Opt-in workspace: current-document restricted Profile startup and read-only v2 views.
//! Restricted discovered-Profile startup; no validation command or caller executable.
use pulse_app::{
    product_document::ProductDocuments,
    registry::{SessionRegistry, WindowToken},
};
#[cfg(unix)]
#[path = "product_app_data.rs"]
mod app_data;
#[cfg(unix)]
#[path = "product_profiles.rs"]
mod profiles;
#[path = "product_ssh_config.rs"]
mod ssh_config;
use std::sync::{Arc, Mutex};
#[path = "product_agent_identities.rs"]
mod agent_identities;
#[path = "product_workspace_app.rs"]
mod app;
#[path = "product_capabilities.rs"]
mod capabilities;
#[cfg(all(feature = "workspace-cli", target_os = "linux"))]
#[path = "product_cli.rs"]
mod cli;
#[path = "product_cli_answer.rs"]
mod cli_answer;
#[cfg(all(unix, feature = "workspace-ssh"))]
#[path = "product_config_import_bank.rs"]
mod config_imports;
#[path = "product_workspace_exit.rs"]
mod exit;
#[path = "product_hosts.rs"]
mod hosts;
#[path = "product_identities.rs"]
mod identities;
#[path = "product_identity_key.rs"]
mod identity_key;
#[path = "product_identity_password.rs"]
mod identity_password;
#[path = "product_key_identities.rs"]
mod key_identities;
#[path = "product_keymap.rs"]
mod keymap;
use app::Application;
#[path = "product_metadata_captures.rs"]
mod metadata_captures;
#[path = "product_organization.rs"]
mod organization;
#[path = "product_organization_behavior.rs"]
mod organization_behavior;
#[path = "product_quick_targets.rs"]
mod quick_targets;
#[path = "product_session.rs"]
mod session;
#[path = "product_settings.rs"]
mod settings;
#[cfg(all(feature = "workspace-cli", target_os = "linux"))]
#[path = "product_cli_single_instance.rs"]
mod single_instance;
#[path = "product_ssh_session.rs"]
mod ssh_connection;
#[path = "product_ssh_profiles.rs"]
mod ssh_profiles;
#[path = "product_ssh_trust.rs"]
mod ssh_trust;
#[path = "product_startup_diagnostics.rs"]
mod startup_diagnostics;
#[path = "product_terminal.rs"]
mod terminal;
#[path = "product_vault.rs"]
mod vault;
use startup_diagnostics::{Stage as StartupStage, record as record_startup};
#[path = "product_visibility.rs"]
mod visibility;
use tauri::webview::PageLoadEvent;
#[path = "product_window_state.rs"]
mod window_state;
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use uuid::Uuid;
use window_state::State;

#[cfg(all(feature = "workspace-cli", target_os = "linux"))]
#[path = "product_workspace_new_window.rs"]
mod new_window;
const LABEL: &str = "workspace";
const PAGE: &str = "/workspace.html";
const WORKSPACE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data:";

fn isolated_config(config: &tauri::utils::config::Config) -> bool {
    use tauri::utils::config::{CapabilityEntry, Csp};
    config.identifier == "dev.pulseterm.workspace-preview"
        && config.app.windows.is_empty()
        && config.app.app_directories_override.is_none()
        && matches!(&config.app.security.csp, Some(Csp::Policy(policy)) if policy == WORKSPACE_CSP)
        && config.app.security.dev_csp.is_none()
        && matches!(config.app.security.capabilities.as_slice(), [CapabilityEntry::Reference(name)] if name == "workspace")
        && !config.bundle.active
}

struct Inner {
    application: Arc<Application>,
    retirement: std::sync::OnceLock<app::Retirement>,
    native: Option<Arc<app::NativeBinding>>,
    ssh_trust: Arc<ssh_trust::TrustBank>,
    ssh_sessions: Arc<ssh_connection::SessionFactory>,
    vault: pulse_app::vault_owner::PageVaultOwner,
    vault_handoff: pulse_app::vault_handoff::VaultHandoffBank,
    password_writes: pulse_app::vault_password_handoff::PasswordWriteBank,
    private_key_writes: pulse_app::vault_key_write_handoff::PrivateKeyWriteBank,
    owner: WindowToken,
    #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
    bootstrap: crate::cli_bootstrap::Bank,
    #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
    presentation: crate::cli_presentation::Presentation,
    #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
    selections: Arc<crate::cli_selection::Selections>,
    #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
    cli_approvals: Arc<cli::Approvals>,
    terminals: Arc<terminal::TerminalBank>,
    visibility: Arc<crate::native_visibility::NativeWatcher>,
    settings: Arc<settings::SettingsOwner>,
    keymaps: Arc<keymap::KeymapOwner>,
    hosts: hosts::captures::HostCaptures,
    ssh_profiles: metadata_captures::MetadataCaptures<pulse_store::ssh_profiles::ProfileSnapshot>,
    #[cfg(all(unix, feature = "workspace-ssh"))]
    quick_targets: quick_targets::QuickTargets,
    organization: metadata_captures::MetadataCaptures<organization::OrganizationCapture>,
    identities: metadata_captures::MetadataCaptures<pulse_store::ssh_identities::IdentitySnapshot>,
    key_identities:
        metadata_captures::MetadataCaptures<pulse_store::ssh_key_identities::KeyIdentitySnapshot>,
    agent_identities: agent_identities::AgentBank,
    #[cfg(all(unix, feature = "workspace-ssh"))]
    configs: metadata_captures::MetadataCaptures<Arc<profiles::PageConfigCapture>>,
    #[cfg(all(unix, feature = "workspace-ssh"))]
    config_imports: config_imports::ConfigImportBank,
    #[cfg(unix)]
    profiles: Arc<profiles::ProfileReader>,
}
impl std::ops::Deref for Inner {
    type Target = Application;
    fn deref(&self) -> &Self::Target {
        &self.application
    }
}
#[derive(Clone)]
struct Workspace(Arc<Inner>);

type Result<T> = std::result::Result<T, &'static str>;
#[cfg(all(test, unix))]
static KEYMAP_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn local_workspace_url(url: &tauri::Url) -> bool {
    crate::native_origin::local_url(url)
        && url.path() == PAGE
        && url.query().is_none()
        && url.fragment().is_none()
}

fn local_workspace(window: &WebviewWindow) -> bool {
    pulse_app::window_directory::WindowLabel::parse(window.label()).is_ok()
        && window.url().is_ok_and(|url| local_workspace_url(&url))
}

impl Workspace {
    pub(super) fn native_label(&self) -> Result<&str> {
        self.0
            .native
            .as_ref()
            .map(|native| native.label().as_str())
            .ok_or("workspace.denied")
    }
    fn on_page_load(
        &self,
        window: &WebviewWindow,
        event: PageLoadEvent,
        url: &tauri::Url,
    ) -> Result<()> {
        if self.native_label().ok() != Some(window.label()) {
            return Err("workspace.denied");
        }
        match event {
            PageLoadEvent::Started => {
                record_startup(StartupStage::PageStarted);
                #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
                self.0.bootstrap.started().map_err(|_| "workspace.denied")?;
                self.0
                    .documents
                    .started(self.0.owner)
                    .map_err(|_| "workspace.denied")?;
                #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
                self.0
                    .presentation
                    .navigation()
                    .map_err(|_| "workspace.denied")?;
                #[cfg(unix)]
                self.0.profiles.retire_read_waiters();
                #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
                cli::retire(self);
                #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
                self.0.selections.retire();
                self.0.ssh_trust.retire();
                self.0.ssh_sessions.retire();
                self.0.terminals.retire();
                self.0.keymaps.retire();
                self.0.hosts.retire();
                self.0.ssh_profiles.retire();
                #[cfg(all(unix, feature = "workspace-ssh"))]
                self.0.quick_targets.retire();
                self.0.organization.retire();
                self.0.identities.retire();
                self.0.key_identities.retire();
                self.0.agent_identities.retire();
                #[cfg(all(unix, feature = "workspace-ssh"))]
                self.0.configs.retire();
                #[cfg(all(unix, feature = "workspace-ssh"))]
                self.0.config_imports.retire();
                self.0
                    .password_writes
                    .retire()
                    .map_err(|_| "workspace.vault_failed")?;
                self.0
                    .private_key_writes
                    .retire()
                    .map_err(|_| "workspace.vault_failed")?;
                self.0
                    .vault_handoff
                    .retire()
                    .map_err(|_| "workspace.vault_failed")?;
                self.0
                    .vault
                    .retire_page()
                    .map_err(|_| "workspace.vault_failed")?;
                eprintln!("workspace.navigation_started");
                Ok(())
            }
            PageLoadEvent::Finished => {
                record_startup(if url.as_str() == "about:blank" {
                    StartupStage::BlankPageFinished
                } else if local_workspace_url(url) {
                    StartupStage::LocalPageFinished
                } else {
                    StartupStage::OtherPageFinished
                });
                if !local_workspace_url(url) || !local_workspace(window) {
                    return Err("workspace.denied");
                }
                let nonce = self
                    .0
                    .documents
                    .finished(self.0.owner)
                    .map_err(|_| "workspace.denied")?;
                #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
                self.0
                    .bootstrap
                    .finished(&self.0.documents, self.0.owner)
                    .map_err(|_| "workspace.denied")?;
                // Only fixed source plus a UUID created by the backend is evaluated.
                // No IPC getter: this value is injected into precisely this local page.
                let script = format!(
                    "window.__PULSE_WORKSPACE_NONCE__={};window.__PULSE_WORKSPACE_SSH_FINGERPRINT__={};window.__PULSE_WORKSPACE_SSH_CONNECTION__={1};window.dispatchEvent(new Event('pulseterm:workspace-ready'));",
                    serde_json::to_string(&nonce.to_string()).map_err(|_| "workspace.denied")?,
                    cfg!(all(feature = "workspace-ssh", unix))
                );
                window.eval(&script).map_err(|_| "workspace.denied")?;
                record_startup(StartupStage::NonceInjected);
                #[cfg(feature = "validation-probe-webgl")]
                window
                    .eval(include_str!("validation_webgl_observer.js"))
                    .map_err(|_| "workspace.denied")?;
                Ok(())
            }
        }
    }
}

fn confirm_current_document(
    documents: &ProductDocuments,
    owner: WindowToken,
    url: &tauri::Url,
    nonce: &str,
) -> Result<()> {
    if !local_workspace_url(url) {
        return Err("workspace.denied");
    }
    let nonce = Uuid::parse_str(nonce).map_err(|_| "workspace.denied")?;
    documents
        .confirm(owner, nonce)
        .map_err(|_| "workspace.denied")
}

fn current_document(
    documents: &ProductDocuments,
    owner: WindowToken,
    url: &tauri::Url,
    nonce: &str,
) -> Result<(Uuid, Arc<pulse_app::registry::PageLaunchScope>)> {
    if !local_workspace_url(url) {
        return Err("workspace.denied");
    }
    let nonce = Uuid::parse_str(nonce).map_err(|_| "workspace.denied")?;
    let scope = documents
        .current(owner, nonce)
        .map_err(|_| "workspace.denied")?;
    Ok((nonce, scope))
}
#[tauri::command]
fn workspace_confirm(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<()> {
    record_startup(StartupStage::DocumentConfirmEntered);
    if state.native_label().ok() != Some(window.label()) {
        return Err("workspace.denied");
    }
    let url = window.url().map_err(|_| "workspace.denied")?;
    confirm_current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
    eprintln!("workspace.document_confirmed");
    record_startup(StartupStage::DocumentConfirmed);
    Ok(())
}

/// Metadata only. It does not confirm a page or admit a CLI action.
#[tauri::command]
fn workspace_cli_ready(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<()> {
    if state.native_label().ok() != Some(window.label()) {
        return Err("workspace.denied");
    }
    let url = window.url().map_err(|_| "workspace.denied")?;
    let (_, scope) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
    #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
    state
        .0
        .presentation
        .mark(&state.0.documents, state.0.owner, scope)
        .map_err(|_| "workspace.denied")?;
    #[cfg(not(all(feature = "workspace-cli", target_os = "linux")))]
    drop(scope);
    eprintln!("workspace.cli_presentation_ready");
    record_startup(StartupStage::CliPageReady);
    Ok(())
}

/// Current-page, read-only Profile identities. No executable, directory or launch ability.
#[derive(serde::Serialize)]
struct ProfileOfferDto {
    kind: &'static str,
    hidden: bool,
}
#[tauri::command]
async fn workspace_profiles(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Vec<ProfileOfferDto>> {
    #[cfg(not(unix))]
    {
        let _ = (window, state, nonce);
        Err("workspace.unavailable")
    }
    #[cfg(unix)]
    {
        if state.native_label().ok() != Some(window.label()) {
            return Err("workspace.denied");
        }
        let url = window.url().map_err(|_| "workspace.denied")?;
        let (nonce, scope) = current_document(&state.0.documents, state.0.owner, &url, &nonce)?;
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let path = std::env::var_os("PATH").ok_or("workspace.unavailable")?;
        let shell = std::env::var_os("SHELL").map(std::path::PathBuf::from);
        eprintln!("workspace.profiles_requested");
        record_startup(StartupStage::ProfilesRequested);
        let offers = state
            .0
            .profiles
            .list(state.0.registry.clone(), scope, directory, path, shell)
            .await
            .inspect_err(|&error| {
                let code = match error {
                    "workspace.denied" => "denied",
                    "workspace.busy" => "busy",
                    _ => "unavailable",
                };
                eprintln!("workspace.profiles_load_failed.{code}");
            })?;
        if !local_workspace(&window) || state.0.documents.current(state.0.owner, nonce).is_err() {
            return Err("workspace.denied");
        }
        eprintln!("workspace.profiles_listed");
        record_startup(StartupStage::ProfilesListed);
        Ok(offers)
    }
}

/// Preview-only separate runner; none of the validation commands are mounted.
pub fn run() {
    record_startup(StartupStage::RunEntered);
    let context = tauri::generate_context!("workspace/tauri.conf.json");
    if !isolated_config(context.config()) {
        eprintln!("workspace.configuration_denied");
        record_startup(StartupStage::ConfigurationDenied);
        std::process::exit(1);
    }
    let start = (|| -> Result<_> {
        let registry = SessionRegistry::new();
        let directory = pulse_app::window_directory::WindowDirectory::new(registry.clone());
        let registration = Arc::new(directory.reserve(true).map_err(|_| "workspace.denied")?);
        let native = Arc::new(app::NativeBinding::new(directory.clone(), registration)?);
        let documents = native.documents();
        let runtime = tauri::async_runtime::handle().inner().clone();
        let sessions = Arc::new(Mutex::new(pulse_app::local_session_root::SessionRoot::new(
            registry.clone(),
            runtime.clone(),
        )));
        #[cfg(unix)]
        let profiles = Arc::new(profiles::ProfileReader::new());
        let application = Arc::new(Application {
            registry,
            documents,
            directory,
            sessions,
            runtime,
            exit: exit::ExitGate::new(),
            settings_io: Arc::new(settings::SettingsIo::new()),
            window_drains: Arc::new(app::WindowDrains::new()),
            #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
            cli: Mutex::new(None),
            #[cfg(unix)]
            profile_storage: profiles.shared_storage(),
        });
        Workspace::from_native(
            application,
            native,
            #[cfg(unix)]
            profiles,
        )
    })();
    let state = match start {
        Ok(state) => state,
        Err(_) => {
            record_startup(StartupStage::ServicesFailed);
            eprintln!("workspace.start_failed");
            std::process::exit(1);
        }
    };
    record_startup(StartupStage::ServicesReady);
    #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
    let unsupported_startup = match crate::cli_startup::from_args(std::env::args_os().skip(1)) {
        Ok(Some(action)) if action.supports_cold_transport() => {
            if state.0.bootstrap.enqueue_first(action).is_err() {
                eprintln!("workspace.startup_denied");
                std::process::exit(1);
            }
            false
        }
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(_) => {
            eprintln!("workspace.startup_denied");
            std::process::exit(2);
        }
    };
    let window_contexts = window_state::Contexts::new();
    if window_contexts.install(LABEL, state.clone()).is_err() {
        eprintln!("workspace.start_failed");
        std::process::exit(1);
    }
    let close_contexts = window_contexts.clone();
    let on_load = state.clone();
    let on_close = state.clone();
    let builder = tauri::Builder::default();
    // The upstream plugin must run before workspace setup or endpoint creation.
    #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
    let builder = builder.plugin(single_instance::plugin());
    record_startup(StartupStage::RuntimeBuildStarted);
    let result = builder
        .manage(state)
        .manage(window_contexts)
        .invoke_handler(tauri::generate_handler![
            workspace_confirm,
            visibility::workspace_native_visibility,
            workspace_cli_ready,
            workspace_profiles,
            hosts::workspace_hosts,
            hosts::workspace_host_save,
            ssh_profiles::workspace_ssh_profiles,
            ssh_profiles::workspace_ssh_profile_save,
            quick_targets::workspace_quick_target_prepare,
            quick_targets::workspace_quick_target_cancel,
            quick_targets::workspace_quick_target_save,
            organization::workspace_recent_hosts,
            organization::workspace_organization,
            organization::workspace_organization_save,
            organization_behavior::workspace_organization_behavior,
            ssh_trust::workspace_ssh_fingerprint_prepare,
            ssh_trust::workspace_ssh_fingerprint_observe,
            ssh_trust::workspace_ssh_fingerprint_confirm,
            agent_identities::workspace_agent_identities,
            agent_identities::workspace_agent_identity_save,
            agent_identities::workspace_agent_identity_cancel,
            ssh_trust::workspace_ssh_fingerprint_cancel,
            ssh_connection::route::workspace_ssh_route_begin,
            ssh_connection::route::workspace_ssh_route_wait,
            ssh_connection::route::workspace_ssh_route_confirm,
            ssh_connection::route::workspace_ssh_route_answer,
            ssh_connection::route::workspace_ssh_route_cancel,
            ssh_connection::workspace_ssh_session_begin,
            ssh_connection::workspace_ssh_agent_session_begin,
            ssh_connection::workspace_ssh_password_interactive_session_begin,
            ssh_connection::workspace_ssh_private_key_interactive_session_begin,
            ssh_connection::workspace_ssh_agent_interactive_session_begin,
            ssh_connection::workspace_ssh_interactive_session_begin,
            ssh_connection::workspace_ssh_interactive_session_prepare,
            ssh_connection::workspace_ssh_interactive_session_answer,
            ssh_connection::workspace_ssh_agent_session_prepare,
            ssh_connection::workspace_ssh_agent_session_start,
            ssh_connection::workspace_ssh_agent_session_cancel,
            ssh_connection::workspace_ssh_key_session_begin,
            ssh_connection::workspace_ssh_key_session_prepare,
            ssh_connection::workspace_ssh_session_prepare,
            ssh_connection::workspace_ssh_session_start,
            ssh_connection::workspace_ssh_session_cancel,
            identities::workspace_identities,
            identities::workspace_identity_save,
            key_identities::workspace_key_identities,
            key_identities::workspace_key_identity_save,
            ssh_config::workspace_ssh_config_preview,
            ssh_config::workspace_ssh_config_confirm,
            ssh_config::workspace_ssh_config_status,
            ssh_config::workspace_ssh_config_import_prepare,
            ssh_config::workspace_ssh_config_import_save,
            ssh_config::workspace_ssh_config_import_cancel,
            identity_password::workspace_identity_password_prepare,
            identity_password::workspace_identity_password_submit,
            identity_password::workspace_identity_password_cancel,
            identity_key::workspace_identity_key_available,
            identity_key::workspace_identity_key_prepare,
            identity_key::workspace_identity_key_submit,
            identity_key::workspace_identity_key_cancel,
            vault::workspace_vault_status,
            vault::workspace_vault_lock,
            vault::handoff::workspace_vault_prepare,
            vault::handoff::workspace_vault_confirm,
            vault::handoff::workspace_vault_cancel,
            vault::handoff::workspace_vault_submit,
            settings::workspace_settings_read,
            settings::workspace_settings_save,
            capabilities::workspace_cap_list,
            cli_answer::workspace_cli_answer,
            cli_answer::workspace_cli_select_admit,
            cli_answer::workspace_cli_selected,
            keymap::workspace_keymap_read,
            keymap::workspace_keymap_confirm,
            keymap::workspace_keymap_save,
            keymap::workspace_keymap_cancel,
            session::workspace_profile_editor_read,
            session::workspace_profile_editor_save,
            session::workspace_profile_editor_cancel,
            session::workspace_custom_profile_read,
            session::workspace_custom_profile_save,
            session::workspace_custom_profile_cancel,
            session::workspace_custom_profile_list,
            session::workspace_custom_profile_open,
            session::workspace_profile_visibility,
            session::workspace_profile_default_read,
            session::workspace_profile_default_save,
            session::workspace_profile_default_cancel,
            session::workspace_profile_deleted,
            session::workspace_profile_lifecycle_prepare,
            session::workspace_profile_lifecycle_commit,
            session::workspace_profile_lifecycle_cancel,
            session::workspace_session_open,
            session::workspace_sessions,
            session::workspace_session_close,
            terminal::workspace_terminal_prepare,
            terminal::workspace_terminal_connect,
            terminal::workspace_terminal_ack,
            terminal::workspace_terminal_confirm,
            terminal::workspace_terminal_enable_input,
            terminal::workspace_terminal_input_ready,
            terminal::workspace_terminal_report_failure,
            terminal::links::workspace_terminal_link_open,
            terminal::clipboard::workspace_terminal_clipboard_prepare,
            terminal::clipboard::workspace_terminal_clipboard_cancel,
            terminal::clipboard::workspace_terminal_clipboard_read,
            terminal::workspace_terminal_input,
            terminal::workspace_terminal_history_page,
            terminal::workspace_terminal_history_search,
            terminal::workspace_terminal_resize,
            terminal::workspace_terminal_detach
        ])
        .setup(move |app| {
            record_startup(StartupStage::SetupEntered);
            #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
            single_instance::verify_owner(app.handle())?;
            record_startup(StartupStage::InstanceVerified);
            #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
            if unsupported_startup {
                return Err("workspace.startup_unavailable".into());
            }
            // WebKit uses app_data_dir for its storage and otherwise creates it
            // with the process umask (typically 0755). Admit/create it privately
            // before constructing any WebView. Never chmod an existing directory.
            #[cfg(unix)]
            let _admitted_data = app_data::prepare(&app.path().app_data_dir()?)?;
            record_startup(StartupStage::DataPrepared);
            record_startup(StartupStage::WindowBuildStarted);
            let window =
                WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("workspace.html".into()))
                    .title("PulseTerm · 工作区预览")
                    .inner_size(960.0, 640.0)
                    .on_page_load(move |window, payload| {
                        if on_load
                            .on_page_load(&window, payload.event(), payload.url())
                            .is_err()
                        {
                            record_startup(StartupStage::PageRejected);
                            let _ = window.close();
                        }
                    })
                    .build()?;
            record_startup(StartupStage::WindowBuilt);
            window.on_window_event(move |event| {
                if matches!(event, WindowEvent::Destroyed) {
                    if let Some(native) = on_close.0.native.as_ref() {
                        record_startup(StartupStage::WindowDestroyed);
                        let _ = native.destroyed();
                    }
                    let _ = close_contexts.retire(LABEL);
                    let _ = on_close.retire_window();
                }
            });
            let state = app.state::<Workspace>();
            visibility::install(&window, &state)?;
            record_startup(StartupStage::VisibilityInstalled);
            state
                .0
                .native
                .as_ref()
                .ok_or("workspace.denied")?
                .publish()?;
            record_startup(StartupStage::WindowPublished);
            #[cfg(all(feature = "workspace-cli", target_os = "linux"))]
            {
                let server = cli::start(app.handle(), (*state).clone())?;
                *state.0.cli.lock().map_err(|_| "workspace.cli_failed")? = Some(server);
                record_startup(StartupStage::EndpointPublished);
            }
            record_startup(StartupStage::SetupFinished);
            Ok(())
        })
        .build(context);
    match result {
        Ok(app) => {
            record_startup(StartupStage::RuntimeBuilt);
            app.run(|handle, event| match event {
                tauri::RunEvent::Ready => record_startup(StartupStage::Ready),
                tauri::RunEvent::ExitRequested { api, code, .. } => {
                    record_startup(StartupStage::ExitRequested);
                    exit::requested(handle, &api, code);
                }
                tauri::RunEvent::Exit => {
                    record_startup(StartupStage::Exited);
                    eprintln!("workspace.exited");
                }
                _ => {}
            });
        }
        Err(_) => {
            eprintln!("workspace.start_failed");
            record_startup(StartupStage::RuntimeBuildFailed);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    // Test fixtures must extract generated nonces; production confirmation never unwraps.
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn native_confirmation_rejects_old_page_foreign_owner_and_nonlocal_source() {
        let registry = SessionRegistry::new();
        let documents = ProductDocuments::new(registry.clone());
        let owner = registry.open_window().unwrap();
        let foreign = registry.open_window().unwrap();
        documents.install(owner).unwrap();
        documents.install(foreign).unwrap();
        let local = tauri::Url::parse("http://tauri.localhost/workspace.html").unwrap();
        let remote = tauri::Url::parse("https://example.com/workspace.html").unwrap();
        documents.started(owner).unwrap();
        let first = documents.finished(owner).unwrap();
        assert_eq!(
            confirm_current_document(&documents, owner, &remote, &first.to_string()),
            Err("workspace.denied")
        );
        assert_eq!(
            confirm_current_document(&documents, foreign, &local, &first.to_string()),
            Err("workspace.denied")
        );
        assert_eq!(
            confirm_current_document(&documents, owner, &local, "not-a-nonce"),
            Err("workspace.denied")
        );
        confirm_current_document(&documents, owner, &local, &first.to_string()).unwrap();
        assert_eq!(
            confirm_current_document(&documents, owner, &local, &first.to_string()),
            Err("workspace.denied")
        );
        documents.started(owner).unwrap();
        assert_eq!(
            confirm_current_document(&documents, owner, &local, &first.to_string()),
            Err("workspace.denied")
        );
        let second = documents.finished(owner).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            confirm_current_document(&documents, owner, &local, &first.to_string()),
            Err("workspace.denied")
        );
        confirm_current_document(&documents, owner, &local, &second.to_string()).unwrap();
        let scope = documents.current(owner, second).unwrap();
        documents.remove(owner).unwrap();
        registry.close_window(owner).unwrap();
        assert!(registry.check_launch_page(&scope).is_err());
        assert_eq!(
            confirm_current_document(&documents, owner, &local, &second.to_string()),
            Err("workspace.denied")
        );
        documents.remove(foreign).unwrap();
        registry.close_window(foreign).unwrap();
        registry.shutdown().unwrap();
    }
    #[test]
    fn actual_workspace_configuration_cannot_inherit_validation_assets_or_capabilities() {
        let config: tauri::utils::config::Config =
            serde_json::from_str(include_str!("../workspace/tauri.conf.json")).unwrap();
        assert!(isolated_config(&config));
        let validation: tauri::utils::config::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(!isolated_config(&validation));
        let mut bad = config.clone();
        bad.identifier = validation.identifier;
        assert!(!isolated_config(&bad));
        bad = config.clone();
        bad.app.windows = validation.app.windows;
        assert!(!isolated_config(&bad));
        bad = config.clone();
        bad.app.security.csp = validation.app.security.csp;
        assert!(!isolated_config(&bad));
        bad = config.clone();
        bad.app.security.capabilities.clear();
        assert!(!isolated_config(&bad));
        bad = config;
        bad.app.security.dev_csp = bad.app.security.csp.clone();
        assert!(!isolated_config(&bad));
    }

    #[cfg(unix)]
    #[test]
    fn profile_reads_use_ready_document_without_consuming_confirmation_again() {
        let registry = SessionRegistry::new();
        let documents = ProductDocuments::new(registry.clone());
        let owner = registry.open_window().unwrap();
        documents.install(owner).unwrap();
        documents.started(owner).unwrap();
        let first = documents.finished(owner).unwrap();
        let local = tauri::Url::parse("tauri://localhost/workspace.html").unwrap();
        assert!(current_document(&documents, owner, &local, &first.to_string()).is_err());
        confirm_current_document(&documents, owner, &local, &first.to_string()).unwrap();
        for _ in 0..3 {
            current_document(&documents, owner, &local, &first.to_string()).unwrap();
        }
        assert!(confirm_current_document(&documents, owner, &local, &first.to_string()).is_err());
        let remote = tauri::Url::parse("https://example.com/workspace.html").unwrap();
        assert!(current_document(&documents, owner, &remote, &first.to_string()).is_err());
        let foreign = registry.open_window().unwrap();
        assert!(current_document(&documents, foreign, &local, &first.to_string()).is_err());
        documents.started(owner).unwrap();
        assert!(current_document(&documents, owner, &local, &first.to_string()).is_err());
        let second = documents.finished(owner).unwrap();
        confirm_current_document(&documents, owner, &local, &second.to_string()).unwrap();
        assert!(current_document(&documents, owner, &local, &first.to_string()).is_err());
        current_document(&documents, owner, &local, &second.to_string()).unwrap();
        documents.remove(owner).unwrap();
        registry.close_window(owner).unwrap();
        registry.close_window(foreign).unwrap();
        registry.shutdown().unwrap();
    }

    #[test]
    fn exact_local_page_only() -> std::result::Result<(), Box<dyn std::error::Error>> {
        for url in [
            "tauri://localhost/workspace.html",
            "http://tauri.localhost/workspace.html",
            "https://tauri.localhost/workspace.html",
        ] {
            assert!(local_workspace_url(&tauri::Url::parse(url)?));
        }
        for url in [
            "tauri://localhost/",
            "tauri://localhost/index.html",
            "tauri://localhost/workspace.html?run=1",
            "tauri://localhost/workspace.html#fragment",
            "https://example.com/workspace.html",
            "http://tauri.localhost:123/workspace.html",
        ] {
            assert!(!local_workspace_url(&tauri::Url::parse(url)?));
        }
        Ok(())
    }
}
