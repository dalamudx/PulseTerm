//! Launch preparation from current automatic-profile metadata, not authorization.
use super::{AutoProfileCatalog, ProfileError, ShellKind};
use crate::{
    PtyCommand,
    launch::{
        LocalLaunchError, LocalSessionEnvironment, LocalShellMode, prepare_local_profile_command,
    },
};
use std::{ffi::OsString, path::PathBuf};

/// Literal profile options. Program and shell classification come only from the catalog.
/// This does not select a default profile, persist settings, or grant launch permission.
#[derive(Default)]
pub struct AutoProfileLaunch {
    /// Literal arguments; never split, interpolated or treated as shell source here.
    pub args: Vec<OsString>,
    /// Optional native working directory.
    pub cwd: Option<PathBuf>,
    /// Profile overrides, subject to the shared reserved-identity and size policy.
    pub env: Vec<(OsString, OsString)>,
    /// Inherited keys to remove before defaults and overrides are applied.
    pub env_remove: Vec<OsString>,
    /// Explicit override or the platform default for the trusted catalog classification.
    pub mode: LocalShellMode,
}
impl std::fmt::Debug for AutoProfileLaunch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoProfileLaunch")
            .field("arguments", &self.args.len())
            .field("working_directory", &self.cwd.is_some())
            .field("environment_overrides", &self.env.len())
            .field("environment_removals", &self.env_remove.len())
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}
/// Static errors contain neither selected paths nor option/environment values.
#[derive(Debug, thiserror::Error)]
pub enum ProfileLaunchError {
    /// Requested identity is absent or unavailable in the current catalog.
    #[error(transparent)]
    Profile(#[from] ProfileError),
    /// The common launch policy rejected options, metadata or login mode.
    #[error(transparent)]
    Launch(#[from] LocalLaunchError),
}
impl AutoProfileCatalog {
    /// Prepare a separately authorized explicit profile launch against current metadata.
    /// Hidden profiles may be launched explicitly; missing/deleted profiles never fall back.
    /// Metadata can change before native spawn, so spawn can still fail without a fallback.
    /// No program is executed and no window/session capability is created by this method.
    pub fn prepare_launch(
        &self,
        kind: ShellKind,
        options: AutoProfileLaunch,
        context: &LocalSessionEnvironment,
    ) -> Result<PtyCommand, ProfileLaunchError> {
        let profile = self.profile(kind).ok_or(ProfileError::NotFound)?;
        let mut command = profile.command()?;
        command.args.extend(options.args);
        command.cwd = options.cwd;
        command.env = options.env;
        command.env_remove = options.env_remove;
        Ok(prepare_local_profile_command(
            command,
            context,
            Some(profile.kind()),
            options.mode,
        )?)
    }
}

#[cfg(test)]
#[path = "profile_launch_tests.rs"]
mod tests;
