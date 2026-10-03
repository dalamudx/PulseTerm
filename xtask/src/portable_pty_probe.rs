use anyhow::{Context, Result, bail, ensure};
use pulse_core::TermSize;
use pulse_pty::{PtyCommand, PtySession};
use pulse_term::TerminalProbe;
use serde_json::json;
use std::io::{IsTerminal, Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
const LINE: &str = "literal ; $(input) 中";
const MAX_OUTPUT: usize = 128 * 1024;
/// Canonical line input deliberately avoids platform-specific raw-mode APIs.
pub fn helper(mode: &str) -> Result<()> {
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(15));
        std::process::exit(99);
    });
    ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "not a real native terminal"
    );
    ensure!(
        std::env::args().nth(2).as_deref() == Some("literal ; $(argv)"),
        "argv altered"
    );
    ensure!(
        std::env::var("PULSE_PTY_PORTABLE")? == "explicit-value",
        "env lost"
    );
    ensure!(
        std::env::current_dir()?.canonicalize()?
            == super::root()?.join("target/probes").canonicalize()?,
        "cwd lost"
    );
    match mode {
        "interactive" => {
            print!("\x1bcPORTABLE_READY\r\n");
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            ensure!(
                line.trim_end_matches(['\r', '\n']) == LINE,
                "input altered: {line:?}"
            );
            print!("\x1bcPORTABLE_OK 中\r\n");
            std::io::stdout().flush()?;
            std::process::exit(7);
        }
        "stream" => {
            print!("\x1bcPORTABLE_STREAM_READY");
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            ensure!(
                line.trim_end_matches(['\r', '\n']) == LINE,
                "stream input altered"
            );
            let mut output = std::io::stdout().lock();
            for _ in 0..2048 {
                output.write_all(&[b'X'; 1024])?;
            }
            output.write_all(b"\x1bcPORTABLE_STREAM_DONE")?;
            output.flush()?;
        }
        "hold" => {
            print!("\x1bcPORTABLE_HOLD\r\n");
            std::io::stdout().flush()?;
            thread::sleep(Duration::from_secs(12));
        }
        _ => bail!("unknown portable helper mode"),
    }
    Ok(())
}
type NativeProbe = (
    PtySession,
    mpsc::Receiver<std::io::Result<Vec<u8>>>,
    thread::JoinHandle<()>,
);
fn spawn(mode: &str) -> Result<NativeProbe> {
    let mut command = PtyCommand::new(std::env::current_exe()?);
    command.args = vec![
        format!("portable-pty-helper-{mode}").into(),
        "literal ; $(argv)".into(),
    ];
    command.cwd = Some(super::root()?.join("target/probes"));
    command
        .env
        .push(("PULSE_PTY_PORTABLE".into(), "explicit-value".into()));
    let mut session = PtySession::spawn(&command, TermSize::new(80, 24)?)?;
    let mut reader = session.take_reader()?;
    let (tx, rx) = mpsc::sync_channel(2);
    let thread = thread::spawn(move || {
        let mut buf = [0; 4096];
        loop {
            let value = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => Ok(buf[..n].to_vec()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => Err(e),
            };
            let failed = value.is_err();
            if tx.send(value).is_err() || failed {
                break;
            }
        }
    });
    Ok((session, rx, thread))
}
fn next(rx: &mpsc::Receiver<std::io::Result<Vec<u8>>>) -> Result<Option<Vec<u8>>> {
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(bytes) => Ok(Some(bytes?)),
        Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        Err(_) => bail!("native PTY stalled"),
    }
}
fn feed(
    session: &mut PtySession,
    terminal: &mut TerminalProbe,
    bytes: &[u8],
    total: &mut usize,
) -> Result<()> {
    *total += bytes.len();
    ensure!(*total <= MAX_OUTPUT, "PTY output quota exceeded");
    terminal.feed(bytes);
    for reply in terminal.take_replies() {
        session.write_input(reply.as_bytes())?;
    }
    Ok(())
}
fn ready(
    session: &mut PtySession,
    rx: &mpsc::Receiver<std::io::Result<Vec<u8>>>,
    term: &mut TerminalProbe,
    marker: &str,
    total: &mut usize,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        ensure!(Instant::now() < deadline, "ready deadline exceeded");
        let chunk = next(rx)?.context("EOF before ready")?;
        feed(session, term, &chunk, total)?;
        let text = term
            .screen()
            .cells
            .into_iter()
            .map(|row| row.into_iter().map(|c| c.text).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        if text.contains(marker) {
            return Ok(());
        }
    }
}
pub fn worker() -> Result<()> {
    std::fs::create_dir_all(super::root()?.join("target/probes"))?;
    let (mut session, rx, pump) = spawn("interactive")?;
    let mut terminal = TerminalProbe::new(TermSize::new(80, 24)?, 128);
    let mut total = 0;
    ready(
        &mut session,
        &rx,
        &mut terminal,
        "PORTABLE_READY",
        &mut total,
    )?;
    let resized = TermSize::new(91, 37)?;
    session.resize(resized)?;
    ensure!(session.dimensions()? == resized, "native resize lost");
    terminal.resize(resized);
    // CR commits canonical input on Windows; LF commits on Unix. Do not infer raw
    // bytes or Win32 keyboard protocol support from this cooked-line test.
    let ending = if cfg!(windows) { "\r" } else { "\n" };
    session.write_input(format!("{LINE}{ending}").as_bytes())?;
    while let Some(bytes) = next(&rx)? {
        feed(&mut session, &mut terminal, &bytes, &mut total)?;
    }
    terminal.finish_input();
    ensure!(session.wait()?.exit_code() == 7, "nonzero child exit lost");
    pump.join()
        .map_err(|_| anyhow::anyhow!("reader panicked"))?;
    let final_text = terminal
        .screen()
        .cells
        .into_iter()
        .map(|row| row.into_iter().map(|c| c.text).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    ensure!(
        final_text.contains("PORTABLE_OK 中"),
        "native Unicode/output repaint lost"
    );
    drop(session);
    let (mut held, rx, pump) = spawn("hold")?;
    let mut terminal = TerminalProbe::new(TermSize::new(80, 24)?, 0);
    let mut total = 0;
    ready(&mut held, &rx, &mut terminal, "PORTABLE_HOLD", &mut total)?;
    let start = Instant::now();
    let first = held.terminate()?;
    ensure!(
        held.terminate()?.exit_code() == first.exit_code(),
        "termination not idempotent"
    );
    drop(held);
    while next(&rx)?.is_some() {}
    pump.join()
        .map_err(|_| anyhow::anyhow!("reader panicked"))?;
    let cancellation = start.elapsed().as_millis();
    ensure!(cancellation < 2000, "direct-child teardown too slow");
    let report = json!({"schemaVersion":1,"platform":std::env::consts::OS,"nativeTerminal":true,"literalArgvCwdEnv":true,"canonicalInputUnicode":true,"nativeResize":true,"exitCode":7,"outputEof":true,"directChildTermination":true,"cancellationMs":cancellation,"queueChunks":2,"chunkBytes":4096,"limits":["Native dimension readback, not child-side get-console-size verification","Canonical input only; not raw keyboard/query delivery","No packaged conpty.dll or OpenConsole.exe; OS provider is tested","Direct child only; descendants and native GUI remain unverified"]});
    std::fs::write(
        super::root()?.join("target/probes/pty-portable.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
pub fn run() -> Result<()> {
    let report = super::root()?.join("target/probes/pty-portable.json");
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("probe-pty-portable-worker")
        .stdin(std::process::Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "portable PTY worker failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("portable PTY exceeded deadline; helpers expire independently");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    ensure!(
        value["platform"] == std::env::consts::OS
            && value["exitCode"] == 7
            && value["nativeTerminal"] == true,
        "invalid portable probe evidence"
    );
    println!(
        "PASS native PTY {}: input/Unicode, native resize, nonzero exit, EOF and direct-child teardown",
        std::env::consts::OS
    );
    Ok(())
}
