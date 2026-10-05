#![allow(clippy::unwrap_used)]
use super::*;
#[cfg(unix)]
use crate::MAX_COMMAND_ARGS;
fn context() -> LocalSessionEnvironment {
    LocalSessionEnvironment {
        application_version: "1.0.0".into(),
        pane: None,
    }
}
#[cfg(unix)]
#[test]
fn known_login_shells_preserve_program_cwd_and_literal_arguments() {
    for shell in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Fish] {
        let mut command = PtyCommand::new("/literal/selected-program");
        command.args = vec![
            "-c".into(),
            "literal ; $(not-code) 中文🌍".into(),
            "".into(),
            "--login".into(),
        ];
        command.cwd = Some(std::env::temp_dir());
        let prepared =
            prepare_local_login_command(command.clone(), &context(), Some(shell)).unwrap();
        assert_eq!(prepared.program, command.program);
        assert_eq!(prepared.cwd, command.cwd);
        assert_eq!(
            prepared.args[0],
            if shell == ShellKind::Bash {
                "--login"
            } else {
                "-l"
            }
        );
        assert_eq!(&prepared.args[1..], &command.args);
        assert_eq!(prepared.build().unwrap().get_env("PULSE_PANE_ID"), None);
    }
}
#[cfg(unix)]
#[test]
fn only_a_leading_login_option_is_reused() {
    for flag in ["-l", "--login"] {
        let mut command = PtyCommand::new("bash");
        command.args = vec![flag.into(), "-c".into(), "literal".into()];
        let prepared =
            prepare_local_login_command(command.clone(), &context(), Some(ShellKind::Bash))
                .unwrap();
        assert_eq!(prepared.args, command.args);
    }
}
#[test]
fn unsupported_login_does_not_fall_back_or_reveal_program() {
    for shell in [None, Some(ShellKind::Sh), Some(ShellKind::Pwsh)] {
        let error =
            prepare_local_login_command(PtyCommand::new("secret-program"), &context(), shell)
                .unwrap_err();
        assert!(matches!(error, LocalLaunchError::UnsupportedLoginShell));
        assert_eq!(error.to_string(), "local.unsupported_login_shell");
    }
    #[cfg(not(unix))]
    for shell in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Fish] {
        assert!(matches!(
            prepare_local_login_command(PtyCommand::new("x"), &context(), Some(shell)),
            Err(LocalLaunchError::UnsupportedLoginShell)
        ));
    }
}
#[cfg(unix)]
#[test]
fn login_insertion_keeps_final_budget_and_environment_authority() {
    let mut command = PtyCommand::new("bash");
    command.args = vec!["".into(); MAX_COMMAND_ARGS];
    assert!(matches!(
        prepare_local_login_command(command, &context(), Some(ShellKind::Bash)),
        Err(LocalLaunchError::Command(PtyError::CommandLimit))
    ));
    let mut command = PtyCommand::new("bash");
    command.env.push(("pulse_socket".into(), "forged".into()));
    assert!(matches!(
        prepare_local_login_command(command, &context(), Some(ShellKind::Bash)),
        Err(LocalLaunchError::ReservedEnvironment)
    ));
}
#[cfg(unix)]
#[test]
fn non_utf8_login_arguments_remain_literal() {
    use std::os::unix::ffi::OsStringExt;
    let mut command = PtyCommand::new("bash");
    command.args = vec![OsString::from_vec(vec![0xff, 0xfe])];
    let prepared =
        prepare_local_login_command(command.clone(), &context(), Some(ShellKind::Bash)).unwrap();
    assert_eq!(&prepared.args[1..], &command.args);
}
#[test]
fn profile_default_is_platform_specific_and_never_infers_unknown_login() {
    for shell in [
        None,
        Some(ShellKind::Sh),
        Some(ShellKind::Pwsh),
        Some(ShellKind::Bash),
        Some(ShellKind::Zsh),
        Some(ShellKind::Fish),
    ] {
        let expected = cfg!(target_os = "macos")
            && matches!(
                shell,
                Some(ShellKind::Bash | ShellKind::Zsh | ShellKind::Fish)
            );
        assert_eq!(LocalShellMode::default().requests_login(shell), expected);
        assert!(!LocalShellMode::Plain.requests_login(shell));
        assert!(LocalShellMode::Login.requests_login(shell));
    }
}
#[test]
fn profile_plain_and_unknown_default_preserve_explicit_user_flags() {
    for mode in [LocalShellMode::Plain, LocalShellMode::PlatformDefault] {
        let mut command = PtyCommand::new("literal-unknown-program");
        command.args = vec!["-l".into(), "-c".into(), "literal script".into()];
        let prepared =
            prepare_local_profile_command(command.clone(), &context(), None, mode).unwrap();
        assert_eq!(prepared.program, command.program);
        assert_eq!(prepared.args, command.args);
    }
    assert!(matches!(
        prepare_local_profile_command(
            PtyCommand::new("sh"),
            &context(),
            Some(ShellKind::Sh),
            LocalShellMode::Login
        ),
        Err(LocalLaunchError::UnsupportedLoginShell)
    ));
}
#[cfg(unix)]
#[test]
fn every_profile_mode_shares_final_budget_and_identity_authority() {
    for mode in [
        LocalShellMode::Plain,
        LocalShellMode::Login,
        LocalShellMode::PlatformDefault,
    ] {
        let mut command = PtyCommand::new("bash");
        command.env_remove.push("pulse_pane_id".into());
        assert!(matches!(
            prepare_local_profile_command(command, &context(), Some(ShellKind::Bash), mode),
            Err(LocalLaunchError::ReservedEnvironment)
        ));
        let command = PtyCommand::new("bash");
        let prepared =
            prepare_local_profile_command(command.clone(), &context(), Some(ShellKind::Bash), mode)
                .unwrap();
        assert_eq!(prepared.program, command.program);
        assert_eq!(
            prepared.args.len(),
            usize::from(mode.requests_login(Some(ShellKind::Bash)))
        );
    }
}

#[test]
fn invalid_command_is_rejected_before_login_classification() {
    let mut command = PtyCommand::new("x");
    command.args.push("invalid\0argument".into());
    assert!(matches!(
        prepare_local_login_command(command, &context(), None),
        Err(LocalLaunchError::Command(PtyError::InvalidCommand))
    ));
}
