//! Opt-in fixed-code diagnostics for the primary native startup, never an IPC API.
#[derive(Clone, Copy)]
pub(super) enum Stage {
    RunEntered,
    ConfigurationDenied,
    ServicesReady,
    ServicesFailed,
    RuntimeBuildStarted,
    SetupEntered,
    InstanceVerified,
    DataPrepared,
    WindowBuildStarted,
    WindowBuilt,
    PageStarted,
    LocalPageFinished,
    BlankPageFinished,
    OtherPageFinished,
    PageRejected,
    NonceInjected,
    DocumentConfirmEntered,
    DocumentConfirmed,
    VisibilityRead,
    #[cfg(any(unix, windows))]
    ProfilesRequested,
    #[cfg(any(unix, windows))]
    ProfilesListed,
    CliPageReady,
    VisibilityInstalled,
    WindowPublished,
    EndpointPublished,
    SetupFinished,
    RuntimeBuilt,
    RuntimeBuildFailed,
    Ready,
    ExitRequested,
    WindowDestroyed,
    Exited,
}

pub(super) fn record(stage: Stage) {
    #[cfg(all(feature = "workspace-startup-diagnostics", target_os = "linux"))]
    enabled::record(stage);
    #[cfg(not(all(feature = "workspace-startup-diagnostics", target_os = "linux")))]
    let _ = stage;
}

#[cfg(all(feature = "workspace-startup-diagnostics", target_os = "linux"))]
mod enabled {
    use super::Stage;
    use std::{
        fs::{self, File, OpenOptions},
        io::{self, Write},
        os::unix::fs::OpenOptionsExt,
        path::Path,
        sync::{Mutex, OnceLock},
        time::Instant,
    };
    const LIMIT: usize = 64;
    static TRACE: OnceLock<Option<Mutex<Trace>>> = OnceLock::new();

    impl Stage {
        fn code(self) -> &'static str {
            match self {
                Self::RunEntered => "run_entered",
                Self::ConfigurationDenied => "configuration_denied",
                Self::ServicesReady => "services_ready",
                Self::ServicesFailed => "services_failed",
                Self::RuntimeBuildStarted => "runtime_build_started",
                Self::SetupEntered => "setup_entered",
                Self::InstanceVerified => "instance_verified",
                Self::DataPrepared => "data_prepared",
                Self::WindowBuildStarted => "window_build_started",
                Self::WindowBuilt => "window_built",
                Self::PageStarted => "page_started",
                Self::LocalPageFinished => "local_page_finished",
                Self::BlankPageFinished => "blank_page_finished",
                Self::OtherPageFinished => "other_page_finished",
                Self::PageRejected => "page_rejected",
                Self::NonceInjected => "nonce_injected",
                Self::DocumentConfirmEntered => "document_confirm_entered",
                Self::DocumentConfirmed => "document_confirmed",
                Self::VisibilityRead => "visibility_read",
                Self::ProfilesRequested => "profiles_requested",
                Self::ProfilesListed => "profiles_listed",
                Self::CliPageReady => "cli_page_ready",
                Self::VisibilityInstalled => "visibility_installed",
                Self::WindowPublished => "window_published",
                Self::EndpointPublished => "endpoint_published",
                Self::SetupFinished => "setup_finished",
                Self::RuntimeBuilt => "runtime_built",
                Self::RuntimeBuildFailed => "runtime_build_failed",
                Self::Ready => "ready",
                Self::ExitRequested => "exit_requested",
                Self::WindowDestroyed => "window_destroyed",
                Self::Exited => "exited",
            }
        }
    }
    struct Trace {
        file: File,
        start: Instant,
        entries: usize,
    }
    impl Trace {
        fn open(directory: &Path) -> io::Result<Self> {
            // Require an existing private directory. Do not create a caller's tree.
            if !fs::symlink_metadata(directory)?.is_dir() {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            let admitted = pulse_store::state_directory::prepare(directory)
                .map_err(|_| io::Error::from(io::ErrorKind::PermissionDenied))?;
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(admitted.join(format!("startup-{}.jsonl", std::process::id())))?;
            Ok(Self {
                file,
                start: Instant::now(),
                entries: 0,
            })
        }
        fn write(&mut self, stage: Stage) -> io::Result<()> {
            if self.entries >= LIMIT {
                return Ok(());
            }
            self.entries += 1;
            let result = writeln!(
                self.file,
                "{{\"version\":1,\"pid\":{},\"elapsed_us\":{},\"stage\":\"{}\"}}",
                std::process::id(),
                self.start.elapsed().as_micros(),
                stage.code()
            );
            if result.is_err() {
                self.entries = LIMIT;
            }
            result
        }
    }
    pub(super) fn record(stage: Stage) {
        let trace = TRACE.get_or_init(|| {
            let directory = std::env::var_os("PULSE_WORKSPACE_STARTUP_TRACE_DIR")?;
            Trace::open(Path::new(&directory)).ok().map(Mutex::new)
        });
        if let Some(trace) = trace
            && let Ok(mut trace) = trace.lock()
        {
            let _ = trace.write(stage);
        }
    }
    #[cfg(test)]
    mod tests {
        #![allow(clippy::unwrap_used)]
        use super::*;
        use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
        fn private_root() -> tempfile::TempDir {
            let root = tempfile::tempdir().unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            root
        }
        fn path(directory: &Path) -> std::path::PathBuf {
            directory.join(format!("startup-{}.jsonl", std::process::id()))
        }
        #[test]
        fn fixed_schema_private_file_and_monotonic_bounded_events() {
            let root = private_root();
            let mut trace = Trace::open(root.path()).unwrap();
            trace.write(Stage::RunEntered).unwrap();
            for _ in 0..LIMIT + 4 {
                trace.write(Stage::PageStarted).unwrap();
            }
            let text = fs::read_to_string(path(root.path())).unwrap();
            let mut previous = 0;
            assert_eq!(text.lines().count(), LIMIT);
            for line in text.lines() {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(value.as_object().unwrap().len(), 4);
                assert_eq!(value["version"], 1);
                assert_eq!(value["pid"], std::process::id());
                let elapsed = value["elapsed_us"].as_u64().unwrap();
                assert!(elapsed >= previous);
                previous = elapsed;
                assert!(matches!(
                    value["stage"].as_str(),
                    Some("run_entered" | "page_started")
                ));
            }
            let metadata = fs::metadata(path(root.path())).unwrap();
            assert_eq!(metadata.mode() & 0o7777, 0o600);
            assert_eq!(metadata.nlink(), 1);
            assert_eq!(metadata.uid(), fs::metadata(root.path()).unwrap().uid());
        }
        #[test]
        fn refuses_existing_file_and_leaf_symlink_without_overwrite() {
            let root = private_root();
            let target = root.path().join("existing");
            fs::write(&target, b"preserve").unwrap();
            fs::write(path(root.path()), b"preserve").unwrap();
            assert!(Trace::open(root.path()).is_err());
            assert_eq!(fs::read(path(root.path())).unwrap(), b"preserve");
            fs::remove_file(path(root.path())).unwrap();
            symlink(&target, path(root.path())).unwrap();
            assert!(Trace::open(root.path()).is_err());
            assert_eq!(fs::read(target).unwrap(), b"preserve");
        }
        #[test]
        fn refuses_missing_relative_and_symlinked_directories() {
            let root = private_root();
            let missing = root.path().join("missing");
            assert!(Trace::open(&missing).is_err());
            assert!(!missing.exists());
            assert!(Trace::open(Path::new(".")).is_err());
            let alias = root.path().join("alias");
            symlink(root.path(), &alias).unwrap();
            assert!(Trace::open(&alias).is_err());
        }
        #[test]
        fn refuses_unsafe_existing_directory_without_chmod() {
            let root = private_root();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
            assert!(Trace::open(root.path()).is_err());
            assert_eq!(fs::metadata(root.path()).unwrap().mode() & 0o7777, 0o755);
            assert!(!path(root.path()).exists());
        }
    }
}
