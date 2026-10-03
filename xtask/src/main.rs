//! Repository checks and reproducible technical probes.
#![forbid(unsafe_code)]

mod attachment_probe;
mod channel_probe;
mod local_session_probe;
mod portable_pty_probe;
mod pty_probe;
mod query_probe;
mod snapshot_probe;
mod software_probe;
#[cfg(feature = "ssh-probe")]
mod ssh_jump_probe;
#[cfg(feature = "ssh-probe")]
mod ssh_probe;
#[cfg(feature = "ssh-probe")]
mod ssh_system_probe;
mod unicode_probe;
mod unicode_table;

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
    #[cfg(feature = "ssh-probe")]
    if std::env::args().nth(1).as_deref() == Some("ssh-system-helper") {
        return ssh_system_probe::helper();
    }
    if std::env::args().nth(1).as_deref() == Some("local-session-helper-detached") {
        return local_session_probe::detached_helper();
    }
    if let Some(mode) = std::env::args()
        .nth(1)
        .and_then(|arg| arg.strip_prefix("portable-pty-helper-").map(str::to_owned))
    {
        return portable_pty_probe::helper(&mode);
    }
    // PTY helpers preserve the cwd chosen by their parent.
    if let Some(mode) = std::env::args()
        .nth(1)
        .and_then(|arg| arg.strip_prefix("pty-helper-").map(str::to_owned))
    {
        return pty_probe::helper(&mode);
    }
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
        Some("probe-window") => channel_probe::run_window()?,
        Some("probe-window-external") => channel_probe::run_window_external()?,
        Some("probe-pty") => pty_probe::run()?,
        Some("probe-pty-portable") => portable_pty_probe::run()?,
        Some("probe-local-session") => local_session_probe::run()?,
        Some("probe-software") => software_probe::run()?,
        Some("probe-software-worker") => software_probe::worker()?,
        Some("probe-pty-portable-worker") => portable_pty_probe::worker()?,
        Some("probe-pty-worker") => pty_probe::worker()?,
        Some("unicode-table") => unicode_table::run(false)?,
        Some("unicode-table-check") => unicode_table::run(true)?,
        Some("probe-snapshot") => snapshot_probe::run()?,
        Some("probe-attachment") => attachment_probe::run()?,
        Some("probe-ssh-jump") => {
            #[cfg(feature = "ssh-probe")]
            ssh_jump_probe::run()?;
            #[cfg(not(feature = "ssh-probe"))]
            bail!("probe-ssh-jump requires --features ssh-probe");
        }
        Some("probe-ssh-system") => {
            #[cfg(feature = "ssh-probe")]
            ssh_system_probe::run()?;
            #[cfg(not(feature = "ssh-probe"))]
            bail!("probe-ssh-system requires --features ssh-probe");
        }
        Some("probe-ssh") => {
            #[cfg(feature = "ssh-probe")]
            ssh_probe::run()?;
            #[cfg(not(feature = "ssh-probe"))]
            bail!("probe-ssh requires --features ssh-probe");
        }
        _ => bail!("usage: cargo xtask <check|check-deps|probe|probe-channel|probe-pty>"),
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
                "pulse-pty" => &["pulse-core"],
                "pulse-app" => &["pulse-core", "pulse-pty", "pulse-term", "pulse-ipc"],
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
        terminal.finish_input();
        let replay = terminal.screen_replay();
        let mut restored = TerminalProbe::new(size, 128);
        restored.feed(replay.as_bytes());
        restored.finish_input();
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
        "queries": query_probe::run(size)?,
        "unicode": unicode_probe::run(size)?,
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
