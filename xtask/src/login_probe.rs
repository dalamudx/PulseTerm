use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::io::{IsTerminal, Write};
use std::thread;
use std::time::{Duration, Instant};
const NAME: &str = "pulseterm-login-probe";
const OUTPUT: &[u8] = b"LOGIN_ARGV0_OK\r\n";
/// Finite own binary mode. It is not a shell and reads no shell startup files.
pub fn helper() -> Result<()> {
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(3));
        std::process::exit(99);
    });
    ensure!(
        std::env::args().next().as_deref() == Some("-pulseterm-login-probe")
            && std::env::args().count() == 1,
        "login.argv0"
    );
    ensure!(
        std::env::var("PULSE_LOGIN_PROBE").ok().as_deref() == Some("owned-fixed-fixture"),
        "login.fixture"
    );
    ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "login.terminal"
    );
    std::io::stdout().write_all(b"LOGIN_ARGV0_OK\n")?;
    std::io::stdout().flush()?;
    std::process::exit(7)
}
#[cfg(unix)]
pub fn worker() -> Result<()> {
    use std::io::Read;
    use std::os::unix::fs::DirBuilderExt;
    let directory = super::root()?
        .join("target/probes")
        .join(format!("login-owned-{}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let program = directory.join(NAME);
    std::fs::hard_link(std::env::current_exe()?, &program)?;
    let mut command = portable_pty::CommandBuilder::new_default_prog();
    command.env("SHELL", &program);
    command.env("PULSE_LOGIN_PROBE", "owned-fixed-fixture");
    command.cwd(&directory);
    ensure!(
        command.is_default_prog() && command.get_argv().is_empty(),
        "login.default_builder"
    );
    ensure!(
        command.get_shell() == program.to_str().context("login.fixture_encoding")?,
        "login.program_selection"
    );
    let pair = portable_pty::native_pty_system().openpty(portable_pty::PtySize {
        cols: 80,
        rows: 24,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let reader = pair.master.try_clone_reader()?;
    let child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    struct ChildGuard(Box<dyn portable_pty::Child + Send + Sync>);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = ChildGuard(child);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let pump = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = reader.take(4097).read_to_end(&mut bytes).map(|_| bytes);
        let _ = tx.send(result);
    });
    let result = (|| -> Result<()> {
        let bytes = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| anyhow::anyhow!("login.output_deadline"))??;
        ensure!(bytes == OUTPUT, "login.output_mismatch");
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(status) = child.0.try_wait()? {
                ensure!(status.exit_code() == 7, "login.exit_code");
                break;
            }
            ensure!(Instant::now() < deadline, "login.exit_deadline");
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    drop(child);
    drop(pair.master);
    drop(rx);
    pump.join()
        .map_err(|_| anyhow::anyhow!("login.reader_panicked"))?;
    result?;
    let missing = directory.join("missing-shell");
    let mut fallback = portable_pty::CommandBuilder::new_default_prog();
    fallback.env("SHELL", &missing);
    ensure!(
        fallback.get_shell() != missing.to_str().context("login.fixture_encoding")?,
        "login.missing_not_fallback"
    );
    let report = json!({"schemaVersion":1,"platform":std::env::consts::OS,"selectedProgram":true,"loginArgv0":true,"argumentCount":0,"nativeTerminal":true,"exitCode":7,"outputEof":true,"readerJoined":true,"directChildReaped":true,"missingProgramFallbackObserved":true,"outputBytes":OUTPUT.len(),"customArgumentsSupported":false});
    validate(&report)?;
    std::fs::write(
        super::root()?.join("target/probes/login.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
#[cfg(not(unix))]
pub fn worker() -> Result<()> {
    bail!("login.probe_requires_unix")
}
pub fn run() -> Result<()> {
    ensure!(cfg!(unix), "login.probe_requires_unix");
    let report = super::root()?.join("target/probes/login.json");
    std::fs::create_dir_all(report.parent().context("login.report_parent")?)?;
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("probe-login-worker")
        .stdin(std::process::Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "login.worker_failed");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&report);
            bail!("login.worker_deadline");
        }
        thread::sleep(Duration::from_millis(20));
    }
    validate(&serde_json::from_slice(&std::fs::read(&report)?)?)?;
    println!(
        "PASS Unix public login builder: selected executable and exact login argv0; fallback limitation observed"
    );
    Ok(())
}
fn validate(r: &Value) -> Result<()> {
    for key in [
        "selectedProgram",
        "loginArgv0",
        "nativeTerminal",
        "outputEof",
        "readerJoined",
        "directChildReaped",
        "missingProgramFallbackObserved",
    ] {
        ensure!(r[key].as_bool() == Some(true), "login.invalid_receipt");
    }
    ensure!(
        r["schemaVersion"] == 1
            && r["platform"] == std::env::consts::OS
            && cfg!(unix)
            && r["argumentCount"] == 0
            && r["exitCode"] == 7
            && r["outputBytes"] == OUTPUT.len()
            && r["customArgumentsSupported"].as_bool() == Some(false),
        "login.invalid_report"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[should_panic(expected = "attempted to add args to a default_prog builder")]
    fn public_default_login_builder_cannot_accept_profile_arguments() {
        portable_pty::CommandBuilder::new_default_prog().arg("-i");
    }
    #[test]
    fn report_cannot_claim_full_custom_login_or_miss_native_receipts() {
        let report = json!({"schemaVersion":1,"platform":std::env::consts::OS,"selectedProgram":true,"loginArgv0":true,"argumentCount":0,"nativeTerminal":true,"exitCode":7,"outputEof":true,"readerJoined":true,"directChildReaped":true,"missingProgramFallbackObserved":true,"outputBytes":OUTPUT.len(),"customArgumentsSupported":false});
        assert_eq!(validate(&report).is_ok(), cfg!(unix));
        for key in [
            "selectedProgram",
            "loginArgv0",
            "nativeTerminal",
            "outputEof",
            "readerJoined",
            "directChildReaped",
            "missingProgramFallbackObserved",
        ] {
            for bad in [Value::Null, json!(false), json!("true"), json!(1)] {
                let mut wrong = report.clone();
                wrong[key] = bad;
                assert!(validate(&wrong).is_err());
            }
        }
        for (key, bad) in [
            ("customArgumentsSupported", json!(true)),
            ("argumentCount", json!(1)),
            ("exitCode", json!(0)),
            ("platform", json!("wrong")),
            ("schemaVersion", json!(2)),
        ] {
            let mut wrong = report.clone();
            wrong[key] = bad;
            assert!(validate(&wrong).is_err());
        }
        assert!(validate(&json!({"completed":true})).is_err());
    }
}
