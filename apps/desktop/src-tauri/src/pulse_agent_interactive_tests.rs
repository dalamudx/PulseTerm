//! The Agent joint method is a closed saved-ID action, including cold transport and chains.
use super::*;
const HOST: &str = "01890f01-2b70-7abc-8def-012345678901";
const IDENTITY: &str = "01890f01-2b70-7abc-8def-012345678902";
#[test]
fn agent_joint_cli_keeps_exact_method_window_and_sequence() -> Result<(), Box<dyn std::error::Error>>
{
    let steps = pulse_chain::parse(
        [
            "-w",
            "workspace",
            "ssh",
            HOST,
            "--identity",
            IDENTITY,
            "--agent-interactive",
            ";",
            "profiles",
        ]
        .into_iter()
        .map(std::ffi::OsString::from),
    )
    .ok_or("fixture.closed_cli")?;
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].0, "workspace.ssh");
    assert_eq!(
        steps[0].1,
        serde_json::json!({
            "host":HOST, "identity":IDENTITY, "authentication":"agent-interactive", "window":"workspace"
        })
    );
    assert_eq!(steps[1].0, "workspace.profiles");
    let cold = pulseterm_desktop::cli_startup::StartupAction::new(
        "workspace.ssh",
        &serde_json::json!({
            "host":HOST, "identity":IDENTITY, "authentication":"agent-interactive"
        }),
    )?;
    let roundtrip = pulseterm_desktop::cli_startup::StartupAction::parse(&cold.encode()?)?;
    assert_eq!(
        roundtrip.params().get("authentication"),
        Some(&serde_json::json!("agent-interactive"))
    );
    assert!(roundtrip.supports_cold_transport());
    assert!(!roundtrip.supports_cold_readonly_transport());
    for suffix in [
        vec!["--agent-interactive", "--interactive"],
        vec!["--key-interactive", "--agent-interactive"],
        vec!["--agent-interactive", "--agent-interactive"],
        vec!["--agent-interactive=false"],
        vec!["--agent-interactive", "--password", "literal"],
    ] {
        let mut args = vec!["ssh", HOST, "--identity", IDENTITY];
        args.extend(suffix);
        assert!(pulse_chain::parse(args.into_iter().map(std::ffi::OsString::from)).is_none());
    }
    assert!(
        request(
            ["ssh", "Saved", "--agent-interactive"]
                .into_iter()
                .map(std::ffi::OsString::from)
        )
        .is_none()
    );
    for params in [
        serde_json::json!({"alias":"Saved", "authentication":"agent-interactive"}),
        serde_json::json!({"host":HOST, "identity":IDENTITY, "authentication":"agent-interactive", "agent":true}),
        serde_json::json!({"host":HOST, "identity":IDENTITY, "authentication":"agent-interactive", "answers":["literal"]}),
    ] {
        assert!(
            pulseterm_desktop::cli_startup::StartupAction::new("workspace.ssh", &params).is_err()
        );
    }
    Ok(())
}
