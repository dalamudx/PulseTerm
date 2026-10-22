//! Bounded Windows executable discovery. Reads metadata only; never executes a candidate.
use super::{MAX_EXECUTABLE_BYTES, MAX_PATH_COMPONENTS, ProfileError, ShellCandidate, ShellKind};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

#[cfg(windows)]
const _: () = assert!((log::STATIC_MAX_LEVEL as usize) <= (log::LevelFilter::Debug as usize));

#[cfg(windows)]
/// Discover against the same best-effort registry-refreshed environment used by Windows PTY spawn.
/// Blocking registry and metadata I/O; callers retain their native I/O credit until completion.
/// No candidate is executed and no process-global environment is changed. Registry read failures
/// retain the PTY dependency's inherited-environment fallback, not a freshness guarantee.
pub fn discover_current_windows_shells(
    system_directory: &Path,
    program_files: Option<&Path>,
    default_distribution: Option<&str>,
) -> Result<Vec<ShellCandidate>, ProfileError> {
    let environment = portable_pty::CommandBuilder::new(system_directory.join("cmd.exe"));
    discover_windows_shells(
        environment
            .get_env("PATH")
            .unwrap_or_else(|| OsStr::new("")),
        system_directory,
        program_files,
        default_distribution,
    )
}

/// Discover Windows shells from backend-owned environment and OS metadata snapshots.
/// System shell paths do not come from PATH. WSL is offered only with a registered default.
/// The default distribution is fixed in literal argv; this grants no launch authority.
pub fn discover_windows_shells(
    path: &OsStr,
    system_directory: &Path,
    program_files: Option<&Path>,
    default_distribution: Option<&str>,
) -> Result<Vec<ShellCandidate>, ProfileError> {
    if path.as_encoded_bytes().len() > 64 * 1024 || path.as_encoded_bytes().contains(&0) {
        return Err(ProfileError::DiscoveryLimit);
    }
    if !system_directory.is_absolute() {
        return Err(ProfileError::InvalidExecutable);
    }
    if program_files.is_some_and(|path| {
        !path.is_absolute() || path.as_os_str().as_encoded_bytes().len() > MAX_EXECUTABLE_BYTES
    }) {
        return Err(ProfileError::InvalidExecutable);
    }
    let mut candidates = BTreeMap::new();
    let mut admit = |kind, executable: PathBuf| -> Result<(), ProfileError> {
        let candidate = ShellCandidate::new(kind, executable)?;
        if !candidates.contains_key(&kind)
            && std::fs::metadata(candidate.executable()).is_ok_and(|file| file.is_file())
        {
            let candidate = if kind == ShellKind::Wsl {
                let Some(distribution) = default_distribution else {
                    return Ok(());
                };
                ShellCandidate::windows_wsl(candidate.executable().to_path_buf(), distribution)?
            } else {
                candidate
            };
            candidates.insert(kind, candidate);
        }
        Ok(())
    };
    admit(ShellKind::Cmd, system_directory.join("cmd.exe"))?;
    admit(
        ShellKind::PowerShell,
        system_directory.join(r"WindowsPowerShell\v1.0\powershell.exe"),
    )?;
    admit(ShellKind::Wsl, system_directory.join("wsl.exe"))?;
    for (index, directory) in std::env::split_paths(path).enumerate() {
        if index >= MAX_PATH_COMPONENTS {
            return Err(ProfileError::DiscoveryLimit);
        }
        if directory.is_absolute() {
            admit(ShellKind::Pwsh, directory.join("pwsh.exe"))?;
        }
    }
    if let Some(directory) = program_files {
        admit(ShellKind::Pwsh, directory.join(r"PowerShell\7\pwsh.exe"))?;
    }
    Ok(candidates.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::AutoProfileCatalog;
    use std::ffi::OsString;
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    fn fixture() -> Result<(tempfile::TempDir, PathBuf, PathBuf), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let system = root.path().join("system");
        let programs = root.path().join("programs");
        for path in [
            system.join("cmd.exe"),
            system.join(r"WindowsPowerShell\v1.0\powershell.exe"),
            system.join("wsl.exe"),
            programs.join(r"PowerShell\7\pwsh.exe"),
        ] {
            std::fs::create_dir_all(path.parent().ok_or("fixture parent missing")?)?;
            std::fs::write(path, b"metadata only, never an executable")?;
        }
        Ok((root, system, programs))
    }
    #[test]
    fn windows_discovery_keeps_shells_distinct_and_pins_default_wsl_arguments() -> TestResult {
        let (_root, system, programs) = fixture()?;
        let candidates = discover_windows_shells(
            OsStr::new(""),
            &system,
            Some(&programs),
            Some("fixture distro"),
        )?;
        assert_eq!(candidates.len(), 4);
        let mut catalog = AutoProfileCatalog::default();
        catalog.reconcile(&candidates)?;
        let command = catalog
            .profile(ShellKind::Wsl)
            .ok_or("missing WSL")?
            .command()?;
        assert_eq!(
            command.args,
            vec![
                OsString::from("--distribution"),
                OsString::from("fixture distro")
            ]
        );
        assert!(catalog.profile(ShellKind::PowerShell).is_some());
        assert!(catalog.profile(ShellKind::Pwsh).is_some());
        Ok(())
    }
    #[test]
    fn wsl_executable_without_a_registered_default_is_not_an_available_profile() -> TestResult {
        let (_root, system, programs) = fixture()?;
        let candidates = discover_windows_shells(OsStr::new(""), &system, Some(&programs), None)?;
        assert_eq!(candidates.len(), 3);
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.kind() == ShellKind::Wsl)
        );
        Ok(())
    }
    #[test]
    fn relative_system_locations_and_unbounded_paths_are_rejected() -> TestResult {
        let (_root, system, _) = fixture()?;
        assert!(
            discover_windows_shells(OsStr::new(""), Path::new("relative"), None, None).is_err()
        );
        assert!(matches!(
            discover_windows_shells(OsStr::new(&"x".repeat(64 * 1024 + 1)), &system, None, None),
            Err(ProfileError::DiscoveryLimit)
        ));
        Ok(())
    }
    #[test]
    fn changed_path_moves_pwsh_without_losing_visibility_or_reviving_deleted_profiles() -> TestResult
    {
        let (root, system, _) = fixture()?;
        let old = root.path().join("old");
        let new = root.path().join("new");
        for directory in [&old, &new] {
            std::fs::create_dir(directory)?;
            std::fs::write(directory.join("pwsh.exe"), b"metadata only; must never run")?;
        }
        let discover = |directory: &Path| -> Result<_, Box<dyn std::error::Error>> {
            Ok(discover_windows_shells(
                &std::env::join_paths([directory])?,
                &system,
                None,
                None,
            )?)
        };
        let mut catalog = AutoProfileCatalog::default();
        catalog.reconcile(&discover(&old)?)?;
        catalog.set_hidden(ShellKind::Pwsh, true)?;
        catalog.reconcile(&discover(&new)?)?;
        let profile = catalog.profile(ShellKind::Pwsh).ok_or("missing pwsh")?;
        assert_eq!(profile.command()?.program, new.join("pwsh.exe"));
        assert!(profile.is_hidden());
        catalog.remove(ShellKind::Pwsh)?;
        catalog.reconcile(&discover(&old)?)?;
        assert!(catalog.profile(ShellKind::Pwsh).is_none());
        Ok(())
    }
}
