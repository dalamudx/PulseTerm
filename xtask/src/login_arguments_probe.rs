use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::thread;
use std::time::{Duration, Instant};
const OUTPUT: &[u8] = "LOGIN_ARGUMENTS_OK 中文🌍\r\n".as_bytes();
#[cfg(unix)]
pub fn worker() -> Result<()> {
    use pulse_pty::{
        PtySession,
        launch::{
            LocalSessionEnvironment, LocalShellMode, prepare_local_login_command,
            prepare_local_profile_command,
        },
        profiles::{ShellKind, discover_unix_shells},
    };
    use std::io::Read;
    use std::os::unix::fs::DirBuilderExt;
    let directory = super::root()?
        .join("target/probes")
        .join(format!("login-arguments-owned-{}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let candidates = discover_unix_shells(std::ffi::OsStr::new("/bin:/usr/bin"), None)?;
    let bash = candidates
        .iter()
        .find(|c| c.kind() == ShellKind::Bash)
        .context("login_arguments.bash_missing")?;
    let context = LocalSessionEnvironment {
        application_version: env!("CARGO_PKG_VERSION").into(),
        pane: None,
    };
    for mode in [
        LocalShellMode::Plain,
        LocalShellMode::PlatformDefault,
        LocalShellMode::Login,
    ] {
        let login = match mode {
            LocalShellMode::Plain => false,
            LocalShellMode::Login => true,
            LocalShellMode::PlatformDefault => cfg!(target_os = "macos"),
        };
        let condition = if login {
            "builtin shopt -q login_shell || exit 31"
        } else {
            "if builtin shopt -q login_shell; then exit 31; fi"
        };
        let script = format!(
            "{condition}\n[ -t 0 ] && [ -t 1 ] || exit 32\n[ \"$0\" = 'pulseterm-login-arguments' ] || exit 33\n[ \"$1\" = 'literal ; $(not-a-command) 中文🌍' ] || exit 34\n[ \"$TERM_PROGRAM\" = 'PulseTerm' ] || exit 35\nbuiltin printf 'LOGIN_ARGUMENTS_OK 中文🌍\\n'\nexit 7"
        );
        let mut command = pulse_pty::PtyCommand::new(bash.executable());
        command.args = vec![
            "--noprofile".into(),
            "--norc".into(),
            "-c".into(),
            script.into(),
            "pulseterm-login-arguments".into(),
            "literal ; $(not-a-command) 中文🌍".into(),
        ];
        command.cwd = Some(directory.clone());
        command.env_remove = ["BASH_ENV", "ENV", "SHELLOPTS", "BASHOPTS"]
            .into_iter()
            .map(Into::into)
            .collect();
        let original = command.clone();
        let command =
            prepare_local_profile_command(command, &context, Some(ShellKind::Bash), mode)?;
        ensure!(
            command.program == original.program
                && command.cwd == original.cwd
                && command.args.len() == 6 + usize::from(login),
            "login_arguments.selection"
        );
        ensure!(
            &command.args[usize::from(login)..] == original.args.as_slice(),
            "login_arguments.literal_argv"
        );
        let mut session = PtySession::spawn(&command, pulse_core::TermSize::new(80, 24)?)
            .map_err(|_| anyhow::anyhow!("login_arguments.pty_spawn"))?;
        let reader = session.take_reader()?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let pump = thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = reader.take(4097).read_to_end(&mut bytes).map(|_| bytes);
            let _ = tx.send(result);
        });
        let result = (|| -> Result<()> {
            let bytes = rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| anyhow::anyhow!("login_arguments.output_deadline"))??;
            ensure!(bytes == OUTPUT, "login_arguments.output_mismatch");
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                if let Some(status) = session.try_wait()? {
                    ensure!(status.exit_code() == 7, "login_arguments.exit_code");
                    break;
                }
                ensure!(Instant::now() < deadline, "login_arguments.exit_deadline");
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
            .map_err(|_| anyhow::anyhow!("login_arguments.reader_panicked"))?;
        result?;
    }
    let command = prepare_local_login_command(
        pulse_pty::PtyCommand::new(directory.join("missing-shell")),
        &context,
        Some(ShellKind::Bash),
    )?;
    ensure!(
        PtySession::spawn(&command, pulse_core::TermSize::new(80, 24)?).is_err(),
        "login_arguments.program_fallback"
    );
    let report = receipt();
    validate(&report)?;
    std::fs::write(
        super::root()?.join("target/probes/login-arguments.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
#[cfg(not(unix))]
pub fn worker() -> Result<()> {
    bail!("login_arguments.requires_unix")
}
#[cfg(any(unix, test))]
fn receipt() -> Value {
    json!({"schemaVersion":2,"method":"arguments","platform":std::env::consts::OS,"shell":"bash","launchCount":3,"argumentCounts":[6,if cfg!(target_os = "macos") {7} else {6},7],"platformDefaultChecked":true,"platformDefaultLogin":cfg!(target_os = "macos"),"ordinaryNonLogin":true,"parameterLogin":true,"selectedProgramPreserved":true,"literalArgumentsPreserved":true,"scriptArgv0Preserved":true,"nativeTerminal":true,"startupFilesDisabled":true,"missingProgramRejected":true,"outputEof":true,"readerJoined":true,"directChildrenReaped":true,"exitCode":7,"outputBytesPerLaunch":OUTPUT.len(),"loginArgv0Rewritten":false})
}
pub fn run() -> Result<()> {
    ensure!(cfg!(unix), "login_arguments.requires_unix");
    let report = super::root()?.join("target/probes/login-arguments.json");
    std::fs::create_dir_all(report.parent().context("login_arguments.report_parent")?)?;
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("probe-login-arguments-worker")
        .stdin(std::process::Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "login_arguments.worker_failed");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&report);
            bail!("login_arguments.worker_deadline");
        }
        thread::sleep(Duration::from_millis(20));
    }
    validate(&serde_json::from_slice(&std::fs::read(&report)?)?)?;
    println!(
        "PASS Unix bash login arguments: native PTY login state, literal argv, no fallback and cleanup"
    );
    Ok(())
}
fn validate(r: &Value) -> Result<()> {
    for key in [
        "platformDefaultChecked",
        "ordinaryNonLogin",
        "parameterLogin",
        "selectedProgramPreserved",
        "literalArgumentsPreserved",
        "scriptArgv0Preserved",
        "nativeTerminal",
        "startupFilesDisabled",
        "missingProgramRejected",
        "outputEof",
        "readerJoined",
        "directChildrenReaped",
    ] {
        ensure!(
            r[key].as_bool() == Some(true),
            "login_arguments.invalid_receipt"
        );
    }
    ensure!(
        cfg!(unix)
            && r["schemaVersion"] == 2
            && r["method"] == "arguments"
            && r["platform"] == std::env::consts::OS
            && r["shell"] == "bash"
            && r["launchCount"] == 3
            && r["argumentCounts"] == json!([6, if cfg!(target_os = "macos") { 7 } else { 6 }, 7])
            && r["platformDefaultLogin"].as_bool() == Some(cfg!(target_os = "macos"))
            && r["exitCode"] == 7
            && r["outputBytesPerLaunch"] == OUTPUT.len()
            && r["loginArgv0Rewritten"].as_bool() == Some(false),
        "login_arguments.invalid_report"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argument_receipt_cannot_substitute_argv0_or_omit_native_evidence() {
        let report = receipt();
        assert_eq!(validate(&report).is_ok(), cfg!(unix));
        for key in [
            "platformDefaultChecked",
            "ordinaryNonLogin",
            "parameterLogin",
            "selectedProgramPreserved",
            "literalArgumentsPreserved",
            "scriptArgv0Preserved",
            "nativeTerminal",
            "startupFilesDisabled",
            "missingProgramRejected",
            "outputEof",
            "readerJoined",
            "directChildrenReaped",
        ] {
            for bad in [Value::Null, json!(false), json!("true"), json!(1)] {
                let mut wrong = report.clone();
                wrong[key] = bad;
                assert!(validate(&wrong).is_err());
            }
        }
        for (key, bad) in [
            ("method", json!("argv0")),
            ("loginArgv0Rewritten", json!(true)),
            ("launchCount", json!(1)),
            ("launchCount", json!(2)),
            ("platformDefaultLogin", json!("true")),
            ("platformDefaultLogin", Value::Null),
            ("argumentCounts", json!([6, 6])),
            ("schemaVersion", json!(1)),
            ("platformDefaultLogin", json!(!cfg!(target_os = "macos"))),
            ("platformDefaultChecked", json!(false)),
            ("platform", json!("wrong")),
            ("shell", json!("zsh")),
            ("exitCode", json!(0)),
            ("outputBytesPerLaunch", json!(4097)),
        ] {
            let mut wrong = report.clone();
            wrong[key] = bad;
            assert!(validate(&wrong).is_err());
        }
        assert!(validate(&json!({"completed":true})).is_err());
    }
}
