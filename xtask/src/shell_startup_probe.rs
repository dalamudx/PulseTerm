//! Isolated Linux Bash startup/hook compatibility probe, not production injection.
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
#[cfg(any(target_os = "linux", test))]
use serde_json::json;
use std::{
    thread,
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
const INTEGRATION: &str = include_str!("fixtures/bash-integration-probe.bash");
#[cfg(target_os = "linux")]
fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}
#[cfg(target_os = "linux")]
fn prompts(bytes: &[u8]) -> usize {
    bytes
        .windows(8)
        .filter(|window| *window == b"\x1b]133;B\x07")
        .count()
}
#[cfg(target_os = "linux")]
fn until(
    rx: &std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>,
    bytes: &mut Vec<u8>,
    terminal: &mut pulse_term::TerminalProbe,
    prompt: Option<usize>,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if prompt.is_some_and(|count| prompts(bytes) >= count) {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let chunk = rx
            .recv_timeout(remaining)
            .map_err(|_| anyhow::anyhow!("shell_startup.read_deadline"))??;
        if chunk.is_empty() {
            ensure!(prompt.is_none(), "shell_startup.early_eof");
            terminal.finish_input();
            return Ok(());
        }
        ensure!(
            bytes.len() + chunk.len() <= 16384,
            "shell_startup.output_budget"
        );
        terminal.feed(&chunk);
        bytes.extend(chunk);
    }
}
#[cfg(target_os = "linux")]
pub fn worker() -> Result<()> {
    use pulse_pty::{
        PtySession,
        launch::{LocalSessionEnvironment, LocalShellMode},
        profiles::{AutoProfileCatalog, AutoProfileLaunch, ShellKind, discover_unix_shells},
    };
    use std::{io::Read, os::unix::fs::DirBuilderExt};
    let directory = super::root()?
        .join("target/probes")
        .join(format!("shell-startup-owned-{}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let mut catalog = AutoProfileCatalog::default();
    catalog.reconcile(&discover_unix_shells(
        std::ffi::OsStr::new("/bin:/usr/bin"),
        None,
    )?)?;
    let user = directory.join("user.bashrc");
    let integration = directory.join("integration.bash");
    let wrapper = directory.join("wrapper.bashrc");
    std::fs::write(&integration, INTEGRATION)?;
    std::fs::write(
        &wrapper,
        ". \"$PULSE_TEST_USER_RC\"\n. \"$PULSE_TEST_INTEGRATION\"\n. \"$PULSE_TEST_INTEGRATION\"\n",
    )?;
    for array in [false, true] {
        let hook = if array {
            "PROMPT_COMMAND=(user_hook user_second)"
        } else {
            "PROMPT_COMMAND=user_hook"
        };
        std::fs::write(
            &user,
            format!(
                "[[ $BASH_VERSINFO -ge 5 ]] || exit 80\nPULSE_USER_LOADS=$(( ${{PULSE_USER_LOADS:-0}} + 1 ))\nPROFILE_ENV='literal 中文 value'\nPS1='USER_PROMPT> '\nPS0='USER_PRE>'\nuser_hook() {{ local code=$?; builtin printf 'USER_HOOK:%d\\n' \"$code\"; return \"$code\"; }}\nuser_second() {{ local code=$?; return \"$code\"; }}\n{hook}\n"
            ),
        )?;
        let command = catalog.prepare_launch(
            ShellKind::Bash,
            AutoProfileLaunch {
                args: vec![
                    "--noprofile".into(),
                    "--rcfile".into(),
                    wrapper.clone().into(),
                    "-i".into(),
                ],
                cwd: Some(directory.clone()),
                env: vec![
                    ("HOME".into(), directory.clone().into()),
                    ("PULSE_TEST_USER_RC".into(), user.clone().into()),
                    ("PULSE_TEST_INTEGRATION".into(), integration.clone().into()),
                    ("PULSE_USER_LOADS".into(), "0".into()),
                    ("PULSE_TEST_INJECTED".into(), "0".into()),
                    ("_PULSE_SEEN".into(), "0".into()),
                ],
                env_remove: [
                    "BASH_ENV",
                    "ENV",
                    "SHELLOPTS",
                    "BASHOPTS",
                    "PROMPT_COMMAND",
                    "PS1",
                    "PS0",
                ]
                .map(Into::into)
                .into(),
                mode: LocalShellMode::Plain,
            },
            &LocalSessionEnvironment {
                application_version: env!("CARGO_PKG_VERSION").into(),
                pane: None,
            },
        )?;
        let mut session = PtySession::spawn(&command, pulse_core::TermSize::new(80, 24)?)
            .map_err(|_| anyhow::anyhow!("shell_startup.spawn"))?;
        let mut reader = session.take_reader()?;
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let pump = thread::spawn(move || {
            loop {
                let mut buffer = [0; 1024];
                let result = reader
                    .read(&mut buffer)
                    .map(|count| buffer[..count].to_vec());
                let finished = !result.as_ref().is_ok_and(|bytes| !bytes.is_empty());
                if tx.send(result).is_err() || finished {
                    break;
                }
            }
        });
        let result = (|| -> Result<()> {
            let mut bytes = Vec::new();
            let mut terminal =
                pulse_term::TerminalProbe::new(pulse_core::TermSize::new(80, 24)?, 0);
            until(&rx, &mut bytes, &mut terminal, Some(1))?;
            ensure!(
                contains(&bytes, b"\x1b]133;A\x07USER_PROMPT> "),
                "shell_startup.user_prompt"
            );
            session.write_input(b"false\n")?;
            until(&rx, &mut bytes, &mut terminal, Some(2))?;
            ensure!(
                contains(&bytes, b"\x1b]133;C\x07USER_PRE>")
                    && contains(&bytes, b"\x1b]133;D;1\x07USER_HOOK:1"),
                "shell_startup.status_and_precommand"
            );
            session.write_input(b"true\n")?;
            until(&rx, &mut bytes, &mut terminal, Some(3))?;
            ensure!(
                contains(&bytes, b"\x1b]133;D;0\x07USER_HOOK:0"),
                "shell_startup.success_status"
            );
            let count = if array { 3 } else { 2 };
            let exit = format!(
                "[[ $PULSE_USER_LOADS == 1 && $PROFILE_ENV == 'literal 中文 value' && ${{#PROMPT_COMMAND[@]}} == {count} && $- == *i* ]] && exit 7; exit 81\n"
            );
            session.write_input(exit.as_bytes())?;
            until(&rx, &mut bytes, &mut terminal, None)?;
            use pulse_term::shell_markers::ShellMarker;
            let hints = terminal.take_shell_markers();
            ensure!(
                hints.dropped == 0
                    && hints
                        .markers
                        .iter()
                        .filter(|marker| **marker == ShellMarker::PromptStart)
                        .count()
                        >= 3
                    && hints
                        .markers
                        .iter()
                        .filter(|marker| **marker == ShellMarker::PromptEnd)
                        .count()
                        >= 3
                    && hints.markers.contains(&ShellMarker::CommandStart)
                    && hints.markers.contains(&ShellMarker::CommandEnd {
                        exit_status: Some(1)
                    })
                    && hints.markers.contains(&ShellMarker::CommandEnd {
                        exit_status: Some(0)
                    })
                    && !hints.markers.contains(&ShellMarker::CommandEnd {
                        exit_status: Some(7)
                    }),
                "shell_startup.backend_hints"
            );
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                if let Some(status) = session.try_wait()? {
                    ensure!(status.exit_code() == 7, "shell_startup.exit_code");
                    break;
                }
                ensure!(Instant::now() < deadline, "shell_startup.exit_deadline");
                thread::sleep(Duration::from_millis(10));
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = session.terminate();
        }
        drop(rx);
        drop(session);
        pump.join()
            .map_err(|_| anyhow::anyhow!("shell_startup.reader_panicked"))?;
        result?;
    }
    let report = receipt();
    validate(&report)?;
    std::fs::write(
        super::root()?.join("target/probes/shell-startup.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub fn worker() -> Result<()> {
    bail!("shell_startup.requires_linux_bash5");
}
#[cfg(any(target_os = "linux", test))]
fn receipt() -> Value {
    json!({"schemaVersion":2,"platform":std::env::consts::OS,"shell":"bash5","launchCount":2,"method":"private-home-rcfile","userConfigPreserved":true,"scalarHookPreserved":true,"arrayHookPreserved":true,"statusPreserved":true,"injectionIdempotent":true,"oscBoundariesChecked":true,"nativeTerminal":true,"backendHintsChecked":true,"eofAndReap":true,"realUserConfigRead":false,"loginShell":false,"productionIntegration":false,"exitCode":7})
}
fn validate(r: &Value) -> Result<()> {
    for key in [
        "userConfigPreserved",
        "scalarHookPreserved",
        "arrayHookPreserved",
        "statusPreserved",
        "injectionIdempotent",
        "oscBoundariesChecked",
        "nativeTerminal",
        "eofAndReap",
        "backendHintsChecked",
    ] {
        ensure!(
            r[key].as_bool() == Some(true),
            "shell_startup.invalid_receipt"
        );
    }
    for key in ["realUserConfigRead", "loginShell", "productionIntegration"] {
        ensure!(
            r[key].as_bool() == Some(false),
            "shell_startup.invalid_scope"
        );
    }
    ensure!(
        cfg!(target_os = "linux")
            && r["schemaVersion"] == 2
            && r["platform"] == std::env::consts::OS
            && r["shell"] == "bash5"
            && r["launchCount"] == 2
            && r["method"] == "private-home-rcfile"
            && r["exitCode"] == 7,
        "shell_startup.invalid_report"
    );
    Ok(())
}
pub fn run() -> Result<()> {
    ensure!(
        cfg!(target_os = "linux"),
        "shell_startup.requires_linux_bash5"
    );
    let report = super::root()?.join("target/probes/shell-startup.json");
    std::fs::create_dir_all(report.parent().context("shell_startup.report_parent")?)?;
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("probe-shell-startup-worker")
        .stdin(std::process::Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "shell_startup.worker_failed");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&report);
            bail!("shell_startup.worker_deadline");
        }
        thread::sleep(Duration::from_millis(20));
    }
    validate(&serde_json::from_slice(&std::fs::read(report)?)?)?;
    println!(
        "PASS Linux Bash isolated user config, scalar/array prompt hooks, idempotent injection and OSC boundaries"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_receipt_rejects_truthy_or_missing_evidence_and_scope_escalation() {
        let report = receipt();
        assert_eq!(validate(&report).is_ok(), cfg!(target_os = "linux"));
        for key in [
            "userConfigPreserved",
            "scalarHookPreserved",
            "arrayHookPreserved",
            "statusPreserved",
            "injectionIdempotent",
            "oscBoundariesChecked",
            "nativeTerminal",
            "eofAndReap",
            "backendHintsChecked",
        ] {
            for value in [Value::Null, json!(false), json!("true"), json!(1)] {
                let mut bad = report.clone();
                bad[key] = value;
                assert!(validate(&bad).is_err());
            }
        }
        for key in ["realUserConfigRead", "loginShell", "productionIntegration"] {
            let mut bad = report.clone();
            bad[key] = json!(true);
            assert!(validate(&bad).is_err());
        }
        for (key, value) in [
            ("schemaVersion", json!(1)),
            ("method", json!("arguments")),
            ("launchCount", json!(1)),
            ("exitCode", json!(0)),
            ("shell", json!("zsh")),
            ("platform", json!("wrong")),
        ] {
            let mut bad = report.clone();
            bad[key] = value;
            assert!(validate(&bad).is_err());
        }
    }
}
