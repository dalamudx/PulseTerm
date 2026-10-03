use anyhow::{Context, Result, bail, ensure};
use pulse_core::TermSize;
use pulse_pty::{PtyCommand, PtyError, PtySession};
use pulse_term::TerminalProbe;
use serde_json::json;
use std::io::{IsTerminal, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CHUNK: usize = 4096;
const QUEUE: usize = 2;
const FLOOD: usize = 1024 * 1024;
const INPUT: &str = "input with ; $(literal)";

// Each test executable is finite even if its parent is killed. These helpers never
// spawn persistent descendants. Production shell trees need separate supervision.
pub fn helper(mode: &str) -> Result<()> {
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(10));
        std::process::exit(99);
    });
    ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "helper did not receive a real terminal"
    );
    ensure!(cfg!(unix), "raw-byte PTY probe is currently Unix-only");
    let status = Command::new("stty").args(["raw", "-echo"]).status()?;
    ensure!(status.success(), "stty raw failed");
    let mut output = std::io::stdout().lock();
    match mode {
        "interactive" => {
            ensure!(
                std::env::args().nth(2).as_deref() == Some("literal ; $(argv)"),
                "argv was changed"
            );
            ensure!(
                std::env::var("PULSE_PTY_TEST")? == "explicit-value",
                "env override lost"
            );
            ensure!(
                std::env::current_dir()?.canonicalize()?
                    == super::root()?.join("target/probes").canonicalize()?,
                "cwd override lost"
            );
            output.write_all(b"READY\r\n")?;
            output.flush()?;
            let mut input = std::io::stdin().lock();
            let mut line = vec![];
            for _ in 0..128 {
                let mut byte = [0];
                input.read_exact(&mut byte)?;
                if byte[0] == b'\n' {
                    break;
                }
                line.push(byte[0]);
            }
            ensure!(line == INPUT.as_bytes(), "input altered");
            let dimensions = Command::new("stty")
                .arg("size")
                .stdin(Stdio::inherit())
                .output()?;
            ensure!(dimensions.status.success(), "child size query failed");
            ensure!(
                String::from_utf8(dimensions.stdout)?.trim() == "37 91",
                "child did not see resize"
            );
            output.write_all("\x1bcPTY 中\x1b[6n".as_bytes())?;
            output.flush()?;
            let mut response = vec![];
            for _ in 0..32 {
                let mut byte = [0];
                input.read_exact(&mut byte)?;
                response.push(byte[0]);
                if byte[0] == b'R' {
                    break;
                }
            }
            ensure!(
                response == b"\x1b[1;7R",
                "query reply altered: {response:?}"
            );
            output.write_all(b"\r\nQUERY_OK\r\n")?;
            output.flush()?;
            std::process::exit(7);
        }
        "flood" => {
            output.write_all(b"FLOOD_READY\r\n")?;
            output.flush()?;
            let mut go = [0];
            std::io::stdin().read_exact(&mut go)?;
            ensure!(go == [b'g'], "missing flood start");
            let mut buffer = [0; CHUNK];
            for offset in (0..FLOOD).step_by(CHUNK) {
                for (index, byte) in buffer.iter_mut().enumerate() {
                    *byte = b'A' + ((offset + index) % 26) as u8;
                }
                output.write_all(&buffer)?;
            }
            output.write_all(b"\r\nFLOOD_DONE\r\n")?;
            output.flush()?;
        }
        "cancel" => {
            output.write_all(b"CANCEL_READY\r\n")?;
            output.flush()?;
            loop {
                thread::sleep(Duration::from_millis(10));
            }
        }
        _ => bail!("unknown PTY helper mode"),
    }
    Ok(())
}

// Bounded pump used only by this probe. Producer retains at most one additional
// fixed-size chunk while blocked on send; OS buffers are not counted as our heap.
struct Pump {
    receiver: Option<Receiver<std::io::Result<Vec<u8>>>>,
    thread: Option<JoinHandle<()>>,
    send_attempts: Arc<AtomicUsize>,
}
impl Pump {
    fn new(mut reader: Box<dyn Read + Send>) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(QUEUE);
        let send_attempts = Arc::new(AtomicUsize::new(0));
        let attempts = send_attempts.clone();
        let thread = thread::Builder::new()
            .name("pty-probe-read".into())
            .spawn(move || {
                let mut buffer = [0; CHUNK];
                loop {
                    let value = match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(length) => Ok(buffer[..length].to_vec()),
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => Err(error),
                    };
                    let failed = value.is_err();
                    attempts.fetch_add(1, Ordering::SeqCst);
                    if sender.send(value).is_err() || failed {
                        break;
                    }
                }
            })?;
        Ok(Self {
            receiver: Some(receiver),
            thread: Some(thread),
            send_attempts,
        })
    }
    fn next(&self) -> Result<Option<Vec<u8>>> {
        let receiver = self.receiver.as_ref().context("pump already joined")?;
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(value) => Ok(Some(value?)),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
            Err(mpsc::RecvTimeoutError::Timeout) => bail!("PTY output stalled"),
        }
    }
    fn join(mut self) -> Result<()> {
        self.receiver.take();
        self.thread
            .take()
            .context("missing reader thread")?
            .join()
            .map_err(|_| anyhow::anyhow!("PTY reader panicked"))
    }
}
impl Drop for Pump {
    fn drop(&mut self) {
        self.receiver.take(); // Unblock a sender before session teardown on error.
        // A blocked OS read is not cancellable here. Helper's independent finite
        // lifetime closes it. Do not wait indefinitely while unwinding failure.
    }
}
fn spawn(mode: &str) -> Result<(PtySession, Pump)> {
    let mut command = PtyCommand::new(std::env::current_exe()?);
    command.args = vec![
        format!("pty-helper-{mode}").into(),
        "literal ; $(argv)".into(),
    ];
    command.cwd = Some(super::root()?.join("target/probes"));
    command
        .env
        .push(("PULSE_PTY_TEST".into(), "explicit-value".into()));
    let mut session = PtySession::spawn(&command, TermSize::new(80, 24)?)?;
    let reader = session.take_reader()?;
    ensure!(
        matches!(session.take_reader(), Err(PtyError::ReaderTaken)),
        "competing reader accepted"
    );
    let pump = Pump::new(reader)?;
    Ok((session, pump))
}
fn until(pump: &Pump, bytes: &mut Vec<u8>, marker: &[u8]) -> Result<()> {
    while !bytes.windows(marker.len()).any(|window| window == marker) {
        let chunk = pump.next()?.context("unexpected EOF before marker")?;
        ensure!(
            bytes.len() + chunk.len() <= 8192,
            "unexpected handshake size"
        );
        bytes.extend(chunk);
    }
    Ok(())
}

pub fn worker() -> Result<()> {
    ensure!(
        cfg!(unix),
        "probe-pty currently validates Unix raw bytes only; Windows ConPTY matrix is pending"
    );
    std::fs::create_dir_all(super::root()?.join("target/probes"))?;
    let (mut session, pump) = spawn("interactive")?;
    let mut captured = vec![];
    until(&pump, &mut captured, b"READY\r\n")?;
    let resized = TermSize::new(91, 37)?;
    session.resize(resized)?;
    ensure!(
        session.dimensions()? == resized,
        "native size readback disagrees"
    );
    let mut terminal = TerminalProbe::new(resized, 128);
    terminal.feed(&captured);
    session.write_input(format!("{INPUT}\n").as_bytes())?;
    let mut replies = vec![];
    while let Some(chunk) = pump.next()? {
        ensure!(
            captured.len() + chunk.len() <= 8192,
            "unexpected interactive output size"
        );
        captured.extend_from_slice(&chunk);
        terminal.feed(&chunk);
        for reply in terminal.take_replies() {
            session.write_input(reply.as_bytes())?;
            replies.push(reply);
        }
    }
    terminal.finish_input();
    let exit = session.wait()?;
    ensure!(
        exit.exit_code() == 7 && !exit.success(),
        "nonzero exit status lost: {exit}; output: {}",
        String::from_utf8_lossy(&captured)
    );
    ensure!(
        replies == ["\x1b[1;7R"],
        "backend did not exclusively answer cursor query"
    );
    ensure!(
        captured.windows(8).any(|row| row == b"QUERY_OK"),
        "helper did not accept reply"
    );
    let screen = terminal.screen();
    ensure!(
        screen.cells[0]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
            .starts_with("PTY 中"),
        "Rust screen disagrees with output"
    );
    ensure!(
        screen.cells[1]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
            .starts_with("QUERY_OK"),
        "trailing output lost"
    );
    session.close_input();
    session.close_input();
    ensure!(
        matches!(session.write_input(b"x"), Err(PtyError::InputClosed)),
        "closed input accepted"
    );
    ensure!(
        session
            .try_wait()?
            .context("lost cached status")?
            .exit_code()
            == 7,
        "exit cache changed"
    );
    pump.join()?;
    drop(session);

    // Real system shell smoke, using a fixed script rather than interpolating
    // user data. This is not interactive/login-shell or shell-integration coverage.
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec![
        "-c".into(),
        "test -t 0 && test -t 1 && printf '\\033cSHELL_OK\\n'".into(),
    ];
    let mut shell = PtySession::spawn(&command, TermSize::new(32, 8)?)?;
    let pump = Pump::new(shell.take_reader()?)?;
    let mut terminal = TerminalProbe::new(TermSize::new(32, 8)?, 128);
    let mut shell_bytes = 0;
    while let Some(chunk) = pump.next()? {
        shell_bytes += chunk.len();
        ensure!(shell_bytes <= 8192, "unexpected shell output size");
        terminal.feed(&chunk);
    }
    terminal.finish_input();
    ensure!(shell.wait()?.success(), "system shell smoke failed");
    ensure!(
        terminal.screen().cells[0]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
            .starts_with("SHELL_OK"),
        "system shell output was not parsed"
    );
    pump.join()?;
    drop(shell);

    let (mut flood, pump) = spawn("flood")?;
    let mut handshake = vec![];
    until(&pump, &mut handshake, b"FLOOD_READY\r\n")?;
    flood.write_input(b"g")?;
    thread::sleep(Duration::from_millis(150)); // Stop consuming and let queues fill.
    let stalled = pump.send_attempts.load(Ordering::SeqCst);
    ensure!(stalled >= QUEUE + 2, "reader never filled bounded queue");
    thread::sleep(Duration::from_millis(50));
    ensure!(
        pump.send_attempts.load(Ordering::SeqCst) == stalled,
        "reader advanced without consumption"
    );
    ensure!(
        flood.try_wait()?.is_none(),
        "producer finished despite stopped consumer"
    );
    let mut count = 0;
    let expected_suffix = b"\r\nFLOOD_DONE\r\n";
    while let Some(chunk) = pump.next()? {
        for byte in chunk {
            let expected = if count < FLOOD {
                b'A' + (count % 26) as u8
            } else {
                *expected_suffix
                    .get(count - FLOOD)
                    .context("extra flood byte")?
            };
            ensure!(
                byte == expected,
                "lost/altered/reordered flood byte at {count}"
            );
            count += 1;
        }
        thread::sleep(Duration::from_millis(1));
    }
    ensure!(count == FLOOD + expected_suffix.len(), "truncated flood");
    ensure!(flood.wait()?.success(), "flood failed");
    pump.join()?;
    drop(flood);

    let (mut cancelled, pump) = spawn("cancel")?;
    let mut ready = vec![];
    until(&pump, &mut ready, b"CANCEL_READY\r\n")?;
    let started = Instant::now();
    let status = cancelled.terminate()?;
    ensure!(
        !status.success(),
        "termination status unexpectedly successful"
    );
    while pump.next()?.is_some() {}
    pump.join()?;
    let cancellation_ms = started.elapsed().as_secs_f64() * 1000.0;
    ensure!(
        cancellation_ms < 2000.0,
        "direct-child cancellation exceeded bound"
    );
    let cached = cancelled.terminate()?;
    ensure!(
        cached.exit_code() == status.exit_code() && cached.signal() == status.signal(),
        "termination is not idempotent"
    );
    drop(cancelled);
    let (live, pump) = spawn("cancel")?;
    let mut ready = vec![];
    until(&pump, &mut ready, b"CANCEL_READY\r\n")?;
    drop(live); // Best-effort Drop must close this simple directly spawned helper.
    while pump.next()?.is_some() {}
    pump.join()?;
    let missing = PtyCommand::new(super::root()?.join("target/missing-pty-executable"));
    ensure!(
        PtySession::spawn(&missing, resized).is_err(),
        "missing executable accepted"
    );

    let report = json!({
        "schemaVersion": 1, "completed": true, "platform": std::env::consts::OS,
        "portablePty": "0.9.0", "realTerminal": true,
        "literalArgvCwdEnv": true, "inputRoundtrip": true, "resizeChildObserved": true,
        "backendReplies": replies, "nonzeroExit": exit.exit_code(), "outputEof": true,
        "slowConsumerBytesVerified": FLOOD, "queueCapacityChunks": QUEUE,
        "readChunkBytes": CHUNK, "stagingBufferBytesUpperBound": (QUEUE + 3) * CHUNK,
        "blockedReaderObserved": true, "producerPausedObserved": true,
        "dropClosesLiveChild": true, "shellSmoke": true,
        "cancellationMs": cancellation_ms, "missingExecutableRejected": true,
        "limitations": ["Unix only; not Windows ConPTY acceptance", "Self-contained helper and /bin/sh smoke, not interactive/login shell or vim/tmux compatibility", "No GUI detach/attach or async actor integration", "Direct-child termination only; descendant trees and hung OS reads need production supervision", "Heap bound excludes kernel buffers and captured handshake/parser state"]
    });
    std::fs::write(
        super::root()?.join("target/probes/pty.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Native PTY input, query reply, resize, EOF, slow consumer and termination passed");
    Ok(())
}

pub fn run() -> Result<()> {
    ensure!(
        cfg!(unix),
        "probe-pty raw-byte scenarios require Unix; Windows ConPTY validation is pending"
    );
    let report = super::root()?.join("target/probes/pty.json");
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let mut child = Command::new(std::env::current_exe()?)
        .arg("probe-pty-worker")
        .stdin(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(25);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("PTY probe exceeded 25 seconds; helpers independently expire within 10 seconds");
        }
        thread::sleep(Duration::from_millis(20));
    };
    ensure!(status.success(), "PTY worker failed: {status}");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    validate_report(&value)?;
    println!("Evidence: {}", report.display());
    Ok(())
}

fn validate_report(value: &serde_json::Value) -> Result<()> {
    for flag in [
        "completed",
        "realTerminal",
        "literalArgvCwdEnv",
        "inputRoundtrip",
        "resizeChildObserved",
        "outputEof",
        "blockedReaderObserved",
        "producerPausedObserved",
        "dropClosesLiveChild",
        "shellSmoke",
        "missingExecutableRejected",
    ] {
        ensure!(value[flag] == true, "missing PTY evidence: {flag}");
    }
    ensure!(
        value["schemaVersion"] == 1
            && value["platform"] == std::env::consts::OS
            && value["portablePty"] == "0.9.0",
        "invalid PTY report version/platform"
    );
    ensure!(
        value["slowConsumerBytesVerified"] == FLOOD
            && value["queueCapacityChunks"] == QUEUE
            && value["readChunkBytes"] == CHUNK
            && value["stagingBufferBytesUpperBound"] == (QUEUE + 3) * CHUNK,
        "invalid byte/queue bound evidence"
    );
    ensure!(
        value["nonzeroExit"] == 7 && value["backendReplies"] == json!(["\x1b[1;7R"]),
        "invalid child exit or query evidence"
    );
    let cancelled = value["cancellationMs"]
        .as_f64()
        .context("missing cancellation time")?;
    ensure!(
        cancelled.is_finite() && (0.0..2000.0).contains(&cancelled),
        "invalid cancellation bound"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> serde_json::Value {
        json!({ "schemaVersion": 1, "platform": std::env::consts::OS, "portablePty": "0.9.0", "completed": true, "realTerminal": true, "literalArgvCwdEnv": true, "inputRoundtrip": true, "resizeChildObserved": true, "outputEof": true, "blockedReaderObserved": true, "producerPausedObserved": true, "dropClosesLiveChild": true, "shellSmoke": true, "missingExecutableRejected": true, "slowConsumerBytesVerified": FLOOD, "queueCapacityChunks": QUEUE, "readChunkBytes": CHUNK, "stagingBufferBytesUpperBound": (QUEUE + 3) * CHUNK, "nonzeroExit": 7, "backendReplies": ["\x1b[1;7R"], "cancellationMs": 50.0 })
    }
    #[test]
    fn requires_real_observations_and_bounds() {
        assert!(validate_report(&report()).is_ok());
        for (key, wrong) in [
            ("realTerminal", json!(false)),
            ("blockedReaderObserved", json!(false)),
            ("schemaVersion", json!(0)),
            ("stagingBufferBytesUpperBound", json!(1)),
            ("cancellationMs", json!(2000)),
            ("backendReplies", json!([])),
            ("nonzeroExit", json!(0)),
        ] {
            let mut value = report();
            value[key] = wrong;
            assert!(validate_report(&value).is_err(), "accepted wrong {key}");
        }
    }
    #[test]
    fn refuses_empty_success_flag_only_report() {
        assert!(validate_report(&json!({ "completed": true })).is_err());
    }
}
