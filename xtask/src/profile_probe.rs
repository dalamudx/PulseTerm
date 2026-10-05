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
const PANE: &str = "BACKEND_PROFILE_PANE";
const SOCKET: &str = "owned-profile-socket";

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
    for (key, value) in [
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("TERM_PROGRAM", "PulseTerm"),
        ("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")),
    ] {
        ensure!(
            std::env::var(key).ok().as_deref() == Some(value),
            "profile.helper_terminal_defaults"
        );
    }
    match std::env::args().nth(3).as_deref() {
        Some("bound") => {
            ensure!(
                std::env::var("PULSE_PANE_ID").ok().as_deref() == Some(PANE)
                    && std::env::var("PULSE_SOCKET").ok().as_deref() == Some(SOCKET),
                "profile.helper_backend_cli"
            );
        }
        Some("unbound") => {
            ensure!(
                std::env::var_os("PULSE_PANE_ID").is_none()
                    && std::env::var_os("PULSE_SOCKET").is_none(),
                "profile.helper_stale_cli"
            );
        }
        _ => bail!("profile.helper_mode"),
    }

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
    ensure!(
        std::env::var("PULSE_PANE_ID").ok().as_deref() == Some("stale-from-launcher")
            && std::env::var("PULSE_SOCKET").ok().as_deref() == Some("stale-socket"),
        "profile.missing_stale_fixture"
    );
    for bound in [false, true] {
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
            if bound { "bound" } else { "unbound" }.into(),
        ];
        command.cwd = Some(super::root()?.join("target/probes"));
        command.env = vec![
            ("PULSE_PROFILE_PROBE".into(), "literal-env".into()),
            ("ENV".into(), "/dev/null".into()),
            ("BASH_ENV".into(), "/dev/null".into()),
        ];
        let context = pulse_pty::launch::LocalSessionEnvironment {
            application_version: env!("CARGO_PKG_VERSION").into(),
            pane: bound.then(|| pulse_pty::launch::PaneEnvironment {
                pane_id: PANE.into(),
                socket: SOCKET.into(),
            }),
        };
        let command =
            pulse_pty::launch::prepare_local_command(command, &context, Some(ShellKind::Sh))?;
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
    }
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
    let report = json!({"schemaVersion":2,"platform":std::env::consts::OS,"completed":true,"metadataDiscovery":true,"nativeTerminal":true,"literalArgvCwdEnv":true,"outputEof":true,"readerJoined":true,"directChildReaped":true,"hiddenPreserved":true,"deletedSuppressed":true,"restoredSuppression":true,"terminalDefaults":true,"staleCliCleared":true,"backendCliPair":true,"launchCount":2,"shellCount":candidates.len(),"exitCode":7,"outputBytes":OUTPUT.len(),"outputBudgetBytes":OUTPUT_BUDGET});
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
        .env("PULSE_PANE_ID", "stale-from-launcher")
        .env("PULSE_SOCKET", "stale-socket")
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
        "terminalDefaults",
        "staleCliCleared",
        "backendCliPair",
    ] {
        ensure!(r[flag].as_bool() == Some(true), "profile.invalid_receipt");
    }
    ensure!(
        r["schemaVersion"] == 2
            && r["launchCount"] == 2
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
        let report = json!({"schemaVersion":2,"platform":std::env::consts::OS,"completed":true,"metadataDiscovery":true,"nativeTerminal":true,"literalArgvCwdEnv":true,"outputEof":true,"readerJoined":true,"directChildReaped":true,"hiddenPreserved":true,"deletedSuppressed":true,"restoredSuppression":true,"terminalDefaults":true,"staleCliCleared":true,"backendCliPair":true,"launchCount":2,"shellCount":3,"exitCode":7,"outputBytes":OUTPUT.len(),"outputBudgetBytes":OUTPUT_BUDGET});
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
            "terminalDefaults",
            "staleCliCleared",
            "backendCliPair",
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
            ("schemaVersion", json!(1)),
            ("launchCount", json!(1)),
        ] {
            let mut wrong = report.clone();
            wrong[key] = bad;
            assert!(validate(&wrong).is_err());
        }
    }
}
