use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::Value;

pub fn run() -> Result<()> {
    let root = super::root()?;
    let report = root.join("target/probes/channel.json");
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let executable = root.join(if cfg!(windows) {
        "target/debug/pulseterm-desktop.exe"
    } else {
        "target/debug/pulseterm-desktop"
    });
    if !executable.exists() {
        bail!("run mise run desktop-build first");
    }
    let headless = cfg!(target_os = "linux")
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none();
    let mut command = if headless {
        // GNU timeout supervises the complete xvfb/dbus/WebView process group.
        let mut command = Command::new("timeout");
        command.args(["--kill-after=5s", "180s", "xvfb-run"]);
        command.args([
            "-a",
            "-s",
            "-screen 0 1280x800x24",
            "dbus-run-session",
            "--",
        ]);
        command.arg(&executable);
        command
    } else {
        Command::new(&executable)
    };
    command
        .args(["--probe", "--report"])
        .arg(&report)
        .stdin(Stdio::null());
    // Only synthetic frames and fixed finite-helper output; keep the WebKit sandbox enabled.
    let mut child = command
        .spawn()
        .context("launch native WebView probe (headless Linux requires xvfb and dbus)")?;
    let deadline = Instant::now() + Duration::from_secs(if headless { 190 } else { 180 });
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            if cfg!(windows) {
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
            bail!("WebView probe exceeded 180 seconds; inspect native logs");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    validate(&report)?;
    if !status.success() {
        bail!("WebView probe process failed: {status}");
    }
    println!(
        "Native WebView data integrity, bounded credit and cancellation passed: {}",
        report.display()
    );
    Ok(())
}

fn validate(path: &Path) -> Result<()> {
    let report: Value =
        serde_json::from_slice(&std::fs::read(path).context("WebView did not write a report")?)?;
    if report["schemaVersion"] != 3
        || report["sessionRuns"] != 2
        || report["typedEvents"] != true
        || report["completed"] != true
        || report["cancellation"] != true
    {
        bail!("native probe incomplete: {}", report["error"]);
    }
    let session = &report["session"];
    if session["generation"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .is_none_or(|n| n == 0)
        || session["nativeExit"] != 0
        || session["peerDenied"] != true
        || session["windowRevoked"] != true
        || session["sessionSurvived"] != true
        || session["idleChecked"] != true
        || session["markerChecked"] != true
        || session["frames"].as_u64().is_none_or(|n| n < 4)
        || session["peakCreditBytes"]
            .as_u64()
            .is_none_or(|n| n > 8212 || n == 0)
        || session["parsedBytes"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .is_none_or(|n| n < 1024 * 1024)
        || session["digest"].as_str().is_none_or(|s| {
            s.len() != 16
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        bail!("missing native session, window authorization or EOF evidence");
    }
    let rows = report["measurements"]
        .as_array()
        .context("missing measurements")?;
    if rows.len() != 7 {
        bail!("expected 7 measurements, got {}", rows.len());
    }
    for row in rows {
        if row["verified"] != true {
            bail!("unverified measurement");
        }
        let count = row["config"]["frames"]
            .as_u64()
            .context("missing frame count")?;
        let bytes = row["config"]["chunkBytes"]
            .as_u64()
            .context("missing chunk size")?;
        let window = row["config"]["window"].as_u64().context("missing window")?;
        if row["backend"]["bytes"] != count * bytes
            || row["backend"]["frames"] != count
            || row["backend"]["peakFrames"]
                .as_u64()
                .context("missing frame peak")?
                > window
            || row["backend"]["peakBytes"]
                .as_u64()
                .context("missing byte peak")?
                > window * bytes
        {
            bail!("invalid integrity or backpressure evidence");
        }
        println!(
            "{} {}: {:.2} MB/s, delivery p95 {:.2} ms, heartbeat max {:.2} ms",
            row["transport"],
            row["label"],
            row["bytesPerSecond"].as_f64().unwrap_or(0.0) / 1e6,
            row["deliveryP95Ms"].as_f64().unwrap_or(0.0),
            row["heartbeatMaxMs"].as_f64().unwrap_or(0.0)
        );
    }
    Ok(())
}
