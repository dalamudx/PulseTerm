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
        const SCRIPT: &str = r#"builtin shopt -q login_shell || exit 93; builtin test "$TERM_PROGRAM" = PulseTerm || exit 94; export PULSE_PROFILE_LOGIN_CHECKED=1; builtin printf '\033]133;A\007\033]133;B\007\033]133;C\007'; exec "$@""#;
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

/// Validation only: launch one backend-owned finite Bash Profile after the real page
/// was authorized. No executable, arguments, cwd, or environment come from the page.
#[cfg(unix)]
pub(super) fn start_page(
    registry: &pulse_app::registry::SessionRegistry,
    page: &pulse_app::registry::PageLaunchScope,
) -> super::Result<pulse_app::local_launch::PendingLocalLaunch> {
    use pulse_app::local_launch::{ProfileSelection, start_page_profile_local};
    use pulse_core::TermSize;
    use pulse_pty::{
        launch::{LocalSessionEnvironment, LocalShellMode},
        profiles::{AutoProfileCatalog, AutoProfileLaunch, ShellKind, discover_unix_shells},
    };
    let bash = discover_unix_shells(std::ffi::OsStr::new("/bin:/usr/bin"), None)
        .map_err(|_| "session.page_profile_invalid".to_string())?
        .into_iter()
        .find(|candidate| candidate.kind() == ShellKind::Bash)
        .ok_or("session.page_profile_unavailable")?;
    let mut catalog = AutoProfileCatalog::default();
    catalog
        .reconcile(&[bash])
        .map_err(|_| "session.page_profile_invalid".to_string())?;
    const SCRIPT: &str = "builtin shopt -q login_shell || exit 93; builtin test \"$TERM_PROGRAM\" = PulseTerm || exit 94; builtin printf 'PAGE_LAUNCH_OK\\n'";
    let selection = ProfileSelection {
        kind: ShellKind::Bash,
        options: AutoProfileLaunch {
            args: [
                "--noprofile",
                "--norc",
                "-c",
                SCRIPT,
                "pulseterm-page-login",
            ]
            .into_iter()
            .map(OsString::from)
            .collect(),
            env_remove: ["BASH_ENV", "ENV", "SHELLOPTS", "BASHOPTS"]
                .map(OsString::from)
                .into(),
            mode: LocalShellMode::Login,
            ..AutoProfileLaunch::default()
        },
    };
    let context = LocalSessionEnvironment {
        application_version: env!("CARGO_PKG_VERSION").into(),
        pane: None,
    };
    start_page_profile_local(
        registry,
        page,
        &catalog,
        selection,
        &context,
        TermSize::new(32, 8).map_err(|_| "session.page_profile_invalid".to_string())?,
        16,
    )
    .map_err(|_| "session.page_launch_failed".to_string())
}

/// Call without holding the page scope lock: navigation must be able to revoke a
/// pending launch. The committed child and actor have their own supervisor.
#[cfg(unix)]
pub(super) async fn verify_page_result(
    registry: &pulse_app::registry::SessionRegistry,
    pending: pulse_app::local_launch::PendingLocalLaunch,
) -> super::Result<()> {
    use pulse_term::session::Lifecycle;
    let ready = pending
        .wait()
        .await
        .map_err(|_| "session.page_launch_failed".to_string())?;
    let owned = pulse_app::local_session_owner::LocalSessionOwner::adopt(
        tokio::runtime::Handle::current(),
        ready,
    );
    if !owned.is_managed_by(registry) {
        return Err("session.page_launch_failed".into());
    }
    let result = owned
        .finish()
        .await
        .map_err(|_| "session.page_launch_failed".to_string())?;
    if result.native.exit_code != 0
        || result.actor.state.lifecycle != Lifecycle::Ended
        || result.actor.state.parsed_bytes < b"PAGE_LAUNCH_OK\r\n".len() as u64
    {
        return Err("session.page_launch_failed".into());
    }
    eprintln!("session.native_page_launch.eof");
    Ok(())
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

    #[cfg(unix)]
    #[tokio::test]
    async fn fixed_page_profile_exits_and_retired_page_cannot_spawn_again()
    -> std::result::Result<(), String> {
        use pulse_app::registry::SessionRegistry;
        use tokio::time::{Duration, timeout};
        let registry = SessionRegistry::new();
        let owner = registry.open_window().map_err(super::super::error)?;
        let old = registry
            .activate_launch_page(owner)
            .map_err(super::super::error)?;
        let pending = start_page(&registry, &old)?;
        timeout(
            Duration::from_secs(5),
            verify_page_result(&registry, pending),
        )
        .await
        .map_err(super::super::error)??;
        assert_eq!(
            registry
                .retire_window_views(owner)
                .map_err(super::super::error)?,
            0
        );
        assert!(start_page(&registry, &old).is_err());
        let current = registry
            .activate_launch_page(owner)
            .map_err(super::super::error)?;
        drop(old);
        let pending = start_page(&registry, &current)?;
        timeout(
            Duration::from_secs(5),
            verify_page_result(&registry, pending),
        )
        .await
        .map_err(super::super::error)??;
        Ok(())
    }
}
