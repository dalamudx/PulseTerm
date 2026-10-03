//! Repository checks and reproducible technical probes.
#![forbid(unsafe_code)]

mod channel_probe;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use pulse_core::TermSize;
use pulse_term::TerminalProbe;
use serde_json::{Value, json};

fn root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("workspace root unavailable")
}

fn main() -> Result<()> {
    std::env::set_current_dir(root()?)?;
    match std::env::args().nth(1).as_deref() {
        Some("check") => {
            run("cargo", &["fmt", "--all", "--", "--check"])?;
            check_deps()?;
            run(
                "cargo",
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "--",
                    "-D",
                    "warnings",
                ],
            )?;
        }
        Some("check-deps") => check_deps()?,
        Some("probe") => probe()?,
        Some("probe-channel") => channel_probe::run()?,
        _ => bail!("usage: cargo xtask <check|check-deps|probe|probe-channel>"),
    }
    Ok(())
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("run {program}"))?;
    if !status.success() {
        bail!("{program} {args:?} failed: {status}");
    }
    Ok(())
}

fn check_deps() -> Result<()> {
    let result = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .output()?;
    if !result.status.success() {
        bail!("cargo metadata failed");
    }
    let metadata: Value = serde_json::from_slice(&result.stdout)?;
    let packages = metadata["packages"]
        .as_array()
        .context("missing packages")?;
    for package in packages {
        let name = package["name"].as_str().context("missing package name")?;
        let dependencies = package["dependencies"]
            .as_array()
            .context("missing dependencies")?;
        for dependency in dependencies {
            let dep = dependency["name"]
                .as_str()
                .context("missing dependency name")?;
            if name.starts_with("pulse-") && (dep == "tauri" || dep.starts_with("tauri-")) {
                bail!("{name} must not depend on {dep}");
            }
            let allowed: &[&str] = match name {
                "pulse-core" => &[],
                "pulse-ipc" => &["pulse-core"],
                "pulse-term" => &["pulse-core", "pulse-ipc"],
                _ => continue,
            };
            if dep.starts_with("pulse-") && !allowed.contains(&dep) {
                bail!("forbidden dependency: {name} -> {dep}");
            }
            if name == "pulse-core"
                && ["tokio", "rusqlite", "reqwest", "portable-pty"].contains(&dep)
            {
                bail!("pulse-core cannot perform I/O: {dep}");
            }
        }
    }
    Ok(())
}

fn probe() -> Result<()> {
    let size = TermSize::new(32, 8)?;
    let fixtures = Path::new("tests/fixtures/terminal");
    let mut paths = std::fs::read_dir(fixtures)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    let mut cases = Vec::new();
    for path in paths {
        if path.extension().and_then(|ext| ext.to_str()) != Some("vt") {
            continue;
        }
        let input = std::fs::read_to_string(&path)?;
        let mut terminal = TerminalProbe::new(size, 128);
        // Deliberately split multibyte UTF-8 and escape sequences.
        for chunk in input.as_bytes().chunks(3) {
            terminal.feed(chunk);
        }
        let replay = terminal.screen_replay();
        let mut restored = TerminalProbe::new(size, 128);
        restored.feed(replay.as_bytes());
        let rust_replay_matches = terminal.screen() == restored.screen();
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .context("invalid fixture name")?;
        let exploratory = name.starts_with("explore-");
        if !exploratory && !rust_replay_matches {
            bail!("Rust visible replay mismatch: {name}");
        }
        cases.push(json!({"name": name, "input": input, "replay": replay, "expected": terminal.screen(), "replies": terminal.take_replies(), "rust_replay_matches": rust_replay_matches, "exploratory": exploratory}));
    }
    if cases.is_empty() {
        bail!("no terminal fixtures found");
    }
    let mut memory = TerminalProbe::new(TermSize::new(120, 40)?, 10_000);
    for _ in 0..100_000 {
        memory.feed(b"sample log line\r\n");
    }
    let report = json!({
        "schema_version": 1, "columns": size.columns(), "rows": size.rows(), "cases": cases,
        "memory": {"retained_history_lines": memory.history_lines(), "grid_cell_bytes_lower_bound": memory.grid_cell_bytes()},
        "limitations": ["Only visible cells and cursor are compared", "No inactive buffer, saved cursor, tab stop, pending wrap or parser-state restoration", "No cold history archive or process-tree memory measurement", "Headless xterm does not measure WebView rendering, IME or IPC throughput"]
    });
    std::fs::create_dir_all("target/probes")?;
    std::fs::write(
        "target/probes/terminal.json",
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
