#![allow(clippy::unwrap_used)]
use super::*;
use crate::{MAX_COMMAND_ARGS, MAX_COMMAND_ENV};
fn context() -> LocalSessionEnvironment {
    LocalSessionEnvironment {
        application_version: "0.1.0-rc.1".into(),
        pane: None,
    }
}
#[test]
fn defaults_merge_with_literal_profile_values_and_no_cli_binding() {
    let mut original = PtyCommand::new("literal-executable");
    original.args = vec![" ; $(not-a-shell) 中文🌍 ".into(), "".into()];
    original.cwd = Some(std::env::temp_dir());
    original.env = vec![
        ("TERM".into(), "vt100".into()),
        ("COLORTERM".into(), "profile-color".into()),
        ("SECRET_KEY".into(), "secret-value".into()),
    ];
    let prepared = prepare_local_command(original.clone(), &context(), None).unwrap();
    assert_eq!(prepared.program, original.program);
    assert_eq!(prepared.args, original.args);
    assert_eq!(prepared.cwd, original.cwd);
    let c = prepared.build().unwrap();
    assert_eq!(c.get_env("TERM"), Some(OsStr::new("vt100")));
    assert_eq!(c.get_env("COLORTERM"), Some(OsStr::new("profile-color")));
    assert_eq!(c.get_env("TERM_PROGRAM"), Some(OsStr::new("PulseTerm")));
    assert_eq!(
        c.get_env("TERM_PROGRAM_VERSION"),
        Some(OsStr::new("0.1.0-rc.1"))
    );
    assert_eq!(c.get_env("PULSE_PANE_ID"), None);
    assert_eq!(c.get_env("PULSE_SOCKET"), None);
    assert!(!format!("{prepared:?}").contains("secret-value"));
}
#[test]
fn pane_pair_only_comes_from_backend_and_debug_is_redacted() {
    let ctx = LocalSessionEnvironment {
        application_version: "1.0.0".into(),
        pane: Some(PaneEnvironment {
            pane_id: "pane-backend-123".into(),
            socket: "private-socket-path".into(),
        }),
    };
    let c = prepare_local_command(PtyCommand::new("x"), &ctx, None)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        c.get_env("PULSE_PANE_ID"),
        Some(OsStr::new("pane-backend-123"))
    );
    assert_eq!(
        c.get_env("PULSE_SOCKET"),
        Some(OsStr::new("private-socket-path"))
    );
    assert!(!format!("{ctx:?}").contains("private-socket-path"));
    assert!(!format!("{:?}", ctx.pane).contains("pane-backend-123"));
}
#[test]
fn profile_cannot_override_or_remove_reserved_case_aliases() {
    for key in [
        "PULSE_PANE_ID",
        "pulse_pane_id",
        "PULSE_SOCKET",
        "PuLsE_SoCkEt",
        "TERM_PROGRAM",
        "term_program_version",
    ] {
        let mut c = PtyCommand::new("x");
        c.env.push((key.into(), "forged".into()));
        assert!(matches!(
            prepare_local_command(c, &context(), None),
            Err(LocalLaunchError::ReservedEnvironment)
        ));
        let mut c = PtyCommand::new("x");
        c.env_remove.push(key.into());
        assert!(matches!(
            prepare_local_command(c, &context(), None),
            Err(LocalLaunchError::ReservedEnvironment)
        ));
    }
}
#[test]
fn malformed_context_never_reaches_native_allocation_or_exposes_value() {
    for bad in ["", "secret\0value", "a\nb", "x y"] {
        let mut ctx = context();
        ctx.application_version = bad.into();
        let error = prepare_local_command(PtyCommand::new("x"), &ctx, None).unwrap_err();
        assert!(matches!(error, LocalLaunchError::InvalidContext));
        assert_eq!(error.to_string(), "local.invalid_environment_context");
    }
    let mut ctx = context();
    ctx.application_version = "x".repeat(65);
    assert!(prepare_local_command(PtyCommand::new("x"), &ctx, None).is_err());
    for (id, socket) in [
        ("", "x"),
        ("id with space", "x"),
        ("id", ""),
        ("id", "secret\0value"),
    ] {
        let mut ctx = context();
        ctx.pane = Some(PaneEnvironment {
            pane_id: id.into(),
            socket: socket.into(),
        });
        assert!(matches!(
            prepare_local_command(PtyCommand::new("x"), &ctx, None),
            Err(LocalLaunchError::InvalidContext)
        ));
    }
}
#[test]
fn pwsh_defaults_are_explicit_and_do_not_split_script_arguments() {
    let mut c = PtyCommand::new("custom-pwsh-path");
    c.args = vec!["-Command".into(), "literal ; $(argv)".into()];
    let prepared = prepare_local_command(c, &context(), Some(ShellKind::Pwsh)).unwrap();
    assert_eq!(
        prepared.args,
        vec![
            OsString::from("-NoLogo"),
            OsString::from("-Command"),
            OsString::from("literal ; $(argv)")
        ]
    );
    assert_eq!(
        prepared.build().unwrap().get_env("POWERSHELL_UPDATECHECK"),
        Some(OsStr::new("Off"))
    );
    let mut c = PtyCommand::new("pwsh");
    c.args.push("-nologo".into());
    c.env
        .push(("POWERSHELL_UPDATECHECK".into(), "profile-choice".into()));
    let prepared = prepare_local_command(c, &context(), Some(ShellKind::Pwsh)).unwrap();
    assert_eq!(prepared.args.len(), 1);
    assert_eq!(
        prepared.build().unwrap().get_env("POWERSHELL_UPDATECHECK"),
        Some(OsStr::new("profile-choice"))
    );
}
#[test]
fn internal_defaults_remain_inside_item_and_byte_budget() {
    let mut c = PtyCommand::new("x");
    c.args = vec!["".into(); MAX_COMMAND_ARGS];
    assert!(matches!(
        prepare_local_command(c, &context(), Some(ShellKind::Pwsh)),
        Err(LocalLaunchError::Command(PtyError::CommandLimit))
    ));
    let mut c = PtyCommand::new("x");
    c.env = (0..MAX_COMMAND_ENV)
        .map(|i| (format!("USER{i}").into(), "".into()))
        .collect();
    assert!(matches!(
        prepare_local_command(c, &context(), None),
        Err(LocalLaunchError::Command(PtyError::CommandLimit))
    ));
}
#[test]
fn inherited_removal_is_bounded_validated_and_precedes_new_override() {
    let mut c = PtyCommand::new("x");
    c.env_remove.push("PATH".into());
    assert!(c.build().unwrap().get_env("PATH").is_none());
    c.env.push(("PATH".into(), "explicit-value".into()));
    assert_eq!(
        c.build().unwrap().get_env("PATH"),
        Some(OsStr::new("explicit-value"))
    );
    for invalid in ["", "bad=key", "secret\0key"] {
        let mut c = PtyCommand::new("x");
        c.env_remove.push(invalid.into());
        assert!(matches!(c.validate(), Err(PtyError::InvalidCommand)));
        assert!(!format!("{c:?}").contains(invalid) || invalid.is_empty());
    }
    let mut c = PtyCommand::new("x");
    c.env_remove = vec!["X".into(); MAX_COMMAND_ENV + 1];
    assert!(matches!(c.validate(), Err(PtyError::CommandLimit)));
}
