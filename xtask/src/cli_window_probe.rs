//! Actual native GUI receipt timing; not production CLI/cold-start or compositor evidence.
#[cfg(test)]
#[path = "cli_window_probe_tests.rs"]
mod tests;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub fn run() -> Result<()> {
    ensure!(
        cfg!(target_os = "linux"),
        "CLI native timing runner currently requires Linux"
    );
    let root = super::root()?;
    let path = root.join("target/probes/cli-window.json");
    std::fs::create_dir_all(root.join("target/probes"))?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let executable = root.join("target/debug/pulseterm-desktop");
    let headless = cfg!(target_os = "linux")
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none();
    let mut command = if headless {
        let mut c = Command::new("timeout");
        c.args([
            "--kill-after=5s",
            "40s",
            "xvfb-run",
            "-a",
            "-s",
            "-screen 0 1280x800x24",
            "dbus-run-session",
            "--",
        ])
        .arg(&executable);
        c
    } else {
        let mut c = Command::new("timeout");
        c.args(["--kill-after=5s", "40s"]).arg(&executable);
        c
    };
    let mut child = command
        .arg("--probe-cli-window")
        .arg("--report")
        .arg(&path)
        .stdin(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(50);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("CLI GUI process exceeded finite deadline");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let report: Value =
        serde_json::from_slice(&std::fs::read(&path).context("native CLI report absent")?)?;
    validate(&report)?;
    ensure!(status.success(), "native CLI failed");
    for row in report["samples"].as_array().context("samples")? {
        println!(
            "CLI server→native visible {:.2}ms; page-load {:.2}ms; client request→native registration receipt {:.2}ms",
            row["native"]["native"]["serverToNativeUs"]
                .as_u64()
                .context("native timestamp")? as f64
                / 1000.,
            row["native"]["pageLoadedUs"]
                .as_u64()
                .context("loaded timestamp")? as f64
                / 1000.,
            row["client"]["requestThroughNativeReceiptUs"]
                .as_u64()
                .context("client timestamp")? as f64
                / 1000.
        );
    }
    println!("PASS fixed authenticated CLI created/loaded/destroyed native windows");
    Ok(())
}
fn validate(r: &Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1
            && r["completed"] == true
            && r["mainPageReadyBeforeClient"] == true
            && r["badTokenCreatedNoWindow"] == true
            && r["clientsListenerWindowsAndDirectoryCleaned"] == true,
        "CLI native completion evidence"
    );
    let rows = r["samples"]
        .as_array()
        .context("CLI native samples missing")?;
    ensure!(rows.len() == 2, "two real window receipts required");
    for row in rows {
        ensure!(
            row["native"]["native"]["nativeVisibleRegistered"] == true
                && row["native"]["nativeDestroyed"] == true
                && row["client"]["nativeReceiptMatched"] == true,
            "actual native/window ownership receipts"
        );
        let built = row["native"]["native"]["serverToNativeUs"]
            .as_u64()
            .context("native timestamp")?;
        let loaded = row["native"]["pageLoadedUs"]
            .as_u64()
            .context("load timestamp")?;
        let receipt = row["client"]["requestThroughNativeReceiptUs"]
            .as_u64()
            .context("request timestamp")?;
        ensure!(
            built > 0
                && built <= 3_000_000
                && loaded >= built
                && loaded <= 5_000_000
                && receipt >= built
                && receipt <= 10_000_000,
            "native timestamp ordering/budget"
        );
    }
    ensure!(
        r["limits"].as_array().is_some_and(|v| !v.is_empty()),
        "CLI native limitations"
    );
    Ok(())
}
