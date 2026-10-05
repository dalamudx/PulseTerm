//! Prepare local launch metadata, not permissions, login argv0, or a session binding.
use crate::{PtyCommand, PtyError, profiles::ShellKind};
use std::ffi::{OsStr, OsString};
/// Backend-owned CLI routing metadata; a profile must not provide these values.
#[derive(Clone)]
pub struct PaneEnvironment {
    /// Backend-assigned pane identifier, never a profile-provided authorization token.
    pub pane_id: String,
    /// Backend-selected local CLI endpoint; no credentials or network route.
    pub socket: OsString,
}
/// Metadata supplied by the trusted local-session adapter on every new launch.
#[derive(Clone)]
pub struct LocalSessionEnvironment {
    /// Version of the running local application, bounded to 64 ASCII bytes.
    pub application_version: String,
    /// Current CLI routing pair, or None to clear inherited routing metadata.
    pub pane: Option<PaneEnvironment>,
}
impl std::fmt::Debug for PaneEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneEnvironment").finish_non_exhaustive()
    }
}
impl std::fmt::Debug for LocalSessionEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSessionEnvironment")
            .field("cli_bound", &self.pane.is_some())
            .finish_non_exhaustive()
    }
}
/// Static launch-preparation errors never include profile or runtime values.
#[derive(Debug, thiserror::Error)]
pub enum LocalLaunchError {
    /// Explicit argv/environment failed the underlying bounded pre-allocation checks.
    #[error("local.invalid_command")]
    Command(#[from] PtyError),
    /// Profile attempted to override/remove application or CLI runtime identity.
    #[error("local.reserved_environment")]
    ReservedEnvironment,
    /// Trusted adapter metadata is malformed, empty, or exceeds its own bound.
    #[error("local.invalid_environment_context")]
    InvalidContext,
    /// Login arguments are supported only for explicitly classified Unix bash/zsh/fish.
    #[error("local.unsupported_login_shell")]
    UnsupportedLoginShell,
}
const RESERVED: [&str; 4] = [
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "PULSE_PANE_ID",
    "PULSE_SOCKET",
];
fn reserved(key: &OsStr) -> bool {
    key.to_str()
        .is_some_and(|key| RESERVED.iter().any(|name| key.eq_ignore_ascii_case(name)))
}
/// Prepare owned literal argv/cwd and terminal defaults. Does not execute a process.
/// Explicit profile TERM/COLORTERM/environment values win over defaults, except
/// reserved identity fields. Old inherited PULSE keys are always removed; a new
/// pair is injected only from `context.pane`. Removal precedes explicit overrides.
/// `Pwsh` prepends -NoLogo and defaults POWERSHELL_UPDATECHECK=Off. Custom shell
/// classification must come from the trusted adapter, not executable text matching.
pub fn prepare_local_command(
    mut command: PtyCommand,
    context: &LocalSessionEnvironment,
    shell: Option<ShellKind>,
) -> Result<PtyCommand, LocalLaunchError> {
    command.validate()?;
    if command.env.iter().any(|(key, _)| reserved(key))
        || command.env_remove.iter().any(|key| reserved(key))
    {
        return Err(LocalLaunchError::ReservedEnvironment);
    }
    let version = &context.application_version;
    if version.is_empty()
        || version.len() > 64
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b))
    {
        return Err(LocalLaunchError::InvalidContext);
    }
    if let Some(pane) = &context.pane
        && (pane.pane_id.is_empty()
            || pane.pane_id.len() > 128
            || !pane
                .pane_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
            || pane.socket.is_empty()
            || pane.socket.as_encoded_bytes().len() > 4096
            || pane.socket.as_encoded_bytes().contains(&0))
    {
        return Err(LocalLaunchError::InvalidContext);
    }
    let profile = std::mem::take(&mut command.env);
    command.env = vec![
        ("TERM".into(), "xterm-256color".into()),
        ("COLORTERM".into(), "truecolor".into()),
        ("TERM_PROGRAM".into(), "PulseTerm".into()),
        ("TERM_PROGRAM_VERSION".into(), version.into()),
    ];
    if shell == Some(ShellKind::Pwsh) {
        if !command.args.first().is_some_and(|arg| {
            arg.to_str()
                .is_some_and(|arg| arg.eq_ignore_ascii_case("-NoLogo"))
        }) {
            command.args.insert(0, "-NoLogo".into());
        }
        command
            .env
            .push(("POWERSHELL_UPDATECHECK".into(), "Off".into()));
    }
    command.env.extend(profile);
    command
        .env_remove
        .extend(["PULSE_PANE_ID".into(), "PULSE_SOCKET".into()]);
    if let Some(pane) = &context.pane {
        command
            .env
            .push(("PULSE_PANE_ID".into(), pane.pane_id.clone().into()));
        command
            .env
            .push(("PULSE_SOCKET".into(), pane.socket.clone()));
    }
    command.validate()?;
    Ok(command)
}

/// Request parameter-based login for a trusted Unix bash/zsh/fish classification.
/// Bash uses --login so following long options remain valid; zsh/fish use -l.
/// Never rewrites argv0, picks a default executable, or splits user arguments.
/// Only an existing leading `-l`/`--login` avoids insertion; later arguments can
/// be script data and are not scanned. User options may still override login mode.
/// Unknown/POSIX sh/pwsh classifications and non-Unix platforms fail explicitly.
/// Application/CLI environment policy and final command budgets remain enforced.
pub fn prepare_local_login_command(
    mut command: PtyCommand,
    context: &LocalSessionEnvironment,
    shell: Option<ShellKind>,
) -> Result<PtyCommand, LocalLaunchError> {
    command.validate()?;
    if !cfg!(unix) {
        return Err(LocalLaunchError::UnsupportedLoginShell);
    }
    let flag = match shell {
        Some(ShellKind::Bash) => "--login",
        Some(ShellKind::Zsh | ShellKind::Fish) => "-l",
        _ => return Err(LocalLaunchError::UnsupportedLoginShell),
    };
    if !command
        .args
        .first()
        .is_some_and(|arg| arg == "-l" || arg == "--login")
    {
        command.args.insert(0, flag.into());
    }
    prepare_local_command(command, context, shell)
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "login_tests.rs"]
mod login_tests;
