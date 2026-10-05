#[cfg(unix)]
use anyhow::Context;
use anyhow::{Result, ensure};
#[cfg(any(unix, test))]
use serde_json::{Value, json};
#[cfg(unix)]
fn command() -> Result<pulse_pty::PtyCommand> {
    use pulse_pty::{
        launch::{LocalSessionEnvironment, LocalShellMode, prepare_local_profile_command},
        profiles::{ShellKind, discover_unix_shells},
    };
    let candidates = discover_unix_shells(std::ffi::OsStr::new("/bin:/usr/bin"), None)?;
    let bash = candidates
        .iter()
        .find(|shell| shell.kind() == ShellKind::Bash)
        .context("profile_session.bash_missing")?;
    let original = super::local_session_probe::command("portable-pty-helper-stream")?;
    let mut command = pulse_pty::PtyCommand::new(bash.executable());
    command.args = vec!["--noprofile".into(), "--norc".into(), "-c".into(), "builtin shopt -q login_shell || exit 90\n[ \"$TERM_PROGRAM\" = PulseTerm ] && [ \"$TERM\" = xterm-256color ] && [ \"$COLORTERM\" = truecolor ] || exit 91\n[ \"$TERM_PROGRAM_VERSION\" = \"$PULSE_PROFILE_TEST_VERSION\" ] || exit 92\n[ \"${PULSE_PANE_ID+x}\" != x ] && [ \"${PULSE_SOCKET+x}\" != x ] || exit 93\nexec \"$@\"".into(), "pulseterm-profile-bootstrap".into(), original.program];
    command.args.extend(original.args);
    command.cwd = original.cwd;
    command.env = original.env;
    command.env.push((
        "PULSE_PROFILE_TEST_VERSION".into(),
        env!("CARGO_PKG_VERSION").into(),
    ));
    command.env_remove = ["BASH_ENV", "ENV", "SHELLOPTS", "BASHOPTS"]
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(prepare_local_profile_command(
        command,
        &LocalSessionEnvironment {
            application_version: env!("CARGO_PKG_VERSION").into(),
            pane: None,
        },
        Some(ShellKind::Bash),
        LocalShellMode::Login,
    )?)
}
#[cfg(any(unix, test))]
fn validate(report: &Value) -> Result<()> {
    for key in [
        "parameterLoginVerified",
        "terminalDefaultsVerified",
        "startupFilesDisabled",
        "literalHelperArguments",
        "noProgramFallback",
        "orderedEofAndReap",
    ] {
        ensure!(
            report[key].as_bool() == Some(true),
            "profile_session.invalid_receipt"
        );
    }
    let wire = &report["wire"];
    for key in [
        "registryOwnerGenerationAuthorization",
        "ackToNativeBackpressure",
        "orderedEofAck",
        "burstExactChecked",
    ] {
        ensure!(
            wire[key].as_bool() == Some(true),
            "profile_session.invalid_wire_receipt"
        );
    }
    ensure!(
        cfg!(unix)
            && report["schemaVersion"] == 1
            && report["platform"] == std::env::consts::OS
            && report["shell"] == "bash"
            && report["mode"] == "login"
            && wire["platform"] == std::env::consts::OS
            && wire["burstBytes"] == 2 * 1024 * 1024
            && wire["creditLimitBytes"] == 9000
            && wire["creditPeakBytes"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= 9000)
            && wire["wireFrames"].as_u64().is_some_and(|n| n > 0)
            && wire["pausedParsedBytes"].as_u64().is_some_and(|n| n > 0)
            && wire["finalParsedBytes"]
                .as_u64()
                .is_some_and(|n| n >= 2 * 1024 * 1024)
            && wire["renderer"] == "Rust validator; not frontend callbacks",
        "profile_session.invalid_report"
    );
    Ok(())
}
pub fn run() -> Result<()> {
    ensure!(cfg!(unix), "profile_session.requires_unix");
    #[cfg(unix)]
    {
        let report_path = super::root()?.join("target/probes/local-profile-session.json");
        std::fs::create_dir_all(
            report_path
                .parent()
                .context("profile_session.report_parent")?,
        )?;
        if report_path.exists() {
            std::fs::remove_file(&report_path)?;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let wire = runtime.block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(18),
                super::local_session_probe::wire_streaming_command(command()?),
            )
            .await
            .context("profile_session.deadline")?
        })?;
        let report = json!({"schemaVersion":1,"platform":std::env::consts::OS,"shell":"bash","mode":"login","parameterLoginVerified":true,"terminalDefaultsVerified":true,"startupFilesDisabled":true,"literalHelperArguments":true,"noProgramFallback":true,"orderedEofAndReap":true,"wire":wire});
        validate(&report)?;
        std::fs::write(report_path, serde_json::to_vec_pretty(&report)?)?;
        println!(
            "PASS Unix login profile → native PTY → authorized actor: 2MiB ACK backpressure, lossless burst and ordered EOF"
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_receipt_requires_native_pressure_and_exact_burst() {
        let report = json!({"schemaVersion":1,"platform":std::env::consts::OS,"shell":"bash","mode":"login","parameterLoginVerified":true,"terminalDefaultsVerified":true,"startupFilesDisabled":true,"literalHelperArguments":true,"noProgramFallback":true,"orderedEofAndReap":true,"wire":{"platform":std::env::consts::OS,"registryOwnerGenerationAuthorization":true,"ackToNativeBackpressure":true,"orderedEofAck":true,"burstExactChecked":true,"burstBytes":2*1024*1024,"creditLimitBytes":9000,"creditPeakBytes":8212,"wireFrames":500,"pausedParsedBytes":8192,"finalParsedBytes":2*1024*1024+64,"renderer":"Rust validator; not frontend callbacks"}});
        assert_eq!(validate(&report).is_ok(), cfg!(unix));
        for key in [
            "parameterLoginVerified",
            "terminalDefaultsVerified",
            "startupFilesDisabled",
            "literalHelperArguments",
            "noProgramFallback",
            "orderedEofAndReap",
        ] {
            for bad in [Value::Null, json!(false), json!("true")] {
                let mut wrong = report.clone();
                wrong[key] = bad;
                assert!(validate(&wrong).is_err());
            }
        }
        for (key, bad) in [
            ("burstBytes", json!(2 * 1024 * 1024 - 1)),
            ("burstBytes", json!(2 * 1024 * 1024 + 1)),
            ("creditPeakBytes", json!(9001)),
            ("creditPeakBytes", json!(-1)),
            ("creditLimitBytes", json!(9001)),
            ("ackToNativeBackpressure", json!("true")),
            ("orderedEofAck", json!(false)),
            ("burstExactChecked", json!(false)),
            ("burstExactChecked", Value::Null),
            ("pausedParsedBytes", json!(0)),
            ("renderer", json!("DOM")),
        ] {
            let mut wrong = report.clone();
            wrong["wire"][key] = bad;
            assert!(validate(&wrong).is_err());
        }
        assert!(validate(&json!({"completed":true})).is_err());
    }
}
