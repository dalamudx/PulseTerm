//! Validation-only bootstrap: known Bash execs exactly our finite helper.
use pulse_pty::PtyCommand;
use std::ffi::OsString;

const MARKER: &str = "PULSE_PROFILE_LOGIN_CHECKED";

pub(super) fn prepare(original: PtyCommand) -> super::Result<PtyCommand> {
    if original.program
        != std::env::current_exe()
            .map_err(super::error)?
            .into_os_string()
        || original.args != [OsString::from("--session-probe-child")]
        || original.cwd.is_some()
        || !original.env.is_empty()
        || !original.env_remove.is_empty()
    {
        return Err("session.profile_requires_fixed_helper".into());
    }
    #[cfg(not(unix))]
    {
        let _ = original;
        Err("session.profile_requires_unix".into())
    }
    #[cfg(unix)]
    {
        use pulse_pty::launch::{
            LocalSessionEnvironment, LocalShellMode, prepare_local_profile_command,
        };
        use pulse_pty::profiles::{ShellKind, discover_unix_shells};
        let bash = discover_unix_shells(std::ffi::OsStr::new("/bin:/usr/bin"), None)
            .map_err(super::error)?
            .into_iter()
            .find(|p| p.kind() == ShellKind::Bash)
            .ok_or("session.profile_bash_missing")?;
        // No dynamic shell source; the executable and helper argument are literal $@.
        const SCRIPT: &str = "builtin shopt -q login_shell || exit 93; builtin test \"$TERM_PROGRAM\" = PulseTerm || exit 94; export PULSE_PROFILE_LOGIN_CHECKED=1; exec \"$@\"";
        let mut command = PtyCommand::new(bash.executable());
        command.args = vec![
            "--noprofile".into(),
            "--norc".into(),
            "-c".into(),
            SCRIPT.into(),
            "pulseterm-window-profile".into(),
            original.program,
        ];
        command.env.push((MARKER.into(), "0".into()));
        command.args.extend(original.args);
        command
            .env_remove
            .extend(["BASH_ENV", "ENV", "SHELLOPTS", "BASHOPTS"].map(OsString::from));
        prepare_local_profile_command(
            command,
            &LocalSessionEnvironment {
                application_version: env!("CARGO_PKG_VERSION").into(),
                pane: None,
            },
            Some(ShellKind::Bash),
            LocalShellMode::Login,
        )
        .map_err(super::error)
    }
}

pub(super) fn check_child() -> std::io::Result<()> {
    check_marker(std::env::var_os(MARKER))
}
fn check_marker(marker: Option<OsString>) -> std::io::Result<()> {
    if marker.as_deref() == Some(std::ffi::OsStr::new("1")) {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "profile login bootstrap not verified",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn finite() -> std::io::Result<PtyCommand> {
        let mut command = PtyCommand::new(std::env::current_exe()?);
        command.args.push("--session-probe-child".into());
        Ok(command)
    }
    #[test]
    fn marker_is_exact_and_is_not_a_permission_credential() {
        for marker in [
            None,
            Some("0".into()),
            Some("true".into()),
            Some("1 ".into()),
        ] {
            assert!(check_marker(marker).is_err());
        }
        assert!(check_marker(Some("1".into())).is_ok());
    }
    #[test]
    fn factory_rejects_any_non_fixed_program_arguments_or_environment() -> std::io::Result<()> {
        let mut command = finite()?;
        command.program = "arbitrary-user-shell".into();
        assert!(prepare(command).is_err());
        let mut command = finite()?;
        command.args.push("extra".into());
        assert!(prepare(command).is_err());
        let mut command = finite()?;
        command.cwd = Some(".".into());
        assert!(prepare(command).is_err());
        let mut command = finite()?;
        command.env.push(("USER_ARGUMENT".into(), "value".into()));
        assert!(prepare(command).is_err());
        let mut command = finite()?;
        command.env_remove.push("HOME".into());
        assert!(prepare(command).is_err());
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn finite_factory_resets_parent_marker_and_requests_known_bash_login() -> std::io::Result<()> {
        let original = finite()?;
        let executable = original.program.clone();
        let command = prepare(original).map_err(std::io::Error::other)?;
        assert_eq!(command.args[0], "--login");
        assert_eq!(command.args[1], "--noprofile");
        assert_eq!(command.args[2], "--norc");
        assert_eq!(command.args[6], executable);
        assert_eq!(command.args[7], "--session-probe-child");
        assert!(command.env.contains(&(MARKER.into(), "0".into())));
        assert!(
            command
                .env
                .contains(&("TERM_PROGRAM".into(), "PulseTerm".into()))
        );
        Ok(())
    }
}
