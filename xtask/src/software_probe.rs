use anyhow::{Context, Result, ensure};
use pulse_core::TermSize;
use pulse_pty::{PtyCommand, PtySession};
use pulse_term::TerminalProbe;
use serde_json::{Value, json};
use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
const LIMIT: usize = 2 * 1024 * 1024;
struct Pump {
    rx: Option<mpsc::Receiver<std::io::Result<Vec<u8>>>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Pump {
    fn start(mut reader: Box<dyn Read + Send>) -> Self {
        let (tx, rx) = mpsc::sync_channel(2);
        let thread = thread::spawn(move || {
            let mut bytes = [0; 4096];
            loop {
                let data = match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => Ok(bytes[..n].to_vec()),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => Err(e),
                };
                let failed = data.is_err();
                if tx.send(data).is_err() || failed {
                    break;
                }
            }
        });
        Self {
            rx: Some(rx),
            thread: Some(thread),
        }
    }
    fn collect(
        &self,
        session: &mut PtySession,
        terminal: &mut TerminalProbe,
        journal: &mut Vec<u8>,
    ) -> Result<Vec<u8>> {
        let start = journal.len();
        let deadline = Instant::now() + Duration::from_secs(3);
        let rx = self.rx.as_ref().context("reader consumed")?;
        loop {
            ensure!(Instant::now() < deadline, "software output did not idle");
            match rx.recv_timeout(Duration::from_millis(150)) {
                Ok(bytes) => {
                    let bytes = bytes?;
                    ensure!(
                        journal.len() + bytes.len() <= LIMIT,
                        "software journal quota exceeded"
                    );
                    terminal.feed(&bytes);
                    journal.extend_from_slice(&bytes);
                    for reply in terminal.take_replies() {
                        session.write_input(reply.as_bytes())?;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(journal[start..].to_vec())
    }
    fn join(mut self) -> Result<()> {
        self.rx.take();
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("software reader panicked"))?;
        }
        Ok(())
    }
}
impl Drop for Pump {
    fn drop(&mut self) {
        self.rx.take();
    }
}
struct TmuxSocket(String);
impl Drop for TmuxSocket {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
fn program(name: &str, file: &std::path::Path, socket: &str) -> PtyCommand {
    let mut cmd = PtyCommand::new(name);
    cmd.env = vec![
        ("TERM".into(), "xterm-256color".into()),
        ("LC_ALL".into(), "C.UTF-8".into()),
    ];
    cmd.args = match name {
        "vim" => vec![
            "-u".into(),
            "NONE".into(),
            "-i".into(),
            "NONE".into(),
            "-n".into(),
            "-N".into(),
            "-X".into(),
            file.as_os_str().to_owned(),
        ],
        "less" => vec!["-R".into(), file.as_os_str().to_owned()],
        "htop" => vec![
            "-C".into(),
            "-d".into(),
            "100".into(),
            "-p".into(),
            std::process::id().to_string().into(),
        ],
        "tmux" => vec![
            "-L".into(),
            socket.into(),
            "-f".into(),
            "/dev/null".into(),
            "new-session".into(),
            "-s".into(),
            "probe".into(),
            "/bin/sh".into(),
            "-c".into(),
            "printf 'TMUX_READY\\n'; sleep 12".into(),
        ],
        _ => vec![],
    };
    cmd
}
struct Capture {
    prefix: Vec<u8>,
    before: String,
    restore: Result<pulse_term::Restore, pulse_term::RestoreGap>,
}
impl Capture {
    fn new(source: &TerminalProbe, prefix: &[u8]) -> Self {
        Self {
            prefix: prefix.to_vec(),
            before: source.restore_fingerprint(),
            restore: source.restore_sequence(),
        }
    }
}
fn sample(
    name: &str,
    stage: &str,
    source: &TerminalProbe,
    captured: Capture,
    suffix: Vec<u8>,
    resized: Option<TermSize>,
) -> Result<Value> {
    let Capture {
        prefix,
        before,
        restore,
    } = captured;
    let strict = restore.as_ref().is_ok_and(|r| r.approximations.is_empty());
    let mut rust_exact = None;
    if let Ok(frame) = &restore
        && strict
    {
        let mut recovered = TerminalProbe::new(TermSize::new(80, 24)?, 128);
        recovered.feed(&frame.bytes);
        let initial = recovered.restore_fingerprint() == before;
        let initial_history_restore = recovered.recent_history_projection(128);
        let mut boundary = TerminalProbe::new(TermSize::new(80, 24)?, 128);
        boundary.feed(&prefix);
        let initial_history_source = boundary.recent_history_projection(128);
        if let Some(size) = resized {
            recovered.resize(size);
        }
        recovered.feed(&suffix);
        let exact = initial && recovered.restore_fingerprint() == source.restore_fingerprint();
        if !exact {
            std::fs::write(
                super::root()?.join("target/probes/software-failure.json"),
                serde_json::to_vec_pretty(
                    &json!({"name":name,"stage":stage,"initial":initial,"sourceHistory":initial_history_source,"restoredHistory":initial_history_restore,"prefix":prefix,"suffix":suffix,"restore":frame.bytes,"before":before,"expected":source.restore_fingerprint(),"actual":recovered.restore_fingerprint()}),
                )?,
            )?;
        }
        ensure!(exact, "{name}/{stage}: backend recovery regression");
        rust_exact = Some(exact);
    }
    let (frame, gap, approximations) = match restore {
        Ok(frame) => (Some(frame.bytes), None, frame.approximations),
        Err(gap) => (None, Some(format!("{gap:?}")), vec![]),
    };
    Ok(
        json!({"name":format!("{name}-{stage}"),"prefix":prefix,"suffix":suffix,"restore":frame,"gap":gap,"approximations":approximations,"strict":strict,"rustExact":rust_exact,"expected":source.screen(),"resized":resized.map(|s|json!({"columns":s.columns(),"rows":s.rows()})),"risk":source.divergence_risk()}),
    )
}
fn record(
    name: &str,
    file: &std::path::Path,
    directory: &std::path::Path,
) -> Result<(Value, Vec<Value>)> {
    let socket = format!("pulseterm-record-{}", std::process::id());
    let _socket = (name == "tmux").then(|| TmuxSocket(socket.clone()));
    let cmd = program(name, file, &socket);
    let mut session = PtySession::spawn(&cmd, TermSize::new(80, 24)?)?;
    let pump = Pump::start(session.take_reader()?);
    let mut terminal = TerminalProbe::new(TermSize::new(80, 24)?, 128);
    let mut journal = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(4);
    while journal.is_empty() {
        ensure!(Instant::now() < deadline, "{name}: no startup output");
        pump.collect(&mut session, &mut terminal, &mut journal)?;
    }
    ensure!(session.try_wait()?.is_none(), "{name}: exited at startup");
    let captured = Capture::new(&terminal, &journal);
    let key = match name {
        "vim" | "less" => b"G".as_slice(),
        "htop" => b"\x1b[B".as_slice(),
        "tmux" => b"\x02:display-message PulseTerm-replay\r".as_slice(),
        _ => b"",
    };
    session.write_input(key)?;
    let suffix = pump.collect(&mut session, &mut terminal, &mut journal)?;
    let first = sample(name, "input", &terminal, captured, suffix, None)?;
    let captured = Capture::new(&terminal, &journal);
    let resized = TermSize::new(91, 37)?;
    session.resize(resized)?;
    terminal.resize(resized);
    let suffix = pump.collect(&mut session, &mut terminal, &mut journal)?;
    let second = sample(name, "resize", &terminal, captured, suffix, Some(resized))?;
    let quit = match name {
        "vim" => b":qa!\r".as_slice(),
        "tmux" => b"\x02:kill-server\r".as_slice(),
        _ => b"q",
    };
    if name == "tmux" {
        ensure!(
            std::process::Command::new("tmux")
                .args(["-L", &socket, "kill-server"])
                .status()?
                .success(),
            "tmux dedicated server shutdown failed"
        );
    } else {
        session.write_input(quit)?;
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        pump.collect(&mut session, &mut terminal, &mut journal)?;
        if session.try_wait()?.is_some() {
            break;
        }
        ensure!(Instant::now() < deadline, "{name}: quit failed");
    }
    let exit = session.wait()?.exit_code();
    ensure!(
        exit == 0 || (name == "tmux" && exit == 1),
        "{name}: unexpected exit {exit}"
    ); // tmux client reports server-exited as 1 after our explicit kill.
    drop(session);
    pump.join()?;
    std::fs::write(directory.join(format!("{name}.vt")), &journal)?;
    Ok((
        json!({"name":name,"bytes":journal.len(),"exit":exit,"sampleCount":2,"risk":terminal.divergence_risk()}),
        vec![first, second],
    ))
}
/// Finite parent receipt; killed workers close PTY fds. tmux's isolated shell
/// also has a twelve-second lifetime if external cancellation bypasses Drop.
pub fn run() -> Result<()> {
    ensure!(cfg!(unix), "software recordings require Unix PTY");
    let root = super::root()?.join("target/probes");
    std::fs::create_dir_all(&root)?;
    for name in [
        "software.json",
        "software-xterm.json",
        "software-failure.json",
    ] {
        let file = root.join(name);
        if file.exists() {
            std::fs::remove_file(file)?;
        }
    }
    let mut worker = std::process::Command::new(std::env::current_exe()?)
        .arg("probe-software-worker")
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(status) = worker.try_wait()? {
            ensure!(status.success(), "software worker {status}");
            return Ok(());
        }
        if Instant::now() >= deadline {
            worker.kill()?;
            worker.wait()?;
            anyhow::bail!("software worker exceeded 40 seconds");
        }
        thread::sleep(Duration::from_millis(20));
    }
}
pub fn worker() -> Result<()> {
    ensure!(cfg!(unix), "software recordings require Unix native PTY");
    let directory = super::root()?.join("target/probes/recordings");
    std::fs::create_dir_all(&directory)?;
    let file = directory.join("sample.txt");
    std::fs::write(
        &file,
        (0..100)
            .map(|i| format!("PulseTerm software sample {i:03} 中 ANSI text\n"))
            .collect::<String>(),
    )?;
    let report = super::root()?.join("target/probes/software.json");
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let mut programs = Vec::new();
    let mut cases = Vec::new();
    for name in ["vim", "less", "htop", "tmux"] {
        let (program, entries) =
            record(name, &file, &directory).with_context(|| format!("software probe {name}"))?;
        programs.push(program);
        cases.extend(entries);
    }
    ensure!(cases.len() == 8, "missing software scenarios");
    std::fs::write(
        &report,
        serde_json::to_vec_pretty(
            &json!({"schemaVersion":1,"platform":std::env::consts::OS,"columns":80,"rows":24,"programs":programs,"cases":cases,"limitations":["Eight controlled boundaries, not representative frequency statistics","Headless only, no native GUI or selection preservation","Full state excludes historical reflow after resize","tmux uses dedicated ephemeral socket; user servers are not touched"]}),
        )?,
    )?;
    println!("Recorded 4 real terminal programs and 8 continuation boundaries");
    Ok(())
}
