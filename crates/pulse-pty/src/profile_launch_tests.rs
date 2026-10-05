use super::*;
use crate::profiles::ShellCandidate;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn context() -> LocalSessionEnvironment {
    LocalSessionEnvironment {
        application_version: "test-version".into(),
        pane: None,
    }
}
fn catalog(kind: ShellKind) -> Result<AutoProfileCatalog, ProfileError> {
    let mut catalog = AutoProfileCatalog::default();
    catalog.reconcile(&[ShellCandidate::new(
        kind,
        std::env::temp_dir().join("literal ; $(secret) 中文"),
    )?])?;
    Ok(catalog)
}
#[test]
fn literal_options_and_runtime_identity_share_one_catalog_path() -> TestResult {
    let catalog = catalog(ShellKind::Bash)?;
    let cwd = std::env::temp_dir().join("literal cwd 中文");
    let args: Vec<OsString> = ["", "--noprofile", "$(not-executed)", "中文"]
        .map(Into::into)
        .into();
    let command = catalog.prepare_launch(
        ShellKind::Bash,
        AutoProfileLaunch {
            args: args.clone(),
            cwd: Some(cwd.clone()),
            env: vec![
                ("TERM".into(), "literal-terminal".into()),
                ("CUSTOM".into(), "secret".into()),
            ],
            env_remove: vec!["OLD_VALUE".into()],
            mode: LocalShellMode::Plain,
        },
        &context(),
    )?;
    assert_eq!(
        command.program,
        std::env::temp_dir().join("literal ; $(secret) 中文")
    );
    assert_eq!(command.args, args);
    assert_eq!(command.cwd, Some(cwd));
    assert!(
        command
            .env
            .contains(&("TERM_PROGRAM".into(), "PulseTerm".into()))
    );
    assert_eq!(
        command
            .env
            .iter()
            .rfind(|(k, _)| k == "TERM")
            .map(|(_, v)| v.as_os_str()),
        Some(std::ffi::OsStr::new("literal-terminal"))
    );
    assert!(command.env_remove.contains(&"PULSE_PANE_ID".into()));
    assert!(command.env_remove.contains(&"PULSE_SOCKET".into()));
    assert!(command.env_remove.contains(&"OLD_VALUE".into()));
    Ok(())
}
#[test]
fn hidden_explicit_launch_works_but_unavailable_deleted_and_absent_fail_closed() -> TestResult {
    let mut catalog = catalog(ShellKind::Bash)?;
    catalog.set_hidden(ShellKind::Bash, true)?;
    let prepare = |catalog: &AutoProfileCatalog, kind| {
        catalog.prepare_launch(kind, AutoProfileLaunch::default(), &context())
    };
    assert!(prepare(&catalog, ShellKind::Bash).is_ok());
    assert!(matches!(
        prepare(&catalog, ShellKind::Zsh),
        Err(ProfileLaunchError::Profile(ProfileError::NotFound))
    ));
    catalog.reconcile(&[])?;
    assert!(matches!(
        prepare(&catalog, ShellKind::Bash),
        Err(ProfileLaunchError::Profile(ProfileError::Unavailable))
    ));
    catalog.remove(ShellKind::Bash)?;
    catalog.reconcile(&[ShellCandidate::new(
        ShellKind::Bash,
        std::env::temp_dir().join("reinstalled"),
    )?])?;
    assert!(matches!(
        prepare(&catalog, ShellKind::Bash),
        Err(ProfileLaunchError::Profile(ProfileError::NotFound))
    ));
    Ok(())
}
#[test]
fn program_and_login_classification_always_come_from_current_metadata() -> TestResult {
    let mut catalog = catalog(ShellKind::Bash)?;
    let moved = std::env::temp_dir().join("new-bash 中文");
    catalog.reconcile(&[ShellCandidate::new(ShellKind::Bash, moved.clone())?])?;
    let command = catalog.prepare_launch(
        ShellKind::Bash,
        AutoProfileLaunch {
            mode: LocalShellMode::Login,
            ..Default::default()
        },
        &context(),
    );
    if cfg!(unix) {
        let command = command?;
        assert_eq!(command.program, moved);
        assert_eq!(command.args, [OsString::from("--login")]);
    } else {
        assert!(matches!(
            command,
            Err(ProfileLaunchError::Launch(
                LocalLaunchError::UnsupportedLoginShell
            ))
        ));
    }
    let sh = self::catalog(ShellKind::Sh)?;
    assert!(matches!(
        sh.prepare_launch(
            ShellKind::Sh,
            AutoProfileLaunch {
                mode: LocalShellMode::Login,
                ..Default::default()
            },
            &context()
        ),
        Err(ProfileLaunchError::Launch(
            LocalLaunchError::UnsupportedLoginShell
        ))
    ));
    Ok(())
}
#[test]
fn reserved_identity_invalid_metadata_and_command_budgets_remain_enforced() -> TestResult {
    let catalog = catalog(ShellKind::Bash)?;
    for options in [
        AutoProfileLaunch {
            env: vec![("pulse_pane_id".into(), "secret".into())],
            ..Default::default()
        },
        AutoProfileLaunch {
            env_remove: vec!["TERM_PROGRAM".into()],
            ..Default::default()
        },
        AutoProfileLaunch {
            args: vec!["x".into(); 129],
            ..Default::default()
        },
        AutoProfileLaunch {
            args: vec!["secret\0value".into()],
            ..Default::default()
        },
        AutoProfileLaunch {
            cwd: Some(PathBuf::new()),
            ..Default::default()
        },
    ] {
        assert!(
            catalog
                .prepare_launch(ShellKind::Bash, options, &context())
                .is_err()
        );
    }
    assert!(
        catalog
            .prepare_launch(
                ShellKind::Bash,
                AutoProfileLaunch::default(),
                &LocalSessionEnvironment {
                    application_version: "".into(),
                    pane: None
                }
            )
            .is_err()
    );
    Ok(())
}
#[test]
fn options_debug_and_errors_do_not_expose_literal_values() -> TestResult {
    let options = AutoProfileLaunch {
        args: vec!["secret-argument".into()],
        cwd: Some("secret-cwd".into()),
        env: vec![("secret-key".into(), "secret-value".into())],
        env_remove: vec!["secret-removed".into()],
        ..Default::default()
    };
    assert!(!format!("{options:?}").contains("secret"));
    let error = AutoProfileCatalog::default()
        .prepare_launch(ShellKind::Bash, options, &context())
        .err()
        .ok_or("missing rejection")?;
    assert_eq!(error.to_string(), "profile.not_found");
    Ok(())
}
