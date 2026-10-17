use super::*;
const ID: &str = "a7d68e84-72b2-435c-8fa8-4c3525ff803e";
const HOST: &str = "018f5691-0059-7a69-a45a-7c4d6a1cfa51";
const IDENTITY: &str = "018f5691-0059-7a69-a45a-7c4d6a1cfa52";
fn body(method: &str, params: Value) -> String {
    json!({"jsonrpc":"2.0","id":ID,"method":method,"params":params}).to_string()
}
#[test]
fn saved_alias_closed_roundtrip_rejects_mixed_ids_unbounded_or_invisible_names() -> Result<(), Error>
{
    let name = "生产入口 Alpha";
    let action = StartupAction::parse(&body("workspace.ssh", json!({"alias":name})))?;
    assert_eq!(
        StartupAction::parse(&action.encode()?)?
            .params()
            .get("alias"),
        Some(&json!(name))
    );
    assert!(action.supports_cold_transport());
    assert!(!action.supports_cold_readonly_transport());
    for params in [
        json!({"alias":"x","host":HOST,"identity":IDENTITY}),
        json!({"alias":"x","program":"/bin/sh"}),
        json!({"alias":""}),
        json!({"alias":" x"}),
        json!({"alias":"x\n"}),
        json!({"alias":"x\u{200b}"}),
        json!({"alias":"x".repeat(257)}),
        json!({"alias":"字".repeat(86)}),
    ] {
        assert!(StartupAction::parse(&body("workspace.ssh", params)).is_err());
    }
    Ok(())
}
#[test]
fn closed_actions_roundtrip_without_changing_correlation() -> Result<(), Error> {
    for (method, params) in [
        ("workspace.focus", json!({})),
        ("workspace.profiles", json!({})),
        ("workspace.new-tab", json!({"profile":"sh"})),
        (
            "workspace.split",
            json!({"profile":"bash","direction":"row"}),
        ),
        (
            "workspace.split",
            json!({"profile":"fish","direction":"column"}),
        ),
        ("workspace.ssh", json!({"host":HOST,"identity":IDENTITY})),
    ] {
        let action = StartupAction::parse(&body(method, params.clone()))?;
        assert_eq!(action.request_id(), ID);
        let encoded = action.encode()?;
        let restored = StartupAction::parse(&encoded)?;
        assert_eq!(restored.method(), method);
        assert_eq!(restored.params(), params.as_object().ok_or(Error)?);
        assert_eq!(restored.request_id(), ID);
    }
    Ok(())
}
#[test]
fn builder_has_distinct_ids_and_remains_inert() -> Result<(), Error> {
    let a = StartupAction::new("workspace.focus", &json!({}))?;
    let b = StartupAction::new("workspace.focus", &json!({}))?;
    assert_ne!(a.request_id(), b.request_id());
    assert!(StartupAction::new("execute", &json!({})).is_err());
    assert!(StartupAction::new("workspace.focus", &json!({"confirm":true})).is_err());
    Ok(())
}
#[test]
fn rejects_methods_secrets_paths_and_extra_metadata() {
    for method in [
        "hello",
        "workspace.open",
        "execute",
        "workspace.ssh-password",
        "focus",
    ] {
        assert!(StartupAction::parse(&body(method, json!({}))).is_err());
    }
    for key in [
        "program", "args", "env", "cwd", "password", "token", "nonce", "confirm",
    ] {
        assert!(
            StartupAction::parse(&body("workspace.new-tab", json!({"profile":"sh",key:"x"})))
                .is_err()
        );
    }
    for params in [
        json!({}),
        json!({"profile":"/bin/sh"}),
        json!({"profile":"SH"}),
        json!({"profile":null}),
        json!({"profile":["sh"]}),
    ] {
        assert!(StartupAction::parse(&body("workspace.new-tab", params)).is_err());
    }
    for direction in ["vertical", "left", "", "row\n"] {
        assert!(
            StartupAction::parse(&body(
                "workspace.split",
                json!({"profile":"sh","direction":direction})
            ))
            .is_err()
        );
    }
}
#[test]
fn rejects_ambiguous_json_before_schema_checks() {
    for params in [
        r#"{"profile":"sh","profile":"sh"}"#,
        r#"{"profile":"sh","prof\u0069le":"sh"}"#,
        r#"{"profile":{"x":1,"x":1}}"#,
    ] {
        let text = format!(
            r#"{{"jsonrpc":"2.0","id":"{ID}","method":"workspace.new-tab","params":{params}}}"#
        );
        assert!(StartupAction::parse(&text).is_err());
    }
    let duplicate = format!(
        r#"{{"jsonrpc":"2.0","id":"{ID}","id":"{ID}","method":"workspace.focus","params":{{}}}}"#
    );
    assert!(StartupAction::parse(&duplicate).is_err());
}
#[test]
fn rejects_noncanonical_wrong_version_ids_and_uuid_shortcuts() {
    for id in [
        HOST.to_owned(),
        ID.to_uppercase(),
        ID.replace('-', ""),
        format!("{{{ID}}}"),
        "action".into(),
    ] {
        let text =
            json!({"jsonrpc":"2.0","id":id,"method":"workspace.focus","params":{}}).to_string();
        assert!(StartupAction::parse(&text).is_err());
    }
    for host in [
        ID.to_owned(),
        HOST.to_uppercase(),
        HOST.replace('-', ""),
        "user@host".into(),
    ] {
        assert!(
            StartupAction::parse(&body(
                "workspace.ssh",
                json!({"host":host,"identity":IDENTITY})
            ))
            .is_err()
        );
    }
}
#[test]
fn rejects_batches_notifications_trailing_data_and_oversized_input() {
    let valid = body("workspace.focus", json!({}));
    for text in [
        String::new(),
        format!("[{valid}]"),
        format!("{valid} {{}}"),
        " ".repeat(MAX_STARTUP_BYTES + 1),
        json!({"jsonrpc":"2.0","method":"workspace.focus","params":{}}).to_string(),
        json!({"jsonrpc":"2.0","id":ID,"method":"workspace.focus","params":{},"approved":true})
            .to_string(),
    ] {
        assert!(StartupAction::parse(&text).is_err());
    }
}
#[test]
fn argument_shape_is_exact_and_unknown_flags_never_become_actions() -> Result<(), Error> {
    assert!(from_args(Vec::<OsString>::new())?.is_none());
    let text = body("workspace.focus", json!({}));
    assert_eq!(
        from_args(["--startup-actions".into(), text.clone().into()])?
            .ok_or(Error)?
            .method(),
        "workspace.focus"
    );
    for args in [
        vec!["--startup-actions".into()],
        vec![
            "--startup-actions".into(),
            text.clone().into(),
            "extra".into(),
        ],
        vec!["--program".into(), text.clone().into()],
        vec![format!("--startup-actions={text}").into()],
    ] {
        assert!(from_args(args).is_err());
    }
    Ok(())
}
#[cfg(unix)]
#[test]
fn non_unicode_arguments_fail_closed() {
    use std::os::unix::ffi::OsStringExt;
    assert!(from_args(["--startup-actions".into(), OsString::from_vec(vec![0xff])]).is_err());
}

#[test]
fn cold_transport_is_readonly_not_general_startup_execution() -> Result<(), Error> {
    for method in ["workspace.focus", "workspace.profiles"] {
        assert!(StartupAction::new(method, &json!({}))?.supports_cold_readonly_transport());
        assert!(StartupAction::new(method, &json!({"program":"/bin/sh"})).is_err());
    }
    for (method, params) in [
        ("workspace.new-tab", json!({"profile":"sh"})),
        ("workspace.split", json!({"profile":"sh","direction":"row"})),
        (
            "workspace.split",
            json!({"profile":"sh","direction":"column"}),
        ),
        ("workspace.ssh", json!({"host":HOST,"identity":IDENTITY})),
    ] {
        let action = StartupAction::new(method, &params)?;
        assert!(!action.supports_cold_readonly_transport());
        assert!(!StartupAction::parse(&action.encode()?)?.supports_cold_readonly_transport());
    }
    Ok(())
}

#[test]
fn cold_new_tab_transport_does_not_upgrade_readonly_or_secondary_forwarding() -> Result<(), Error> {
    for method in ["workspace.focus", "workspace.profiles"] {
        assert!(StartupAction::new(method, &json!({}))?.supports_cold_transport());
    }
    let new_tab = StartupAction::new("workspace.new-tab", &json!({"profile":"sh"}))?;
    assert!(new_tab.supports_cold_transport());
    assert!(!new_tab.supports_cold_readonly_transport());
    assert!(StartupAction::parse(&new_tab.encode()?)?.supports_cold_transport());
    let ssh = StartupAction::new("workspace.ssh", &json!({"host":HOST,"identity":IDENTITY}))?;
    assert!(ssh.supports_cold_transport());
    assert!(!ssh.supports_cold_readonly_transport());
    let split = StartupAction::new(
        "workspace.split",
        &json!({"profile":"sh","direction":"row"}),
    )?;
    assert!(split.supports_cold_transport());
    assert!(!split.supports_cold_readonly_transport());
    Ok(())
}

#[test]
fn cold_selector_only_normalizes_primary_and_rejects_recreating_named_windows() -> Result<(), Error>
{
    let action = StartupAction::parse(&body(
        "workspace.new-tab",
        json!({"profile":"sh","window":"workspace"}),
    ))?;
    assert_eq!(
        action.params(),
        &json!({"profile":"sh"}).as_object().ok_or(Error)?.clone()
    );
    for label in [
        json!(format!("workspace-{}", uuid::Uuid::new_v4())),
        json!(null),
        json!("other"),
    ] {
        assert!(
            StartupAction::parse(&body(
                "workspace.new-tab",
                json!({"profile":"sh","window":label})
            ))
            .is_err()
        );
    }
    assert!(StartupAction::parse(&body("workspace.windows", json!({}))).is_err());
    Ok(())
}
