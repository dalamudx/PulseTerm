use super::syntax::wildcard;
use super::*;
fn query(alias: &str) -> Result<ConfigQuery<'_>, ConfigError> {
    ConfigQuery::new(alias, "local", None, None)
}
#[test]
fn first_value_and_command_line_override_with_hostname_tokens() -> Result<(), ConfigError> {
    let bundle = ConfigBundle::parse(b"Host prod\n HostName %h.example\n User alice\n Port 2200\nHost *\n User fallback\n Port 22\n", &[])?;
    let result = bundle.resolve(&query("prod")?)?;
    assert_eq!(result.target().host(), "prod.example");
    assert_eq!(result.target().port(), 2200);
    assert_eq!(result.user(), "alice");
    let result = bundle.resolve(&ConfigQuery::new("prod", "local", Some("cli"), Some(2222))?)?;
    assert_eq!(result.target().port(), 2222);
    assert_eq!(result.user(), "cli");
    let defaults = ConfigBundle::parse(b"", &[])?.resolve(&query("::1")?)?;
    assert_eq!(defaults.target().host(), "::1");
    assert_eq!(defaults.user(), "local");
    Ok(())
}
#[test]
fn host_negation_case_and_question_patterns() -> Result<(), ConfigError> {
    let bundle = ConfigBundle::parse(
        b"Host p?od !prod\n Port 2000\nHost P*\n User alice\nHost *\n Port 22\n",
        &[],
    )?;
    assert_eq!(bundle.resolve(&query("prod")?)?.target().port(), 22);
    assert_eq!(bundle.resolve(&query("pRod")?)?.user(), "local");
    assert_eq!(bundle.resolve(&query("plod")?)?.target().port(), 2000);
    assert_eq!(bundle.resolve(&query("other")?)?.user(), "local");
    let all_negative = ConfigBundle::parse(b"Host !no\n User denied\n", &[])?;
    assert_eq!(all_negative.resolve(&query("yes")?)?.user(), "local");
    Ok(())
}
#[test]
fn include_sort_first_value_and_textual_context() -> Result<(), ConfigError> {
    let sources = [
        ProvidedConfigSource::new("parts/z.conf", b"User last\n"),
        ProvidedConfigSource::new("parts/a.conf", b"User first\nHost other\nPort 99\n"),
    ];
    let bundle = ConfigBundle::parse(
        b"Host prod\nInclude parts/*.conf\nPort 2300\nHost *\nPort 22\n",
        &sources,
    )?;
    let result = bundle.resolve(&query("prod")?)?;
    assert_eq!(result.user(), "first");
    assert_eq!(result.target().port(), 2300);
    let nested = [
        ProvidedConfigSource::new("a", b"Include b\n"),
        ProvidedConfigSource::new("b", b"HostName real.example\n"),
    ];
    assert_eq!(
        ConfigBundle::parse(b"Include a\n", &nested)?
            .resolve(&query("prod")?)?
            .target()
            .host(),
        "real.example"
    );
    Ok(())
}
#[test]
fn include_missing_cycle_inactive_and_depth() -> Result<(), ConfigError> {
    assert_eq!(
        ConfigBundle::parse(b"Include missing\n", &[])?
            .resolve(&query("prod")?)
            .err(),
        Some(ConfigError::MissingInclude)
    );
    let cycle = [
        ProvidedConfigSource::new("a", b"Include b\n"),
        ProvidedConfigSource::new("b", b"Include a\n"),
    ];
    assert_eq!(
        ConfigBundle::parse(b"Include a\n", &cycle)?
            .resolve(&query("prod")?)
            .err(),
        Some(ConfigError::IncludeCycle)
    );
    assert!(
        ConfigBundle::parse(b"Host other\n Include missing\n", &[])?
            .resolve(&query("prod")?)
            .is_ok()
    );
    let names: Vec<_> = (0..10).map(|n| format!("f{n}")).collect();
    let texts: Vec<_> = (0..10).map(|n| format!("Include f{}\n", n + 1)).collect();
    let sources: Vec<_> = names
        .iter()
        .zip(&texts)
        .map(|(name, text)| ProvidedConfigSource::new(name, text.as_bytes()))
        .collect();
    assert_eq!(
        ConfigBundle::parse(b"Include f0\n", &sources)?
            .resolve(&query("prod")?)
            .err(),
        Some(ConfigError::Limit)
    );
    Ok(())
}
#[test]
fn repeated_include_has_bounded_total_work_without_cycles() -> Result<(), ConfigError> {
    let sources = [
        ProvidedConfigSource::new("a", b"Include b b b b b b b b\n"),
        ProvidedConfigSource::new("b", b"Include c c c c c c c c\n"),
        ProvidedConfigSource::new("c", b"Include d d d d d d d d\n"),
        ProvidedConfigSource::new("d", b"Include e e e e e e e e\n"),
        ProvidedConfigSource::new("e", b"User alice\n"),
    ];
    assert_eq!(
        ConfigBundle::parse(b"Include a\n", &sources)?
            .resolve(&query("prod")?)
            .err(),
        Some(ConfigError::Limit)
    );
    Ok(())
}
#[test]
fn match_uses_effective_host_original_host_user_and_local_user() -> Result<(), ConfigError> {
    let bundle = ConfigBundle::parse(b"Host prod\n HostName REAL.example\n User Alice\nMatch host real.* originalhost prod localuser local user Alice,!root\n Port 2222\nMatch all\n IdentitiesOnly yes\n", &[])?;
    let result = bundle.resolve(&query("prod")?)?;
    assert_eq!(result.target().port(), 2222);
    assert_eq!(result.identities_only(), Some(true));
    assert_eq!(
        bundle
            .resolve(&ConfigQuery::new("prod", "local", Some("alice"), None)?)?
            .target()
            .port(),
        22
    );
    let unsupported = ConfigBundle::parse(
        b"Host other\nMatch exec \"touch /data/temp/not-executed\"\nUser root\n",
        &[],
    )?;
    assert_eq!(
        unsupported.resolve(&query("prod")?).err(),
        Some(ConfigError::Unsupported)
    );
    Ok(())
}
#[test]
fn whitespace_equals_quotes_comments_and_literal_identity_hints() -> Result<(), ConfigError> {
    let bundle = ConfigBundle::parse(b"  hOsT = prod # comment\r\n HoStNaMe=REAL.example\n IdentityFile \"~/.ssh/key with spaces\"\nIdentityFile \"~/.ssh/key with spaces\"\nIdentityFile none\nIdentitiesOnly = NO\n", &[])?;
    let result = bundle.resolve(&query("prod")?)?;
    assert_eq!(result.target().host(), "real.example");
    assert_eq!(result.identity_files(), &["~/.ssh/key with spaces", "none"]);
    assert_eq!(result.identities_only(), Some(false));
    Ok(())
}
#[test]
fn active_unsupported_directives_fail_closed_inactive_options_do_not() -> Result<(), ConfigError> {
    for key in [
        "ProxyCommand echo x",
        "ProxyJump jump",
        "LocalCommand echo x",
        "RemoteCommand echo x",
        "ForwardAgent yes",
        "UserKnownHostsFile /dev/null",
        "StrictHostKeyChecking no",
        "UnknownOption anything",
    ] {
        assert_eq!(
            ConfigBundle::parse(key.as_bytes(), &[])?
                .resolve(&query("prod")?)
                .err(),
            Some(ConfigError::Unsupported)
        );
        let inactive = format!("Host other\n{key}\nHost prod\nUser ok\n");
        assert_eq!(
            ConfigBundle::parse(inactive.as_bytes(), &[])?
                .resolve(&query("prod")?)?
                .user(),
            "ok"
        );
    }
    Ok(())
}
#[test]
fn malformed_input_and_resolved_target_are_not_approximated() -> Result<(), ConfigError> {
    for text in [
        b"User \"unclosed".as_slice(),
        b"Port 0",
        b"Port 65536",
        b"Port -22",
        b"User two values",
        b"HostName",
        b"\xff",
        b"User a\0b",
        b"IdentitiesOnly maybe",
        b"HostName real\rexample",
    ] {
        assert!(ConfigBundle::parse(text, &[]).is_err());
    }
    for text in [
        b"HostName user@host".as_slice(),
        b"User /root",
        b"HostName host:22",
    ] {
        assert_eq!(
            ConfigBundle::parse(text, &[])?
                .resolve(&query("prod")?)
                .err(),
            Some(ConfigError::InvalidTarget)
        );
    }
    assert_eq!(
        ConfigBundle::parse(b"HostName %x.example", &[])?
            .resolve(&query("prod")?)
            .err(),
        Some(ConfigError::Unsupported)
    );
    assert!(ConfigQuery::new("bad*alias", "local", None, None).is_err());
    Ok(())
}
#[test]
fn explicit_source_names_duplicates_and_aggregate_bounds() -> Result<(), ConfigError> {
    for name in ["", "a*", "a?", "a\n", "a%h"] {
        assert_eq!(
            ConfigBundle::parse(b"", &[ProvidedConfigSource::new(name, b"")]).err(),
            Some(ConfigError::Syntax)
        );
    }
    assert_eq!(
        ConfigBundle::parse(
            b"",
            &[
                ProvidedConfigSource::new("a", b""),
                ProvidedConfigSource::new("a", b"")
            ]
        )
        .err(),
        Some(ConfigError::Syntax)
    );
    assert_eq!(
        ConfigBundle::parse(&vec![b'#'; MAX_SOURCE + 1], &[]).err(),
        Some(ConfigError::Limit)
    );
    assert_eq!(
        ConfigBundle::parse(&b"\n".repeat(MAX_LINES + 1), &[]).err(),
        Some(ConfigError::Limit)
    );
    assert_eq!(
        ConfigBundle::parse(&vec![b'#'; 8193], &[]).err(),
        Some(ConfigError::Limit)
    );
    let huge = b"# padded line\n".repeat(18000);
    let sources = [
        ProvidedConfigSource::new("a", &huge),
        ProvidedConfigSource::new("b", &huge),
    ];
    assert!(ConfigBundle::parse(&huge, &sources).is_err());
    Ok(())
}
#[test]
fn glob_work_budget_is_shared_and_matching_is_non_recursive() -> Result<(), ConfigError> {
    let mut budget = 100;
    assert!(wildcard("*a?b*", "prefixaxb-tail", false, &mut budget)?);
    assert!(!wildcard("a?b", "abb-tail", false, &mut budget)?);
    let mut budget = 0;
    assert_eq!(
        wildcard("*", "a", false, &mut budget),
        Err(ConfigError::Limit)
    );
    Ok(())
}
#[test]
fn errors_never_echo_source_strings() -> Result<(), ConfigError> {
    for error in [
        ConfigError::Limit,
        ConfigError::Syntax,
        ConfigError::Unsupported,
        ConfigError::MissingInclude,
        ConfigError::IncludeCycle,
        ConfigError::InvalidTarget,
    ] {
        assert!(error.to_string().starts_with("ssh.config."));
        assert!(!format!("{error:?}").contains("alice"));
    }
    Ok(())
}

#[test]
fn include_globs_do_not_cross_directories_or_select_hidden_files() -> Result<(), ConfigError> {
    let sources = [
        ProvidedConfigSource::new("parts/a.conf", b"User visible\n"),
        ProvidedConfigSource::new("parts/.hidden.conf", b"ProxyCommand forbidden\n"),
        ProvidedConfigSource::new("parts/sub/z.conf", b"ProxyCommand forbidden\n"),
    ];
    let result =
        ConfigBundle::parse(b"Include parts/*.conf\n", &sources)?.resolve(&query("prod")?)?;
    assert_eq!(result.user(), "visible");
    assert_eq!(
        ConfigBundle::parse(b"Include parts/.*.conf\n", &sources)?
            .resolve(&query("prod")?)
            .err(),
        Some(ConfigError::Unsupported)
    );
    Ok(())
}
#[test]
fn single_quotes_and_midword_hash_preserve_literal_identity_hint() -> Result<(), ConfigError> {
    let result = ConfigBundle::parse(
        b"IdentityFile 'key with spaces'\nIdentityFile key#suffix # comment\n",
        &[],
    )?
    .resolve(&query("prod")?)?;
    assert_eq!(result.identity_files(), &["key with spaces", "key#suffix"]);
    Ok(())
}
