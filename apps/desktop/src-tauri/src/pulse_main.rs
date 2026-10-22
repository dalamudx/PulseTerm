//! Bounded Linux/Windows CLI; cold actions retain original-page admission and explicit GUI approval.
use std::{ffi::OsStr, process::ExitCode};
mod cli_profiles;
#[cfg(any(target_os = "linux", windows, test))]
mod cli_windows;
#[cfg(target_os = "linux")]
mod pulse_cold;
#[cfg(windows)]
#[path = "pulse_warm_windows.rs"]
mod pulse_cold;

mod cli_information;
#[cfg(test)]
mod pulse_agent_interactive_tests;
mod pulse_chain;
#[cfg(test)]
mod pulse_password_interactive_tests;
fn method(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<&'static str> {
    let mut args = args.into_iter();
    let action = args.next()?;
    if args.next().is_some() {
        return None;
    }
    match action.as_os_str() {
        value if value == OsStr::new("focus") => Some("workspace.focus"),
        value if value == OsStr::new("profiles") => Some("workspace.profiles"),
        value if value == OsStr::new("windows") => Some("workspace.windows"),
        value if value == OsStr::new("new-window") => Some("workspace.new-window"),
        _ => None,
    }
}

#[cfg(test)]
#[test]
fn new_window_is_closed_warm_action_with_exact_secondary_receipt() {
    let args = ["-w", "workspace", "new-window"]
        .into_iter()
        .map(std::ffi::OsString::from);
    assert_eq!(
        request(args),
        Some((
            "workspace.new-window",
            serde_json::json!({"window":"workspace"})
        ))
    );
    assert!(
        request(
            ["new-window", "sh"]
                .into_iter()
                .map(std::ffi::OsString::from)
        )
        .is_none()
    );
    assert!(
        pulseterm_desktop::cli_startup::StartupAction::new(
            "workspace.new-window",
            &serde_json::json!({})
        )
        .is_err()
    );
    let params = serde_json::json!({});
    let label = format!("workspace-{}", uuid::Uuid::new_v4());
    assert_eq!(
        receipt(
            "workspace.new-window",
            &params,
            serde_json::json!({"window":label})
        ),
        Some(label.clone())
    );
    for result in [
        serde_json::json!({"window":"workspace"}),
        serde_json::json!({"window":"other"}),
        serde_json::json!({"window":label,"session":"1"}),
    ] {
        assert!(receipt("workspace.new-window", &params, result).is_none());
    }
}
fn request(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Option<(&'static str, serde_json::Value)> {
    let mut args: Vec<_> = args.into_iter().take(8).collect();
    if args.len() > 7 {
        return None;
    }
    let label = if args.first().is_some_and(|arg| arg == OsStr::new("-w")) {
        let label = pulse_app::window_directory::WindowLabel::parse(args.get(1)?.to_str()?).ok()?;
        args.drain(..2);
        Some(label)
    } else {
        None
    };
    let (method, mut params) = base_request(args)?;
    if let Some(label) = label {
        if method == "workspace.windows" {
            return None;
        }
        params.as_object_mut()?.insert(
            "window".into(),
            serde_json::Value::String(label.as_str().to_owned()),
        );
    }
    Some((method, params))
}
fn base_request(args: Vec<std::ffi::OsString>) -> Option<(&'static str, serde_json::Value)> {
    if args.first().is_some_and(|arg| arg == OsStr::new("ssh")) {
        let authentication = match args.get(4).and_then(|arg| arg.to_str()) {
            Some("--interactive") => Some("interactive"),
            Some("--key-interactive") => Some("key-interactive"),
            Some("--agent-interactive") => Some("agent-interactive"),
            Some("--password-interactive") => Some("password-interactive"),
            _ => None,
        };
        if (authentication.is_none() && args.len() != 4)
            || args.len() > 5
            || args[2] != OsStr::new("--identity")
        {
            return None;
        }
        let host = args[1].to_str()?;
        let identity = args[3].to_str()?;
        for text in [host, identity] {
            let id = uuid::Uuid::parse_str(text).ok()?;
            if id.get_version_num() != 7
                || id.get_variant() != uuid::Variant::RFC4122
                || id.to_string() != text
            {
                return None;
            }
        }
        Some((
            "workspace.ssh",
            if let Some(authentication) = authentication {
                serde_json::json!({"host":host,"identity":identity,"authentication":authentication})
            } else {
                serde_json::json!({"host":host,"identity":identity})
            },
        ))
    } else if args
        .first()
        .is_some_and(|arg| arg == OsStr::new("focus-tab"))
    {
        if args.len() != 2 {
            return None;
        }
        let id = session(&serde_json::json!({"session":args[1].to_str()?}))?;
        Some(("workspace.focus-tab", serde_json::json!({"session":id})))
    } else if args.first().is_some_and(|arg| arg == OsStr::new("open"))
        && args.get(1).is_some_and(|arg| arg == OsStr::new("--host"))
    {
        if args.len() != 3 {
            return None;
        }
        let alias = args[2].to_str()?;
        pulse_core::ssh_endpoint::SshEndpoint::new(alias, "alias.invalid", 22, "metadata").ok()?;
        Some(("workspace.ssh", serde_json::json!({"alias":alias})))
    } else if args
        .first()
        .is_some_and(|arg| arg == OsStr::new("split") || arg == OsStr::new("split-pane"))
    {
        if !(2..=3).contains(&args.len()) {
            return None;
        }
        let direction = match args[1].to_str()? {
            "-v" => "row",
            "-h" => "column",
            _ => return None,
        };
        let mut params = serde_json::json!({"direction":direction});
        if let Some(value) = args.get(2) {
            let profile = value.to_str()?;
            if !cli_profiles::valid(profile) {
                return None;
            }
            params["profile"] = serde_json::Value::String(profile.to_owned());
        }
        Some(("workspace.split", params))
    } else if args
        .first()
        .is_some_and(|arg| arg == OsStr::new("new-tab") || arg == OsStr::new("open"))
    {
        if args.len() != 2 {
            return None;
        }
        let profile = args[1].to_str()?;
        if !cli_profiles::valid(profile) {
            return None;
        }
        Some(("workspace.new-tab", serde_json::json!({"profile":profile})))
    } else {
        Some((method(args)?, serde_json::json!({})))
    }
}
fn session(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;
    if object.len() != 1 {
        return None;
    }
    let id = object.get("session")?.as_str()?;
    if id.len() > 20
        || id.starts_with('0')
        || !id.bytes().all(|byte| byte.is_ascii_digit())
        || id.parse::<u64>().ok()? == 0
    {
        return None;
    }
    Some(id.to_owned())
}
#[cfg(any(target_os = "linux", windows, test))]
fn selected(value: &serde_json::Value, expected: &str) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == 2
        && object
            .get("selectionConfirmed")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && object.get("session").and_then(serde_json::Value::as_str) == Some(expected)
        && session(&serde_json::json!({"session":expected})).is_some()
}
#[cfg(any(target_os = "linux", windows, test))]
fn receipt(method: &str, params: &serde_json::Value, value: serde_json::Value) -> Option<String> {
    match method {
        "workspace.focus" if value == serde_json::json!({"focusRequested":true}) => {
            Some("focus requested".to_owned())
        }
        "workspace.windows" => {
            cli_windows::decode(&value)?;
            Some(value.to_string())
        }
        "workspace.new-window" => {
            let object = value.as_object()?;
            if object.len() != 1 {
                return None;
            }
            let label =
                pulse_app::window_directory::WindowLabel::parse(object.get("window")?.as_str()?)
                    .ok()?;
            if label.as_str() == "workspace" {
                return None;
            }
            Some(label.as_str().to_owned())
        }
        "workspace.profiles" => {
            let kinds = cli_profiles::decode(&value).ok()?;
            cli_profiles::project(kinds.into_iter().map(|kind| (kind, false)))
                .ok()
                .map(|public| public.to_string())
        }
        "workspace.focus-tab" => {
            let id = params.get("session")?.as_str()?;
            selected(&value, id).then(|| format!("selected session {id}"))
        }
        "workspace.new-tab" | "workspace.split" | "workspace.ssh" => {
            session(&value).map(|id| format!("session {id}"))
        }
        _ => None,
    }
}
#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).take(65).collect();
    if let Some(text) = cli_information::output(&args) {
        print!("{text}");
        return ExitCode::SUCCESS;
    }
    // Validate every step before touching endpoints, spawning a window, or authenticating.
    let Some(steps) = pulse_chain::parse(args) else {
        eprintln!(
            "pulse: invalid or unavailable action; run 'pulse --help' or 'pulse --help=zh-CN'"
        );
        return ExitCode::from(2);
    };
    #[cfg(any(target_os = "linux", windows))]
    {
        let Ok(directory) = pulseterm_desktop::cli_paths::directory() else {
            eprintln!("pulse: no private runtime directory");
            return ExitCode::FAILURE;
        };
        let mut continuation = None;
        for (index, (method, params)) in steps.iter().enumerate() {
            let reply = if index == 0 && steps.len() == 1 {
                pulse_cold::dispatch(&directory, method, params.clone()).await
            } else if index == 0 {
                match pulse_cold::dispatch_connected(&directory, method, params.clone()).await {
                    Ok((value, client)) => {
                        continuation = Some(client);
                        Ok(value)
                    }
                    Err(error) => Err(error),
                }
            } else {
                let Some(client) = continuation.as_mut() else {
                    return ExitCode::FAILURE;
                };
                client
                    .invoke(method, params.clone())
                    .await
                    .map_err(|_| "pulse.chain_denied")
            };
            let line = reply.ok().and_then(|value| receipt(method, params, value));
            let Some(line) = line else {
                // Unknown results never retry or roll back already registered sessions.
                eprintln!("pulse: step {} not confirmed", index + 1);
                return ExitCode::FAILURE;
            };
            println!("{line}");
        }
        ExitCode::SUCCESS
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = steps;
        eprintln!("pulse: CLI is not available on this platform");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_help_languages_use_examples_accepted_by_the_actual_chain_parser() {
        for flag in ["--help", "--help=zh-CN"] {
            let Some(text) = cli_information::output(&[flag.into()]) else {
                panic!("fixture.help_missing")
            };
            let mut examples = 0;
            for line in text
                .lines()
                .filter_map(|line| line.strip_prefix("  $ pulse "))
            {
                assert!(
                    pulse_chain::parse(line.split_whitespace().map(std::ffi::OsString::from))
                        .is_some()
                );
                examples += 1;
            }
            assert_eq!(examples, 3);
        }
    }
    #[test]
    fn only_exact_closed_commands_are_accepted() {
        for (input, expected) in [
            (vec!["focus"], Some("workspace.focus")),
            (vec!["profiles"], Some("workspace.profiles")),
            (vec![], None),
            (vec!["profiles", "/bin/sh"], None),
            (vec!["profiles", "--program=/bin/sh"], None),
            (vec!["profiles", "--all"], None),
            (vec!["focus;profiles"], None),
            (vec!["settings"], None),
        ] {
            assert_eq!(
                method(input.into_iter().map(std::ffi::OsString::from)),
                expected
            );
        }
    }
    #[test]
    fn cli_new_tab_never_accepts_program_argv_env_or_extra_profile() {
        for input in [
            vec!["new-tab"],
            vec!["new-tab", "/bin/sh"],
            vec!["new-tab", "sh", "--login"],
            vec!["new-tab", "sh", "--env=PATH=x"],
            vec!["new-tab", "--program=/bin/sh"],
            vec!["new-tab", "sh", "fish"],
        ] {
            assert!(request(input.into_iter().map(std::ffi::OsString::from)).is_none());
        }
        assert_eq!(
            request(["new-tab", "sh"].into_iter().map(std::ffi::OsString::from)),
            Some(("workspace.new-tab", serde_json::json!({"profile":"sh"})))
        );
        for value in [
            serde_json::json!({"session":"0"}),
            serde_json::json!({"session":"01"}),
            serde_json::json!({"session":"1\ncommand"}),
            serde_json::json!({"session":"18446744073709551616"}),
            serde_json::json!({"session":"2","path":"/bin/sh"}),
        ] {
            assert!(session(&value).is_none());
        }
        assert_eq!(
            session(&serde_json::json!({"session":"2"})),
            Some("2".to_owned())
        );
    }
    #[test]
    fn split_requires_closed_direction_and_backend_profile() -> Result<(), &'static str> {
        for direction in ["-v", "-h"] {
            let (_, args) = request(
                ["split", direction, "sh"]
                    .into_iter()
                    .map(std::ffi::OsString::from),
            )
            .ok_or("missing split")?;
            assert_eq!(
                args["direction"],
                if direction == "-v" { "row" } else { "column" }
            );
            let (_, default_args) = request(
                ["split", direction]
                    .into_iter()
                    .map(std::ffi::OsString::from),
            )
            .ok_or("missing default split")?;
            assert_eq!(
                default_args,
                serde_json::json!({"direction":if direction == "-v" {"row"} else {"column"}})
            );
        }
        for input in [
            vec!["split"],
            vec!["split", "-x", "sh"],
            vec!["split", "-v", "/bin/sh"],
            vec!["split", "-v", "sh", "--env=PATH=x"],
        ] {
            assert!(request(input.into_iter().map(std::ffi::OsString::from)).is_none());
        }
        Ok(())
    }
    #[test]
    fn open_host_alias_is_exact_saved_metadata_not_an_address_or_extra_options()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let parse = |args: Vec<&str>| request(args.into_iter().map(std::ffi::OsString::from));
        let (method, params) =
            parse(vec!["open", "--host", "生产入口 Alpha"]).ok_or("fixture.alias")?;
        assert_eq!(method, "workspace.ssh");
        assert_eq!(params, serde_json::json!({"alias":"生产入口 Alpha"}));
        assert!(
            pulseterm_desktop::cli_startup::StartupAction::new(method, &params)
                .map_err(|_| "fixture.startup")?
                .supports_cold_transport()
        );
        for input in [
            vec!["open", "--host"],
            vec!["open", "--host", " x"],
            vec!["open", "--host", "x\n"],
            vec!["open", "--host", "x", "--identity=other"],
            vec!["new-tab", "--host", "x"],
        ] {
            assert!(parse(input).is_none());
        }
        Ok(())
    }
    #[test]
    fn ssh_accepts_only_canonical_saved_ids_without_address_secret_or_program() {
        let host = "11111111-1111-7111-8111-111111111111";
        let identity = "22222222-2222-7222-8222-222222222222";
        assert_eq!(
            request(
                ["ssh", host, "--identity", identity]
                    .into_iter()
                    .map(std::ffi::OsString::from)
            ),
            Some((
                "workspace.ssh",
                serde_json::json!({"host":host,"identity":identity})
            ))
        );
        for input in [
            vec!["ssh"],
            vec!["ssh", host],
            vec!["ssh", "127.0.0.1", "--identity", identity],
            vec!["ssh", host, "--password", identity],
            vec![
                "ssh",
                host,
                "--identity",
                "00000000-0000-4000-8000-000000000000",
            ],
            vec!["ssh", host, "--identity", identity, "--program=/bin/sh"],
            vec!["ssh", host, "--identity", identity, "--identity", identity],
        ] {
            assert!(request(input.into_iter().map(std::ffi::OsString::from)).is_none());
        }
    }
    #[test]
    fn local_aliases_use_existing_actions_and_reject_unbounded_targets()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        for profile in ["bash", "zsh", "fish", "sh", "pwsh"] {
            let parse = |args: Vec<&str>| request(args.into_iter().map(std::ffi::OsString::from));
            let opened = parse(vec!["open", profile]);
            assert_eq!(opened, parse(vec!["new-tab", profile]));
            let (method, params) = opened.ok_or("fixture.open-route")?;
            let cold = pulseterm_desktop::cli_startup::StartupAction::new(method, &params)
                .map_err(|_| "fixture.open-admission")?;
            assert!(cold.supports_cold_transport());
            assert!(!cold.supports_cold_readonly_transport());
            for direction in ["-v", "-h"] {
                let split = parse(vec!["split-pane", direction, profile]);
                assert_eq!(split, parse(vec!["split", direction, profile]));
                let (method, params) = split.ok_or("fixture.split-route")?;
                assert!(
                    pulseterm_desktop::cli_startup::StartupAction::new(method, &params)
                        .map_err(|_| "fixture.split-admission")?
                        .supports_cold_transport()
                );
            }
        }
        for args in [
            vec!["open"],
            vec!["open", "/bin/sh"],
            vec!["open", "ssh://host"],
            vec!["open", "sh", "--env=PATH=x"],
            vec!["open", "sh", "bash"],
            vec!["open", "SH"],
            vec!["open", "sh\ncmd"],
            vec!["open", "sh;focus"],
            vec!["split-pane", "-v", "/bin/sh"],
            vec!["split-pane", "-x", "sh"],
            vec!["split-pane", "-h", "sh", "--login"],
        ] {
            assert!(request(args.into_iter().map(std::ffi::OsString::from)).is_none());
        }
        Ok(())
    }
    #[test]
    fn fixed_window_selector_is_metadata_and_cannot_expand_action_parameters() {
        for args in [
            vec!["focus"],
            vec!["profiles"],
            vec!["focus-tab", "1"],
            vec!["open", "sh"],
            vec!["split-pane", "-v", "sh"],
            vec![
                "ssh",
                "01890f01-2b70-7abc-8def-012345678901",
                "--identity",
                "01890f01-2b70-7abc-8def-012345678902",
            ],
        ] {
            let plain = request(args.iter().copied().map(std::ffi::OsString::from));
            let scoped = request(
                ["-w", "workspace"]
                    .into_iter()
                    .chain(args)
                    .map(std::ffi::OsString::from),
            );
            let Some(mut expected) = plain else {
                panic!("fixture.plain_request")
            };
            let Some(object) = expected.1.as_object_mut() else {
                panic!("fixture.request_object")
            };
            object.insert("window".into(), serde_json::json!("workspace"));
            assert_eq!(Some(expected), scoped);
        }
    }
    #[test]
    fn cli_window_metadata_is_canonical_closed_and_separate_from_action_args() {
        let label = format!("workspace-{}", uuid::Uuid::new_v4());
        let args = ["-w", label.as_str(), "new-tab", "sh"];
        assert_eq!(
            request(args.into_iter().map(std::ffi::OsString::from)),
            Some((
                "workspace.new-tab",
                serde_json::json!({"profile":"sh","window":label})
            ))
        );
        assert_eq!(
            request(["windows"].into_iter().map(std::ffi::OsString::from)),
            Some(("workspace.windows", serde_json::json!({})))
        );
        assert!(
            request(
                ["-w", "workspace", "windows"]
                    .into_iter()
                    .map(std::ffi::OsString::from)
            )
            .is_none()
        );
        for value in [
            serde_json::json!({"version":1,"windows":["workspace"],"token":"x"}),
            serde_json::json!({"version":1,"windows":["workspace","workspace"]}),
        ] {
            assert!(receipt("workspace.windows", &serde_json::json!({}), value).is_none());
        }
        for args in [
            vec!["-w"],
            vec!["-w", "workspace"],
            vec!["-w", "other", "focus"],
            vec!["-w", "../workspace", "focus"],
            vec!["-w", "workspace", "-w", "workspace", "focus"],
            vec!["-w", "workspace", "focus", "/bin/sh"],
            vec!["focus", "-w", "workspace"],
            vec!["-w", "workspace", "open", "sh", "--env=X=1"],
            vec![
                "-w",
                "workspace",
                "ssh",
                "01890f01-2b70-7abc-8def-012345678901",
                "--identity",
                "01890f01-2b70-7abc-8def-012345678902",
                "--password=x",
            ],
        ] {
            assert!(request(args.into_iter().map(std::ffi::OsString::from)).is_none());
        }
    }
    #[test]
    fn focus_tab_accepts_one_canonical_root_id_and_exact_matching_receipt() {
        for id in ["1", "18446744073709551615"] {
            assert_eq!(
                request(["focus-tab", id].into_iter().map(std::ffi::OsString::from)),
                Some(("workspace.focus-tab", serde_json::json!({"session":id})))
            );
            assert!(selected(
                &serde_json::json!({"session":id,"selectionConfirmed":true}),
                id
            ));
        }
        for id in [
            "",
            "0",
            "01",
            "+1",
            "-1",
            "1\ncmd",
            "18446744073709551616",
            "/bin/sh",
        ] {
            assert!(request(["focus-tab", id].into_iter().map(std::ffi::OsString::from)).is_none());
        }
        for args in [
            vec!["focus-tab"],
            vec!["focus-tab", "1", "--program=/bin/sh"],
            vec!["focus-tab", "1", "2"],
        ] {
            assert!(request(args.into_iter().map(std::ffi::OsString::from)).is_none());
        }
        for value in [
            serde_json::json!({"session":"1"}),
            serde_json::json!({"session":"2","selectionConfirmed":true}),
            serde_json::json!({"session":1,"selectionConfirmed":true}),
            serde_json::json!({"session":"1","selectionConfirmed":false}),
            serde_json::json!({"session":"1","selectionConfirmed":"true"}),
            serde_json::json!({"session":"1","selectionConfirmed":true,"path":"/bin/sh"}),
        ] {
            assert!(!selected(&value, "1"));
        }
    }
    #[test]
    fn interactive_cli_requires_exact_saved_ids_and_explicit_flag()
    -> Result<(), Box<dyn std::error::Error>> {
        let host = "01890f01-2b70-7abc-8def-012345678901";
        let identity = "01890f01-2b70-7abc-8def-012345678902";
        let parsed = request(
            ["ssh", host, "--identity", identity, "--interactive"]
                .into_iter()
                .map(std::ffi::OsString::from),
        )
        .ok_or("fixture.missing")?;
        assert_eq!(parsed.0, "workspace.ssh");
        assert_eq!(
            parsed.1,
            serde_json::json!({"host":host,"identity":identity,"authentication":"interactive"})
        );
        for args in [
            vec!["ssh", "GUI_HOST", "--interactive"],
            vec!["ssh", host, "--identity", identity, "--password=x"],
            vec![
                "ssh",
                host,
                "--identity",
                identity,
                "--interactive",
                "--interactive",
            ],
            vec!["ssh", host, "--identity", identity, "--interactive=false"],
        ] {
            assert!(request(args.into_iter().map(std::ffi::OsString::from)).is_none());
        }
        Ok(())
    }
    #[test]
    fn key_interactive_cli_is_a_separate_saved_identity_choice()
    -> Result<(), Box<dyn std::error::Error>> {
        let host = "01890f01-2b70-7abc-8def-012345678901";
        let identity = "01890f01-2b70-7abc-8def-012345678902";
        let parsed = request(
            ["ssh", host, "--identity", identity, "--key-interactive"]
                .into_iter()
                .map(std::ffi::OsString::from),
        )
        .ok_or("fixture.missing")?;
        assert_eq!(
            parsed.1,
            serde_json::json!({"host":host,"identity":identity,"authentication":"key-interactive"})
        );
        for args in [
            vec!["ssh", "GUI_HOST", "--key-interactive"],
            vec![
                "ssh",
                host,
                "--identity",
                identity,
                "--interactive",
                "--key-interactive",
            ],
            vec![
                "ssh",
                host,
                "--identity",
                identity,
                "--key-interactive",
                "--interactive",
            ],
            vec![
                "ssh",
                host,
                "--identity",
                identity,
                "--key-interactive=false",
            ],
            vec![
                "ssh",
                host,
                "--identity",
                identity,
                "--key-interactive",
                "--key-interactive",
            ],
        ] {
            assert!(request(args.into_iter().map(std::ffi::OsString::from)).is_none());
        }
        Ok(())
    }
}
