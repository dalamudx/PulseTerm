//! Only verified physical drain can permit a subsequent Tauri ExitRequested.
use super::*;
use std::sync::atomic::{AtomicU8, Ordering};
const IDLE: u8 = 0;
const DRAINING: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;
pub(super) struct ExitGate(AtomicU8);
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    Begin,
    Prevent,
    Allow,
}
impl ExitGate {
    pub(super) fn new() -> Self {
        Self(AtomicU8::new(IDLE))
    }
    pub(super) fn accepting_windows(&self) -> bool {
        self.0.load(Ordering::SeqCst) == IDLE
    }
    pub(super) fn request(&self) -> Action {
        match self
            .0
            .compare_exchange(IDLE, DRAINING, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => Action::Begin,
            Err(READY) => Action::Allow,
            Err(_) => Action::Prevent,
        }
    }
    fn complete(&self, result: Result<()>) {
        let next = if result.is_ok() { READY } else { FAILED };
        let _ = self
            .0
            .compare_exchange(DRAINING, next, Ordering::SeqCst, Ordering::SeqCst);
    }
}
pub(super) fn requested(handle: &tauri::AppHandle, api: &tauri::ExitRequestApi, code: Option<i32>) {
    let state = handle.state::<Workspace>();
    match state.0.exit.request() {
        Action::Allow => {}
        Action::Prevent => api.prevent_exit(),
        Action::Begin => {
            api.prevent_exit();
            eprintln!("workspace.shutdown_started");
            let labels = state.0.window_drains.native_labels();
            // Revoke every window synchronously; independently owned cleanup follows.
            let _ = state.0.window_drains.close();
            // Proactively close owned windows; exit permission itself must wait for Destroyed.
            for label in labels {
                if let Some(window) = handle.get_webview_window(&label) {
                    let _ = window.close();
                }
            }
            #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
            if let Ok(guard) = state.0.cli.lock()
                && let Some(server) = guard.as_ref()
            {
                server.close();
            }
            let state = (*state).clone();
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let worker_state = state.clone();
                let work = tokio::spawn(async move { physical(&worker_state).await });
                let result = work.await.unwrap_or(Err("workspace.shutdown_failed"));
                state.0.exit.complete(result);
                if result.is_ok() {
                    eprintln!("workspace.shutdown_complete");
                    handle.exit(code.unwrap_or(0));
                } else {
                    // Keep the runtime alive: a failed/unknown receipt is not permission to exit.
                    eprintln!("workspace.shutdown_failed");
                }
            });
        }
    }
}
async fn physical(state: &Workspace) -> Result<()> {
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    let cli = state
        .0
        .cli
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned());
    let native = state.0.sessions.lock().ok().map(|mut root| root.shutdown());
    let native = async move {
        let native = native.ok_or("workspace.shutdown_failed")?;
        native.join().await.map_err(|_| "workspace.shutdown_failed")
    };
    #[cfg(any(unix, windows))]
    let profiles = state.0.profile_storage.shutdown();
    #[cfg(not(any(unix, windows)))]
    let profiles = std::future::ready(Ok::<_, &'static str>(()));
    let (cli, windows, native, profiles, settings) = tokio::join!(
        async {
            #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
            {
                cli.ok_or("workspace.shutdown_failed")?
                    .shutdown()
                    .await
                    .map_err(|_| "workspace.shutdown_failed")
            }
            #[cfg(not(all(feature = "workspace-cli", any(target_os = "linux", windows))))]
            {
                Ok::<_, &'static str>(())
            }
        },
        state.0.window_drains.shutdown(),
        native,
        profiles,
        state.0.settings_io.shutdown()
    );
    // Every physical branch has completed before an error can prevent exit.
    cli?;
    windows?;
    native?;
    settings?;
    eprintln!("workspace.native_drained");
    profiles?;
    eprintln!("workspace.profiles_drained");
    state
        .0
        .registry
        .shutdown()
        .map_err(|_| "workspace.shutdown_failed")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_verified_completion_allows_exit_and_failure_cannot_be_promoted_later() {
        let gate = ExitGate::new();
        assert_eq!(gate.request(), Action::Begin);
        assert_eq!(gate.request(), Action::Prevent);
        gate.complete(Err("simulated unknown cleanup"));
        assert_eq!(gate.request(), Action::Prevent);
        gate.complete(Ok(()));
        assert_eq!(gate.request(), Action::Prevent);
        let gate = ExitGate::new();
        gate.complete(Ok(())); // No spurious completion before admission.
        assert_eq!(gate.request(), Action::Begin);
        gate.complete(Ok(()));
        assert_eq!(gate.request(), Action::Allow);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn actual_live_root_pty_and_profile_database_both_drain_before_exit_ready()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        use pulse_app::{
            local_launch::StoredProfileLaunch,
            local_session_root::{LocalSessionRoot, start_stored_page_local},
        };
        use pulse_pty::{
            launch::LocalSessionEnvironment,
            profiles::{ShellKind, discover_unix_shells},
        };
        use std::{ffi::OsString, fs, os::unix::fs::PermissionsExt};
        use tokio::time::{Duration, timeout};
        let temp = tempfile::tempdir()?;
        let bin = temp.path().join("bin");
        fs::create_dir(&bin)?;
        let script = bin.join("sh");
        // One direct process waiting on PTY input; no user shell configuration.
        fs::write(
            &script,
            b"#!/bin/sh\nexec /bin/sh -c 'printf ready; read -r line'\n",
        )?;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700))?;
        let registry = SessionRegistry::new();
        let window_directory = pulse_app::window_directory::WindowDirectory::new(registry.clone());
        let documents = window_directory.documents();
        let owner = registry.open_window()?;
        documents.install(owner)?;
        documents.started(owner)?;
        let nonce = documents.finished(owner)?;
        documents.confirm(owner, nonce)?;
        let page = documents.current(owner, nonce)?;
        let sessions = Arc::new(Mutex::new(LocalSessionRoot::new(
            registry.clone(),
            tokio::runtime::Handle::current(),
        )));
        let profiles = Arc::new(profiles::ProfileReader::new());
        let directory = temp.path().join("state");
        profiles
            .list(
                registry.clone(),
                page.clone(),
                directory.clone(),
                bin.clone().into_os_string(),
                None,
            )
            .await?;
        let (store, task) = pulse_store::StateStore::open(directory.join("state.db")).await?;
        let candidates = discover_unix_shells(&OsString::from(bin), None)?;
        let context = LocalSessionEnvironment {
            application_version: "test-drain".into(),
            pane: None,
        };
        let spec = StoredProfileLaunch {
            candidates: &candidates,
            kind: ShellKind::Sh,
            context: &context,
            size: pulse_core::TermSize::new(32, 8)?,
            history: 16,
        };
        timeout(
            Duration::from_secs(5),
            start_stored_page_local(&registry, &page, &store, &sessions, spec),
        )
        .await??;
        assert_eq!(sessions.lock().map_err(|_| "root poisoned")?.occupied(), 1);
        task.shutdown().await?; // The second source is fixture-owned, not an app resource.
        #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
        let cli = {
            struct NoActions;
            impl pulse_app::cli_control::Handler for NoActions {
                fn call(
                    &self,
                    _: pulse_ipc::cli::Request,
                    _: tokio_util::sync::CancellationToken,
                ) -> pulse_app::cli_control::Operation {
                    Box::pin(async { Err(pulse_app::cli_control::Error) })
                }
            }
            let directory = temp.path().join("cli");
            let server = pulse_app::cli_control::Server::start(
                &directory,
                &tokio::runtime::Handle::current(),
                Arc::new(NoActions),
            )?;
            (Arc::new(server), directory)
        };
        let documents = documents.clone();
        let application = Arc::new(Application {
            registry,
            documents,
            directory: window_directory,
            sessions,
            runtime: tokio::runtime::Handle::current(),
            exit: ExitGate::new(),
            settings_io: Arc::new(settings::SettingsIo::new()),
            window_drains: Arc::new(app::WindowDrains::new()),
            #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
            cli: Mutex::new(Some(cli.0.clone())),
            profile_storage: profiles.shared_storage(),
        });
        let state = Workspace::from_application(application, owner, profiles)?;
        assert_eq!(state.0.exit.request(), Action::Begin);
        state.retire_window()?;
        let result = timeout(Duration::from_secs(5), physical(&state)).await?;
        state.0.exit.complete(result);
        result?;
        #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
        {
            assert!(!cli.1.join("control.sock").exists());
            assert!(!cli.1.join("control.token").exists());
        }
        assert_eq!(state.0.exit.request(), Action::Allow);
        let mut root = state.0.sessions.lock().map_err(|_| "root poisoned")?;
        root.collect_reaped();
        assert_eq!(root.occupied(), 0);
        Ok(())
    }

    #[test]
    fn concurrent_requests_start_exactly_one_supervisor() {
        let gate = Arc::new(ExitGate::new());
        let requests: Vec<_> = (0..16)
            .map(|_| {
                let gate = gate.clone();
                std::thread::spawn(move || gate.request())
            })
            .collect();
        let mut begins = 0;
        for request in requests {
            match request.join() {
                Ok(Action::Begin) => begins += 1,
                Ok(Action::Prevent) => {}
                other => panic!("invalid exit action: {other:?}"),
            }
        }
        assert_eq!(begins, 1);
    }
}
