use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::Value;

pub fn run() -> Result<()> {
    run_mode(false, false, false, false)
}
pub fn run_window() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("probe-window currently requires isolated Linux X11/Openbox");
    }
    run_mode(true, false, false, false)
}
pub fn run_window_external() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("external window probe requires isolated X11/Openbox");
    }
    run_mode(true, true, false, false)
}
pub fn run_v2() -> Result<()> {
    run_mode(false, false, true, false)
}
pub fn run_encoding() -> Result<()> {
    run_mode(false, false, true, true)
}
fn run_mode(minimize: bool, external: bool, dual: bool, encoding: bool) -> Result<()> {
    let root = super::root()?;
    let report = root.join(if encoding {
        "target/probes/encoding-window.json"
    } else if dual {
        "target/probes/channel-v2.json"
    } else if external {
        "target/probes/window-external.json"
    } else if minimize {
        "target/probes/window-minimize.json"
    } else {
        "target/probes/channel.json"
    });
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
        && (minimize
            || (std::env::var_os("DISPLAY").is_none()
                && std::env::var_os("WAYLAND_DISPLAY").is_none()));
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
        if minimize {
            command
                .arg("sh")
                .arg(root.join("scripts/probe-window-x11.sh"))
                .arg(root.join("target/probes/window-manager.log"));
        }
        if external {
            command
                .arg("sh")
                .arg(root.join("scripts/probe-window-external-x11.sh"))
                .arg(root.join("target/probes/window-external-actions.log"));
        }
        command.arg(&executable);
        command
    } else {
        Command::new(&executable)
    };
    command
        .args([
            if encoding {
                "--probe-encoding"
            } else if dual {
                "--probe-v2"
            } else if external {
                "--probe-window-external"
            } else if minimize {
                "--probe-window"
            } else {
                "--probe"
            },
            "--report",
        ])
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
    validate(&report, minimize, external, dual)?;
    let actual: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    if encoding {
        validate_encoding(&actual)?;
    } else if actual["encodingRuns"] != 0 || !actual["encoding"].is_null() {
        bail!("unexpected GBK evidence outside explicit encoding mode");
    }
    if external {
        let actions = root.join("target/probes/window-external-actions.log");
        if std::fs::metadata(&actions)?.len() > 128 {
            bail!("external action evidence too large");
        }
        validate_external_actions(&std::fs::read_to_string(actions)?)?;
    }
    if !status.success() {
        bail!("WebView probe process failed: {status}");
    }
    println!(
        "Native WebView data integrity, bounded credit and cancellation passed: {}",
        report.display()
    );
    Ok(())
}

fn validate_external_actions(text: &str) -> Result<()> {
    let rows: Vec<_> = text.lines().collect();
    if text.len() > 128 || rows.len() != 4 {
        bail!("external WM evidence must contain two complete action pairs");
    }
    let mut previous = 0;
    for pair in rows.chunks_exact(2) {
        let generation = pair[0]
            .strip_prefix("minimize:")
            .context("external minimize receipt missing")?;
        let numeric = generation
            .parse::<u64>()
            .context("external generation invalid")?;
        if numeric <= previous
            || numeric.to_string() != generation
            || pair[1] != format!("restore:{generation}")
        {
            bail!("external WM action ordering or generation mismatch");
        }
        previous = numeric;
    }
    Ok(())
}

fn validate_encoding(report: &Value) -> Result<()> {
    let e = &report["encoding"];
    if report["encodingRuns"] != 2
        || e["sourceBytes"] != "89"
        || e["utf8Bytes"] != "91"
        || e["replacements"] != 1
        || e["domRowsChecked"] != 12
        || [
            "reverseInputChecked",
            "sourceOnlyMarker",
            "historyChecked",
            "callbackAckChecked",
        ]
        .iter()
        .any(|key| e[key] != true)
        || e["peakCreditBytes"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > 64)
        || e["frames"]
            .as_u64()
            .is_none_or(|n| !(4..=16384).contains(&n))
        || e["generation"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .is_none_or(|n| n == 0)
        || e["revision"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .is_none_or(|n| n == 0)
        || e["digest"].as_str().is_none_or(|s| {
            s.len() != 16
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        bail!("missing actual GBK actor/Channel/DOM evidence");
    }
    Ok(())
}

fn window_mode_matches(report: &Value, minimize: bool) -> bool {
    report["domSession"]["nativeMinimizeChecked"].as_bool() == Some(minimize)
        && report["session"]["nativeMinimizeChecked"].as_bool() == Some(false)
}
fn attachment_mode_matches(report: &Value, dual: bool) -> bool {
    let version = if dual { 2 } else { 1 };
    ["session", "domSession"]
        .iter()
        .all(|key| report[key]["attachmentVersion"] == version)
}
fn validate(path: &Path, minimize: bool, external: bool, dual: bool) -> Result<()> {
    let report: Value =
        serde_json::from_slice(&std::fs::read(path).context("WebView did not write a report")?)?;
    if report["schemaVersion"] != 4
        || report["domRuns"] != 2
        || report["sessionRuns"] != 2
        || report["typedEvents"] != true
        || report["completed"] != true
        || report["cancellation"] != true
        || !attachment_mode_matches(&report, dual)
    {
        bail!("native probe incomplete: {}", report["error"]);
    }
    for key in ["session", "domSession"] {
        let session = &report[key];
        if session["attachmentVersion"] != if dual { 2 } else { 1 }
            || session["generation"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .is_none_or(|n| n == 0)
            || session["nativeExit"] != 0
            || session["peerDenied"] != true
            || session["windowRevoked"] != true
            || session["sessionSurvived"] != true
            || session["idleChecked"] != true
            || session["markerChecked"] != true
            || session["repairChecked"] != true
            || session["inputChecked"] != true
            || session["frames"].as_u64().is_none_or(|n| n < 4)
            || session["peakCreditBytes"]
                .as_u64()
                .is_none_or(|n| n > 8192 + if dual { 28 } else { 20 } || n == 0)
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
    }
    let dom = &report["domEvidence"];
    if dom["readonly"] != true
        || dom["keyboardChecked"] != true
        || dom["compositionChecked"] != true
        || dom["pasteBlocked"] != true
        || dom["fitChecked"] != true
        || report["domSession"]["resizeCount"] != 3
        || report["domSession"]["pauseCount"] != 2
        || report["domSession"]["nativeWindowChecked"] != true
        || report["session"]["nativeWindowChecked"] != false
        || !window_mode_matches(&report, minimize)
        || dom["visibilityChecked"] != true
        || dom["nativeSignalChecked"] != true
        || dom["nativePauseChecked"] != true
        || dom["viewHostChecked"] != true
        || dom["viewPoolChecked"] != true
        || dom["activeParkChecked"] != true
        || report["domSession"]["nativeOnlyChecked"] != true
        || report["session"]["nativeOnlyChecked"] != false
        || report["domSession"]["externalWmChecked"].as_bool() != Some(external)
        || report["session"]["externalWmChecked"] != false
        || report["session"]["resizeCount"] != 0
        || report["session"]["pauseCount"] != 0
        || dom["rowsChecked"] != 12
        || dom["renderEvents"].as_u64().is_none_or(|n| n == 0)
        || dom["frameFences"].as_u64().is_none_or(|n| n == 0)
        || ["widthPx", "heightPx"].iter().any(|key| {
            dom[key]
                .as_f64()
                .is_none_or(|n| !n.is_finite() || !(1.0..=4096.0).contains(&n))
        })
    {
        bail!("missing visible DOM rendering evidence");
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoding_receipt_rejects_utf8_only_missing_render_or_unbounded_credit() {
        let mut r = serde_json::json!({"encodingRuns":2,"encoding":{"sourceBytes":"89","utf8Bytes":"91","generation":"2","revision":"20","digest":"9e1323ca657a4985","replacements":1,"frames":100,"peakCreditBytes":64,"domRowsChecked":12,"reverseInputChecked":true,"sourceOnlyMarker":true,"historyChecked":true,"callbackAckChecked":true}});
        assert!(validate_encoding(&r).is_ok());
        r["encoding"]["utf8Bytes"] = serde_json::json!("89");
        assert!(validate_encoding(&r).is_err());
        r["encoding"]["utf8Bytes"] = serde_json::json!("91");
        r["encoding"]["callbackAckChecked"] = serde_json::json!(false);
        assert!(validate_encoding(&r).is_err());
        r["encoding"]["callbackAckChecked"] = serde_json::json!(true);
        r["encoding"]["peakCreditBytes"] = serde_json::json!(65);
        assert!(validate_encoding(&r).is_err());
        assert!(validate_encoding(&serde_json::json!({})).is_err());
    }

    #[test]
    fn attachment_version_evidence_cannot_substitute_v1_for_v2_or_mix_runs() {
        for dual in [false, true] {
            let version = if dual { 2 } else { 1 };
            let r = serde_json::json!({"session":{"attachmentVersion":version},"domSession":{"attachmentVersion":version}});
            assert!(attachment_mode_matches(&r, dual));
            assert!(!attachment_mode_matches(&r, !dual));
        }
        assert!(!attachment_mode_matches(&serde_json::json!({}), false));
        assert!(!attachment_mode_matches(
            &serde_json::json!({"session":{"attachmentVersion":2},"domSession":{"attachmentVersion":1}}),
            true
        ));
    }

    #[test]
    fn external_evidence_requires_two_distinct_ordered_minimize_restore_pairs() {
        assert!(
            validate_external_actions("minimize:6\nrestore:6\nminimize:11\nrestore:11\n").is_ok()
        );
        for bad in [
            "",
            "minimize:6\nrestore:6",
            "minimize:6\nrestore:7\nminimize:11\nrestore:11",
            "minimize:6\nrestore:6\nminimize:6\nrestore:6",
            "restore:6\nminimize:6\nminimize:11\nrestore:11",
        ] {
            assert!(validate_external_actions(bad).is_err());
        }
    }
    #[test]
    fn external_evidence_rejects_noncanonical_or_unbounded_generation() {
        for bad in [
            "minimize:0\nrestore:0\nminimize:1\nrestore:1",
            "minimize:06\nrestore:06\nminimize:11\nrestore:11",
            "minimize:x\nrestore:x\nminimize:11\nrestore:11",
            "minimize:18446744073709551616\nrestore:18446744073709551616\nminimize:11\nrestore:11",
        ] {
            assert!(validate_external_actions(bad).is_err());
        }
        assert!(validate_external_actions(&"x".repeat(129)).is_err());
    }

    #[test]
    fn only_requested_native_mode_with_false_headless_claim_is_accepted() {
        for mode in [false, true] {
            let mut r = serde_json::json!({"domSession":{"nativeMinimizeChecked":mode},"session":{"nativeMinimizeChecked":false}});
            assert!(window_mode_matches(&r, mode));
            assert!(!window_mode_matches(&r, !mode));
            r["session"]["nativeMinimizeChecked"] = Value::Bool(true);
            assert!(!window_mode_matches(&r, mode));
        }
    }
    #[test]
    fn missing_or_non_boolean_window_receipts_fail_closed() {
        for invalid in [Value::Null, Value::String("true".into()), Value::from(1)] {
            let r = serde_json::json!({"domSession":{"nativeMinimizeChecked":invalid},"session":{"nativeMinimizeChecked":false}});
            assert!(!window_mode_matches(&r, false));
            assert!(!window_mode_matches(&r, true));
        }
        assert!(!window_mode_matches(&serde_json::json!({}), false));
    }
}
