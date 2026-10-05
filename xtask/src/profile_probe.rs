use anyhow::{Context, Result, bail, ensure};
#[cfg(unix)]
use pulse_pty::profiles::{AutoProfileCatalog, ShellKind, discover_unix_shells};
use serde_json::{Value, json};
use std::io::{IsTerminal, Write};
use std::thread;
use std::time::{Duration, Instant};
const LITERAL: &str = "literal ; $(not-a-command) 中文🌍";
const OUTPUT: &str = "PROFILE_OK 中文🌍\r\n";
const OUTPUT_BUDGET: usize = 4096;

pub fn helper() -> Result<()> {
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(3));
        std::process::exit(99);
    });
    ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "profile.helper_terminal"
    );
    ensure!(
        std::env::args().nth(2).as_deref() == Some(LITERAL),
        "profile.helper_argv"
    );
    ensure!(
        std::env::var("PULSE_PROFILE_PROBE").ok().as_deref() == Some("literal-env"),
        "profile.helper_env"
    );
    ensure!(
        std::env::current_dir()?.canonicalize()?
            == super::root()?.join("target/probes").canonicalize()?,
        "profile.helper_cwd"
    );
    std::io::stdout().write_all("PROFILE_OK 中文🌍\n".as_bytes())?;
    std::io::stdout().flush()?;
    std::process::exit(7);
}
#[cfg(unix)]
pub fn worker() -> Result<()> {
    use pulse_pty::PtySession;
    use std::io::Read;
    let candidates = discover_unix_shells(std::ffi::OsStr::new("/bin:/usr/bin"), None)?;
    let mut catalog = AutoProfileCatalog::default();
    catalog.reconcile(&candidates)?;
    let sh = catalog
        .profile(ShellKind::Sh)
        .context("profile.sh_unavailable")?;
    let mut command = sh.command()?;
    command.args = vec![
        "-c".into(),
        "exec \"$@\"".into(),
        "pulseterm-profile-probe".into(),
        std::env::current_exe()?.into_os_string(),
        "profile-probe-helper".into(),
        LITERAL.into(),
    ];
    command.cwd = Some(super::root()?.join("target/probes"));
    command.env = vec![
        ("PULSE_PROFILE_PROBE".into(), "literal-env".into()),
        ("ENV".into(), "/dev/null".into()),
        ("BASH_ENV".into(), "/dev/null".into()),
    ];
    let mut session = PtySession::spawn(&command, pulse_core::TermSize::new(80, 24)?)
        .map_err(|_| anyhow::anyhow!("profile.pty_spawn"))?;
    let mut reader = session.take_reader()?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let pump = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = reader
            .by_ref()
            .take((OUTPUT_BUDGET + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let result = (|| -> Result<()> {
        let bytes = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| anyhow::anyhow!("profile.output_deadline"))?
            .map_err(|_| anyhow::anyhow!("profile.output_io"))?;
        ensure!(
            bytes.len() <= OUTPUT_BUDGET && bytes == OUTPUT.as_bytes(),
            "profile.output_mismatch"
        );
        let deadline = Instant::now() + Duration::from_secs(1);
        let code = loop {
            if let Some(status) = session.try_wait()? {
                break status.exit_code();
            }
            ensure!(Instant::now() < deadline, "profile.exit_deadline");
            thread::sleep(Duration::from_millis(10));
        };
        ensure!(code == 7, "profile.exit_code");
        Ok(())
    })();
    if result.is_err() {
        let _ = session.terminate();
    }
    drop(rx);
    drop(session);
    pump.join()
        .map_err(|_| anyhow::anyhow!("profile.reader_panicked"))?;
    result?;
    catalog.set_hidden(ShellKind::Sh, true)?;
    catalog.reconcile(&candidates)?;
    ensure!(
        catalog
            .profile(ShellKind::Sh)
            .is_some_and(|p| p.is_hidden()),
        "profile.hidden_reset"
    );
    catalog.remove(ShellKind::Sh)?;
    catalog.reconcile(&candidates)?;
    ensure!(
        catalog.profile(ShellKind::Sh).is_none(),
        "profile.deleted_reappeared"
    );
    let mut restored = AutoProfileCatalog::new(catalog.deleted());
    restored.reconcile(&candidates)?;
    ensure!(
        restored.profile(ShellKind::Sh).is_none(),
        "profile.tombstone_restore"
    );
    let report = json!({"schemaVersion":1,"platform":std::env::consts::OS,"completed":true,"metadataDiscovery":true,"nativeTerminal":true,"literalArgvCwdEnv":true,"outputEof":true,"readerJoined":true,"directChildReaped":true,"hiddenPreserved":true,"deletedSuppressed":true,"restoredSuppression":true,"shellCount":candidates.len(),"exitCode":7,"outputBytes":OUTPUT.len(),"outputBudgetBytes":OUTPUT_BUDGET});
    validate(&report)?;
    std::fs::write(
        super::root()?.join("target/probes/profiles.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
#[cfg(not(unix))]
pub fn worker() -> Result<()> {
    bail!("profile.probe_requires_unix")
}
pub fn run() -> Result<()> {
    ensure!(cfg!(unix), "profile.probe_requires_unix");
    let report = super::root()?.join("target/probes/profiles.json");
    std::fs::create_dir_all(report.parent().context("profile.report_parent")?)?;
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("probe-profiles-worker")
        .stdin(std::process::Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                let _ = std::fs::remove_file(&report);
                bail!("profile.worker_failed");
            }
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&report);
            bail!("profile.worker_deadline");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let value: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    validate(&value)?;
    println!("PASS Unix profile discovery, literal native PTY launch and deletion suppression");
    Ok(())
}
fn validate(r: &Value) -> Result<()> {
    for flag in [
        "completed",
        "metadataDiscovery",
        "nativeTerminal",
        "literalArgvCwdEnv",
        "outputEof",
        "readerJoined",
        "directChildReaped",
        "hiddenPreserved",
        "deletedSuppressed",
        "restoredSuppression",
    ] {
        ensure!(r[flag].as_bool() == Some(true), "profile.invalid_receipt");
    }
    ensure!(
        r["schemaVersion"] == 1
            && r["platform"] == std::env::consts::OS
            && (cfg!(unix))
            && r["exitCode"] == 7
            && r["outputBudgetBytes"] == OUTPUT_BUDGET
            && r["outputBytes"] == OUTPUT.len()
            && r["shellCount"]
                .as_u64()
                .is_some_and(|n| (1..=5).contains(&n)),
        "profile.invalid_report"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_rejects_missing_truthy_or_unbounded_receipts() {
        let report = json!({"schemaVersion":1,"platform":std::env::consts::OS,"completed":true,"metadataDiscovery":true,"nativeTerminal":true,"literalArgvCwdEnv":true,"outputEof":true,"readerJoined":true,"directChildReaped":true,"hiddenPreserved":true,"deletedSuppressed":true,"restoredSuppression":true,"shellCount":3,"exitCode":7,"outputBytes":OUTPUT.len(),"outputBudgetBytes":OUTPUT_BUDGET});
        assert_eq!(validate(&report).is_ok(), cfg!(unix));
        assert!(validate(&json!({"completed":true})).is_err());
        for key in [
            "metadataDiscovery",
            "nativeTerminal",
            "literalArgvCwdEnv",
            "outputEof",
            "readerJoined",
            "directChildReaped",
            "hiddenPreserved",
            "deletedSuppressed",
            "restoredSuppression",
        ] {
            for bad in [Value::Null, json!(false), json!("true"), json!(1)] {
                let mut wrong = report.clone();
                wrong[key] = bad;
                assert!(validate(&wrong).is_err());
            }
        }
        for (key, bad) in [
            ("exitCode", json!(0)),
            ("outputBytes", json!(4097)),
            ("shellCount", json!(0)),
            ("shellCount", json!(6)),
            ("platform", json!("wrong")),
            ("schemaVersion", json!(2)),
        ] {
            let mut wrong = report.clone();
            wrong[key] = bad;
            assert!(validate(&wrong).is_err());
        }
    }
}
