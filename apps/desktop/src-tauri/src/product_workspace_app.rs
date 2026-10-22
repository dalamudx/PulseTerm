//! Application services outlive a page or window; each context owns its approval banks.
use super::*;
#[cfg(all(test, unix))]
#[path = "product_workspace_app_tests.rs"]
pub(super) mod tests;

#[path = "product_workspace_window_drains.rs"]
mod windows;

#[cfg(all(test, any(unix, windows)))]
pub(crate) fn test_application() -> Arc<Application> {
    let registry = SessionRegistry::new();
    let directory = pulse_app::window_directory::WindowDirectory::new(registry.clone());
    Arc::new(Application {
        documents: directory.documents(),
        directory,
        sessions: Arc::new(Mutex::new(pulse_app::local_session_root::SessionRoot::new(
            registry.clone(),
            tokio::runtime::Handle::current(),
        ))),
        registry,
        runtime: tokio::runtime::Handle::current(),
        exit: exit::ExitGate::new(),
        settings_io: Arc::new(settings::SettingsIo::new()),
        window_drains: Arc::new(WindowDrains::new()),
        #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
        cli: Mutex::new(None),
        profile_storage: Arc::new(profiles::ProfileStorage::new()),
    })
}
pub(super) use windows::{Retirement, WindowDrains};
#[path = "product_workspace_native_window.rs"]
mod native;
pub(super) use native::NativeBinding;
pub(super) struct Application {
    pub(super) registry: SessionRegistry,
    pub(super) documents: Arc<ProductDocuments>,
    pub(super) directory: pulse_app::window_directory::WindowDirectory,
    pub(super) sessions: Arc<Mutex<pulse_app::local_session_root::SessionRoot>>,
    pub(super) runtime: tokio::runtime::Handle,
    pub(super) exit: exit::ExitGate,
    pub(super) settings_io: Arc<settings::SettingsIo>,
    pub(super) window_drains: Arc<WindowDrains>,
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    pub(super) cli: Mutex<Option<Arc<pulse_app::cli_control::Server>>>,
    #[cfg(any(unix, windows))]
    pub(super) profile_storage: Arc<profiles::ProfileStorage>,
}

impl Workspace {
    #[cfg(test)]
    pub(super) fn from_application(
        application: Arc<Application>,
        owner: WindowToken,
        #[cfg(any(unix, windows))] profiles: Arc<profiles::ProfileReader>,
    ) -> Result<Self> {
        Self::construct(
            application,
            owner,
            None,
            #[cfg(any(unix, windows))]
            profiles,
        )
    }
    pub(super) fn from_native(
        application: Arc<Application>,
        native: Arc<NativeBinding>,
        #[cfg(any(unix, windows))] profiles: Arc<profiles::ProfileReader>,
    ) -> Result<Self> {
        if !Arc::ptr_eq(&native.documents(), &application.documents)
            || !Arc::ptr_eq(&application.directory.documents(), &application.documents)
        {
            return Err("workspace.denied");
        }
        let owner = native.owner();
        Self::construct(
            application,
            owner,
            Some(native),
            #[cfg(any(unix, windows))]
            profiles,
        )
    }
    fn construct(
        application: Arc<Application>,
        owner: WindowToken,
        native: Option<Arc<NativeBinding>>,
        #[cfg(any(unix, windows))] profiles: Arc<profiles::ProfileReader>,
    ) -> Result<Self> {
        if !application.exit.accepting_windows() || !application.window_drains.accepting() {
            return Err("workspace.denied");
        }
        #[cfg(any(unix, windows))]
        if !Arc::ptr_eq(&profiles.shared_storage(), &application.profile_storage) {
            return Err("workspace.denied");
        }
        let registry = application.registry.clone();
        #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
        let documents = application.documents.clone();
        let vault = pulse_app::vault_owner::PageVaultOwner::new(registry.clone());
        let vault_handoff = pulse_app::vault_handoff::VaultHandoffBank::new(&vault);
        let password_writes = pulse_app::vault_password_handoff::PasswordWriteBank::new(&vault);
        let private_key_writes =
            pulse_app::vault_key_write_handoff::PrivateKeyWriteBank::new(&vault);
        let keymaps = keymap::KeymapOwner::new(registry.clone(), application.runtime.clone());
        let settings = Arc::new(settings::SettingsOwner::with_io(
            application.settings_io.clone(),
        ));
        let context = Self(Arc::new(Inner {
            application,
            owner,
            retirement: std::sync::OnceLock::new(),
            native,
            ssh_trust: Arc::new(ssh_trust::TrustBank::new(registry.clone(), owner)),
            ssh_sessions: Arc::new(ssh_connection::SessionFactory::new(registry, owner)),
            vault,
            vault_handoff,
            password_writes,
            private_key_writes,
            #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
            bootstrap: crate::cli_bootstrap::Bank::default(),
            #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
            presentation: crate::cli_presentation::Presentation::default(),
            #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
            selections: Arc::new(crate::cli_selection::Selections::new(documents, owner)),
            #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
            cli_approvals: Arc::new(cli::Approvals::default()),
            terminals: Arc::new(terminal::TerminalBank::new()),
            visibility: crate::native_visibility::NativeWatcher::new(),
            settings,
            keymaps,
            #[cfg(any(unix, windows))]
            profiles,
            hosts: hosts::captures::HostCaptures::new(),
            ssh_profiles: metadata_captures::MetadataCaptures::new("workspace.ssh_profiles_failed"),
            #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
            quick_targets: quick_targets::QuickTargets::new(),
            organization: metadata_captures::MetadataCaptures::new("workspace.organization_failed"),
            identities: metadata_captures::MetadataCaptures::new("workspace.identities_failed"),
            key_identities: metadata_captures::MetadataCaptures::new(
                "workspace.key_identities_failed",
            ),
            agent_identities: agent_identities::AgentBank::new(),
            #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
            configs: metadata_captures::MetadataCaptures::new("workspace.config_unavailable"),
            #[cfg(all(any(unix, windows), feature = "workspace-ssh"))]
            config_imports: config_imports::ConfigImportBank::new(),
        }));
        context.0.window_drains.install(context.clone())?;
        Ok(context)
    }

    pub(super) fn retire_window(&self) -> Result<()> {
        self.0.window_drains.retire(self)
    }

    fn revoke_window(&self) -> Result<()> {
        self.0.visibility.close();
        #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
        {
            let _ = self.0.bootstrap.close();
            let _ = self.0.presentation.close();
            self.0.selections.close();
            cli::retire(self);
        }
        #[cfg(any(unix, windows))]
        self.0.profiles.close();
        self.0.ssh_trust.close();
        self.0.ssh_sessions.close();
        self.0.settings.close();
        self.0.keymaps.close();
        self.0.terminals.retire();
        let password = self
            .0
            .password_writes
            .close()
            .map_err(|_| "workspace.vault_failed");
        let key = self
            .0
            .private_key_writes
            .close()
            .map_err(|_| "workspace.vault_failed");
        let handoff = self
            .0
            .vault_handoff
            .close()
            .map_err(|_| "workspace.vault_failed");
        let vault = self.0.vault.close().map_err(|_| "workspace.vault_failed");
        let authority = if let Some(native) = self.0.native.as_ref() {
            native.revoke()
        } else {
            // Only model/test contexts are constructed without a native binding.
            let document = self
                .0
                .documents
                .remove(self.0.owner)
                .map_err(|_| "workspace.denied");
            let registry = self
                .0
                .registry
                .close_window(self.0.owner)
                .map_err(|_| "workspace.denied");
            document.and(registry)
        };
        password.and(key).and(handoff).and(vault).and(authority)
    }
}
