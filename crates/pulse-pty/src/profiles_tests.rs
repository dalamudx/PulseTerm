#![allow(clippy::unwrap_used)]
use super::*;
fn candidate(kind: ShellKind, name: &str) -> ShellCandidate {
    ShellCandidate::new(kind, std::env::temp_dir().join(name)).unwrap()
}
#[test]
fn discovery_snapshot_admission_is_atomic_and_bounded() {
    let mut catalog = AutoProfileCatalog::default();
    let bash = candidate(ShellKind::Bash, "old-bash");
    catalog.reconcile(std::slice::from_ref(&bash)).unwrap();
    let duplicates = [
        candidate(ShellKind::Bash, "new-bash"),
        candidate(ShellKind::Bash, "other-bash"),
    ];
    assert_eq!(
        catalog.reconcile(&duplicates),
        Err(ProfileError::DuplicateKind)
    );
    assert_eq!(
        catalog.profile(ShellKind::Bash).unwrap().executable(),
        bash.executable()
    );
    assert!(catalog.profile(ShellKind::Bash).unwrap().is_available());
    assert_eq!(
        catalog.reconcile(&vec![bash; MAX_SHELLS + 1]),
        Err(ProfileError::DiscoveryLimit)
    );
    assert!(catalog.profile(ShellKind::Bash).unwrap().is_available());
}
#[test]
fn hidden_state_survives_path_changes_and_disappearance() {
    let mut catalog = AutoProfileCatalog::default();
    catalog
        .reconcile(&[candidate(ShellKind::Bash, "old-bash")])
        .unwrap();
    catalog.set_hidden(ShellKind::Bash, true).unwrap();
    let changed = candidate(ShellKind::Bash, "new-bash");
    catalog.reconcile(std::slice::from_ref(&changed)).unwrap();
    let p = catalog.profile(ShellKind::Bash).unwrap();
    assert!(p.is_hidden());
    assert_eq!(p.executable(), changed.executable());
    assert_eq!(
        p.command().unwrap().program,
        changed.executable().as_os_str()
    );
    catalog.reconcile(&[]).unwrap();
    let p = catalog.profile(ShellKind::Bash).unwrap();
    assert!(p.is_hidden());
    assert!(!p.is_available());
    assert!(matches!(p.command(), Err(ProfileError::Unavailable)));
    catalog.reconcile(&[changed]).unwrap();
    assert!(catalog.profile(ShellKind::Bash).unwrap().is_hidden());
    catalog.set_hidden(ShellKind::Bash, false).unwrap();
    assert!(!catalog.profile(ShellKind::Bash).unwrap().is_hidden());
}
#[test]
fn deletion_is_stable_across_reinstallation_and_tombstone_restore() {
    let mut catalog = AutoProfileCatalog::default();
    catalog
        .reconcile(&[
            candidate(ShellKind::Bash, "old-bash"),
            candidate(ShellKind::Fish, "fish"),
        ])
        .unwrap();
    catalog.remove(ShellKind::Bash).unwrap();
    catalog
        .reconcile(&[candidate(ShellKind::Bash, "moved-bash")])
        .unwrap();
    assert!(catalog.profile(ShellKind::Bash).is_none());
    assert_eq!(catalog.deleted().collect::<Vec<_>>(), [ShellKind::Bash]);
    let mut restored = AutoProfileCatalog::new(catalog.deleted());
    restored
        .reconcile(&[
            candidate(ShellKind::Bash, "yet-another-bash"),
            candidate(ShellKind::Zsh, "zsh"),
        ])
        .unwrap();
    assert!(restored.profile(ShellKind::Bash).is_none());
    assert!(restored.profile(ShellKind::Zsh).unwrap().is_available());
    assert!(catalog.profile(ShellKind::Fish).is_some());
    assert_eq!(
        restored.set_hidden(ShellKind::Bash, true),
        Err(ProfileError::NotFound)
    );
    assert_eq!(
        restored.remove(ShellKind::Bash),
        Err(ProfileError::NotFound)
    );
}
#[test]
fn invalid_paths_and_error_display_do_not_expose_values() {
    for path in [
        PathBuf::new(),
        PathBuf::from("relative-shell"),
        std::env::temp_dir().join("secret\0value"),
        std::env::temp_dir().join("x".repeat(MAX_EXECUTABLE_BYTES)),
    ] {
        let error = ShellCandidate::new(ShellKind::Bash, path).unwrap_err();
        assert_eq!(error, ProfileError::InvalidExecutable);
        assert_eq!(error.to_string(), "profile.invalid_executable");
    }
}
#[test]
fn literal_program_with_punctuation_is_not_shell_source() {
    let candidate = candidate(ShellKind::Bash, "literal ; $(not-executed) 中文");
    let mut catalog = AutoProfileCatalog::default();
    catalog.reconcile(std::slice::from_ref(&candidate)).unwrap();
    let command = catalog.profile(ShellKind::Bash).unwrap().command().unwrap();
    assert_eq!(command.program, candidate.executable().as_os_str());
    assert!(command.args.is_empty());
    assert!(command.env.is_empty());
    assert!(command.cwd.is_none());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let root = std::env::temp_dir();
            for _ in 0..32 {
                let path = root.join(format!(
                    "pulseterm-profile-tests-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => return Self(path),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("private fixture: {e}"),
                }
            }
            panic!("private fixture name exhausted");
        }
        fn file(&self, name: &str, mode: u32) -> PathBuf {
            let p = self.0.join(name);
            std::fs::write(
                &p,
                b"#!/bin/sh\nprintf executed > \"$(dirname \"$0\")/executed\"\n",
            )
            .unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            p
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn metadata_only_discovery_obeys_path_precedence_and_preferred_shell() {
        let first = Directory::new();
        let second = Directory::new();
        let first_bash = first.file("bash", 0o700);
        let second_bash = second.file("bash", 0o700);
        let path = std::env::join_paths([&first.0, &second.0]).unwrap();
        let found = discover_unix_shells(&path, None).unwrap();
        assert_eq!(
            found
                .iter()
                .find(|p| p.kind() == ShellKind::Bash)
                .unwrap()
                .executable(),
            first_bash
        );
        let preferred = discover_unix_shells(&path, Some(&second_bash)).unwrap();
        assert_eq!(
            preferred
                .iter()
                .find(|p| p.kind() == ShellKind::Bash)
                .unwrap()
                .executable(),
            second_bash
        );
        assert!(!first.0.join("executed").exists());
        assert!(!second.0.join("executed").exists());
        assert!(found.len() <= MAX_SHELLS);
    }
    #[test]
    fn directories_nonexecutables_unknown_and_relative_entries_are_ignored() {
        let directory = Directory::new();
        let noexec = directory.file("fish", 0o600);
        std::fs::create_dir(directory.0.join("zsh")).unwrap();
        let unknown = directory.file("unknown", 0o700);
        let path = std::env::join_paths([
            Path::new(""),
            Path::new("."),
            Path::new("relative"),
            &directory.0,
        ])
        .unwrap();
        let found = discover_unix_shells(&path, Some(&unknown)).unwrap();
        assert!(found.iter().all(|p| p.executable() != noexec
            && p.executable() != directory.0.join("zsh")
            && p.executable() != unknown));
        assert!(found.iter().all(|p| p.executable().is_absolute()));
        assert!(!directory.0.join("executed").exists());
    }
    #[test]
    fn shell_invocation_name_is_not_replaced_with_symlink_target() {
        let directory = Directory::new();
        let bash = directory.file("bash", 0o700);
        let sh = directory.0.join("sh");
        std::os::unix::fs::symlink(&bash, &sh).unwrap();
        let found = discover_unix_shells(directory.0.as_os_str(), Some(&sh)).unwrap();
        let selected = found.iter().find(|p| p.kind() == ShellKind::Sh).unwrap();
        assert_eq!(selected.executable(), sh);
        assert_ne!(selected.executable(), std::fs::canonicalize(&sh).unwrap());
        let mut catalog = AutoProfileCatalog::default();
        catalog.reconcile(&found).unwrap();
        assert_eq!(
            catalog
                .profile(ShellKind::Sh)
                .unwrap()
                .command()
                .unwrap()
                .build()
                .unwrap()
                .get_argv()[0],
            sh.as_os_str()
        );
    }
    #[test]
    fn oversized_or_nul_environment_snapshot_fails_instead_of_partial_results() {
        let oversized = std::ffi::OsString::from("x".repeat(65537));
        assert_eq!(
            discover_unix_shells(&oversized, None),
            Err(ProfileError::DiscoveryLimit)
        );
        let too_many =
            std::env::join_paths(vec![PathBuf::from("/tmp"); MAX_PATH_COMPONENTS + 1]).unwrap();
        assert_eq!(
            discover_unix_shells(&too_many, None),
            Err(ProfileError::DiscoveryLimit)
        );
        assert_eq!(
            discover_unix_shells(std::ffi::OsStr::new("/tmp\0secret"), None),
            Err(ProfileError::DiscoveryLimit)
        );
    }
}
