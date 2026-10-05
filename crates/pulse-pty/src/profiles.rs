//! Local automatic profiles. Discovery reads metadata, never executes a candidate.
//! Stable kind identities keep deleted profiles suppressed even after PATH changes.
use crate::PtyCommand;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Maximum supported Unix shell kinds in one discovery snapshot.
pub const MAX_SHELLS: usize = 5;
/// Maximum PATH components considered by a Unix discovery pass.
pub const MAX_PATH_COMPONENTS: usize = 64;
/// Maximum encoded bytes in one executable path.
pub const MAX_EXECUTABLE_BYTES: usize = 4096;

/// A stable automatic Unix profile identity, independent of symlinks and PATH order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShellKind {
    /// Bourne Again Shell.
    Bash,
    /// Z shell.
    Zsh,
    /// Friendly interactive shell.
    Fish,
    /// POSIX shell; preserve the `sh` invocation name, not its symlink target.
    Sh,
    /// PowerShell Core, not Windows PowerShell.
    Pwsh,
}
impl ShellKind {
    /// Literal discovery filename, not a shell command.
    pub const fn executable_name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Sh => "sh",
            Self::Pwsh => "pwsh",
        }
    }
}
#[cfg(unix)]
const KINDS: [ShellKind; MAX_SHELLS] = [
    ShellKind::Bash,
    ShellKind::Zsh,
    ShellKind::Fish,
    ShellKind::Sh,
    ShellKind::Pwsh,
];

/// A backend-discovered executable; construction does not grant launch permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCandidate {
    kind: ShellKind,
    executable: PathBuf,
}
impl ShellCandidate {
    /// Check a trusted discovery result before reconciling profiles. No I/O.
    pub fn new(kind: ShellKind, executable: PathBuf) -> Result<Self, ProfileError> {
        if !executable.is_absolute()
            || executable.as_os_str().as_encoded_bytes().len() > MAX_EXECUTABLE_BYTES
            || executable.as_os_str().as_encoded_bytes().contains(&0)
        {
            return Err(ProfileError::InvalidExecutable);
        }
        Ok(Self { kind, executable })
    }
    /// Stable automatic identity.
    pub fn kind(&self) -> ShellKind {
        self.kind
    }
    /// Literal executable path, retaining invocation-name semantics.
    pub fn executable(&self) -> &Path {
        &self.executable
    }
}

/// Stable errors intentionally contain no user paths, environment values or argv.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProfileError {
    /// Absolute non-NUL executable path within the byte budget is required.
    #[error("profile.invalid_executable")]
    InvalidExecutable,
    /// Discovery or snapshot exceeds its finite budget.
    #[error("profile.discovery_limit")]
    DiscoveryLimit,
    /// A discovery snapshot must have at most one candidate per shell kind.
    #[error("profile.duplicate_kind")]
    DuplicateKind,
    /// No automatic profile currently has this identity.
    #[error("profile.not_found")]
    NotFound,
    /// An installed shell disappeared from the latest snapshot.
    #[error("profile.unavailable")]
    Unavailable,
}

/// Discovered automatic profile metadata, separate from user-authored profiles.
#[derive(Debug, Clone)]
pub struct AutoProfile {
    candidate: ShellCandidate,
    hidden: bool,
    available: bool,
}
impl AutoProfile {
    /// Stable identity used for persistence and deletion tombstones.
    pub fn kind(&self) -> ShellKind {
        self.candidate.kind
    }
    /// Detected program; no implicit shell interpolation or canonical-target rewrite.
    pub fn executable(&self) -> &Path {
        self.candidate.executable()
    }
    /// User-selected visibility survives discovery refreshes.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }
    /// Whether the latest discovery snapshot contained this shell.
    pub fn is_available(&self) -> bool {
        self.available
    }
    /// Construct literal argv for a separately authorized backend launch.
    /// Does not launch, add login options, or refresh Windows environment variables.
    pub fn command(&self) -> Result<PtyCommand, ProfileError> {
        if !self.available {
            return Err(ProfileError::Unavailable);
        }
        Ok(PtyCommand::new(self.candidate.executable.clone()))
    }
}

/// Backend-owned metadata. Persist `deleted()` separately to suppress reappearance.
/// This is not a settings database or a session authorization API.
#[derive(Debug, Default)]
pub struct AutoProfileCatalog {
    profiles: BTreeMap<ShellKind, AutoProfile>,
    deleted: BTreeSet<ShellKind>,
}
impl AutoProfileCatalog {
    /// Restore deletion tombstones from trusted persisted settings.
    pub fn new(deleted: impl IntoIterator<Item = ShellKind>) -> Self {
        Self {
            profiles: BTreeMap::new(),
            deleted: deleted.into_iter().collect(),
        }
    }
    /// Atomic snapshot reconciliation. Failed admission leaves all metadata unchanged.
    /// Available programs may change; hidden state and deletion tombstones do not.
    pub fn reconcile(&mut self, candidates: &[ShellCandidate]) -> Result<(), ProfileError> {
        if candidates.len() > MAX_SHELLS {
            return Err(ProfileError::DiscoveryLimit);
        }
        let mut kinds = BTreeSet::new();
        for candidate in candidates {
            if !kinds.insert(candidate.kind) {
                return Err(ProfileError::DuplicateKind);
            }
        }
        for profile in self.profiles.values_mut() {
            profile.available = false;
        }
        for candidate in candidates {
            if self.deleted.contains(&candidate.kind) {
                continue;
            }
            self.profiles
                .entry(candidate.kind)
                .and_modify(|profile| {
                    profile.candidate = candidate.clone();
                    profile.available = true;
                })
                .or_insert_with(|| AutoProfile {
                    candidate: candidate.clone(),
                    hidden: false,
                    available: true,
                });
        }
        Ok(())
    }
    /// Look up an automatic profile without exposing mutation of its identity.
    pub fn profile(&self, kind: ShellKind) -> Option<&AutoProfile> {
        self.profiles.get(&kind)
    }
    /// Iterate over both available and temporarily missing automatic profiles.
    pub fn profiles(&self) -> impl Iterator<Item = &AutoProfile> {
        self.profiles.values()
    }
    /// Hide or unhide without removing the profile or disabling explicit launches.
    pub fn set_hidden(&mut self, kind: ShellKind, hidden: bool) -> Result<(), ProfileError> {
        self.profiles
            .get_mut(&kind)
            .ok_or(ProfileError::NotFound)?
            .hidden = hidden;
        Ok(())
    }
    /// Remove an existing profile and retain its tombstone across later discoveries.
    pub fn remove(&mut self, kind: ShellKind) -> Result<(), ProfileError> {
        self.profiles.remove(&kind).ok_or(ProfileError::NotFound)?;
        self.deleted.insert(kind);
        Ok(())
    }
    /// Snapshot tombstones for a trusted store; reinstallation/PATH moves do not reset them.
    pub fn deleted(&self) -> impl Iterator<Item = ShellKind> + '_ {
        self.deleted.iter().copied()
    }
}

/// Discover known Unix shells from a supplied local environment snapshot.
/// Blocking metadata I/O: callers must use a dedicated worker, not a UI/runtime thread.
/// Relative/empty PATH entries, directories and files without execute bits are ignored.
/// A recognized absolute preferred shell wins for that kind; no candidate is executed.
#[cfg(unix)]
pub fn discover_unix_shells(
    path: &std::ffi::OsStr,
    preferred_shell: Option<&Path>,
) -> Result<Vec<ShellCandidate>, ProfileError> {
    use std::os::unix::fs::PermissionsExt;
    if path.as_encoded_bytes().len() > 64 * 1024 || path.as_encoded_bytes().contains(&0) {
        return Err(ProfileError::DiscoveryLimit);
    }
    let mut candidates = BTreeMap::new();
    let mut admit = |kind, program: PathBuf| -> Result<(), ProfileError> {
        if !program.is_absolute() {
            return Ok(());
        }
        let candidate = ShellCandidate::new(kind, program)?;
        if candidates.contains_key(&kind) {
            return Ok(());
        }
        if let Ok(metadata) = std::fs::metadata(candidate.executable())
            && metadata.is_file()
            && metadata.permissions().mode() & 0o111 != 0
        {
            candidates.insert(kind, candidate);
        }
        Ok(())
    };
    if let Some(program) = preferred_shell
        && let Some(kind) = KINDS.into_iter().find(|kind| {
            program
                .file_name()
                .is_some_and(|name| name == kind.executable_name())
        })
    {
        admit(kind, program.to_path_buf())?;
    }
    for (index, directory) in std::env::split_paths(path).enumerate() {
        if index >= MAX_PATH_COMPONENTS {
            return Err(ProfileError::DiscoveryLimit);
        }
        if directory.is_absolute() {
            for kind in KINDS {
                admit(kind, directory.join(kind.executable_name()))?;
            }
        }
    }
    for directory in ["/usr/local/bin", "/usr/bin", "/bin"] {
        for kind in KINDS {
            admit(kind, Path::new(directory).join(kind.executable_name()))?;
        }
    }
    Ok(candidates.into_values().collect())
}

#[cfg(test)]
#[path = "profiles_tests.rs"]
mod tests;
