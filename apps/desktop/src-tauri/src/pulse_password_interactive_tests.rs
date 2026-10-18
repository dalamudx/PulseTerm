use super::*;
const HOST: &str = "01890f01-2b70-7abc-8def-012345678901";
const IDENTITY: &str = "01890f01-2b70-7abc-8def-012345678902";
#[test]
fn password_joint_cli_is_explicit_closed_and_preflights_whole_chains()
-> Result<(), Box<dyn std::error::Error>> {
    let args = [
        "-w",
        "workspace",
        "ssh",
        HOST,
        "--identity",
        IDENTITY,
        "--password-interactive",
        ";",
        "profiles",
    ];
    let steps = pulse_chain::parse(args.map(std::ffi::OsString::from)).ok_or("fixture.cli")?;
    assert_eq!(steps.len(), 2);
    assert_eq!(
        steps[0].1,
        serde_json::json!({"host":HOST,"identity":IDENTITY,"authentication":"password-interactive","window":"workspace"})
    );
    assert_eq!(steps[1].0, "workspace.profiles");
    let cold = pulseterm_desktop::cli_startup::StartupAction::new("workspace.ssh", &steps[0].1)?;
    let roundtrip = pulseterm_desktop::cli_startup::StartupAction::parse(&cold.encode()?)?;
    assert_eq!(roundtrip.params(), cold.params());
    assert!(roundtrip.supports_cold_transport());
    assert!(!roundtrip.supports_cold_readonly_transport());
    for extra in [
        "--interactive",
        "--key-interactive",
        "--agent-interactive",
        "--password-interactive",
        "--password",
        "--password-interactive=false",
    ] {
        let invalid = [
            "profiles",
            ";",
            "ssh",
            HOST,
            "--identity",
            IDENTITY,
            "--password-interactive",
            extra,
        ];
        assert!(pulse_chain::parse(invalid.map(std::ffi::OsString::from)).is_none());
    }
    for params in [
        serde_json::json!({"alias":"Saved","authentication":"password-interactive"}),
        serde_json::json!({"host":HOST,"identity":IDENTITY,"authentication":"password-interactive","password":"literal"}),
        serde_json::json!({"host":HOST,"identity":IDENTITY,"authentication":"password-interactive","answers":[]}),
        serde_json::json!({"host":HOST,"identity":IDENTITY,"authentication":null}),
    ] {
        assert!(
            pulseterm_desktop::cli_startup::StartupAction::new("workspace.ssh", &params).is_err()
        );
    }
    assert!(
        request(["ssh", "Saved", "--password-interactive"].map(std::ffi::OsString::from)).is_none()
    );
    Ok(())
}
