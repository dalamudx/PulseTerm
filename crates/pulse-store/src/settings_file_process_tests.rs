//! Fixed child fixtures; process termination does not simulate power failure.
use super::*;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const CHILD: &str = "settings_file::unix::process_tests::fixture_child";
const DIR_ENV: &str = "PULSE_SETTINGS_FIXTURE_DIRECTORY";
const MODE_ENV: &str = "PULSE_SETTINGS_FIXTURE_MODE";
fn directory() -> std::io::Result<tempfile::TempDir> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}
fn marker(phase: &str) -> std::io::Result<()> {
    println!("\nPULSE_SETTINGS_FIXTURE:{phase}:{}", std::process::id());
    std::io::stdout().flush()
}
#[test]
#[ignore = "Only parent tests invoke this fixture with private paths and exact phase/PID"]
fn fixture_child() -> TestResult {
    let dir = std::env::var_os(DIR_ENV).ok_or("missing private directory")?;
    let mode = std::env::var(MODE_ENV)?;
    if !["temp", "installed", "writer18", "writer20"].contains(&mode.as_str()) {
        return Err("invalid fixed mode".into());
    }
    let store = FileStore::open(dir.into())?;
    let initial = store.load()?;
    if mode.starts_with("writer") {
        marker("loaded")?;
        let mut command = String::new();
        std::io::stdin().lock().take(4).read_line(&mut command)?;
        if command != "GO\n" {
            return Err("invalid fixed release".into());
        }
        let size = if mode == "writer18" { 18 } else { 20 };
        let result = store.save(
            &initial,
            SettingsMutation::Set(TerminalSettingEdit::FontSize(size)),
            |_| Ok(()),
        );
        match result {
            Ok(_) => marker("committed")?,
            Err(SettingsFileError::Conflict) => marker("conflict")?,
            Err(_) => return Err("unexpected save failure".into()),
        }
        return Ok(());
    }
    store.save(
        &initial,
        SettingsMutation::Set(TerminalSettingEdit::FontSize(18)),
        |stage| {
            let stop = matches!(
                (mode.as_str(), stage),
                ("temp", Stage::TempSynced) | ("installed", Stage::Installed)
            );
            if stop {
                marker(&mode).map_err(|_| SettingsFileError::Io)?;
                thread::sleep(Duration::from_secs(10));
                return Err(SettingsFileError::Io);
            }
            Ok(())
        },
    )?;
    Err("fixture returned without forced termination".into())
}
struct OwnedChild {
    child: Child,
    events: mpsc::Receiver<String>,
    reader: Option<thread::JoinHandle<()>>,
}
impl OwnedChild {
    fn spawn(path: &std::path::Path, mode: &str) -> TestResult<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                CHILD,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(DIR_ENV, path)
            .env(MODE_ENV, mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().ok_or("missing child stdout")?;
        let (send, events) = mpsc::sync_channel(4);
        let reader = thread::Builder::new()
            .name("settings-fixed-receipts".into())
            .spawn(move || {
                let mut stream = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match stream.by_ref().take(1025).read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    if line.len() > 1024 {
                        break;
                    }
                    let text = line.trim_end_matches(['\r', '\n']);
                    if text.starts_with("PULSE_SETTINGS_FIXTURE:")
                        && send.send(text.to_owned()).is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            child,
            events,
            reader: Some(reader),
        })
    }
    fn expect(&self, phase: &str) -> TestResult {
        let expected = format!("PULSE_SETTINGS_FIXTURE:{phase}:{}", self.child.id());
        let actual = self.events.recv_timeout(Duration::from_secs(5))?;
        if actual != expected {
            return Err("mismatched phase or PID".into());
        }
        Ok(())
    }
    fn release(&mut self) -> TestResult {
        self.child
            .stdin
            .take()
            .ok_or("missing child input")?
            .write_all(b"GO\n")?;
        Ok(())
    }
    fn finish(&mut self) -> TestResult {
        if !self.child.wait()?.success() {
            return Err("fixture exit was not success".into());
        }
        Ok(())
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn initial(dir: &std::path::Path) -> TestResult {
    let store = FileStore::open(dir.into())?;
    let snap = store.load()?;
    store.save(
        &snap,
        SettingsMutation::Set(TerminalSettingEdit::FontSize(14)),
        |_| Ok(()),
    )?;
    Ok(())
}
#[test]
fn two_independent_process_snapshots_cannot_overwrite_committed_settings() -> TestResult {
    let dir = directory()?;
    initial(dir.path())?;
    let mut first = OwnedChild::spawn(dir.path(), "writer18")?;
    let mut stale = OwnedChild::spawn(dir.path(), "writer20")?;
    first.expect("loaded")?;
    stale.expect("loaded")?;
    first.release()?;
    first.expect("committed")?;
    first.finish()?;
    stale.release()?;
    stale.expect("conflict")?;
    stale.finish()?;
    assert_eq!(
        FileStore::open(dir.path().into())?.load()?.text(),
        "[terminal]\nfont_size = 18\n"
    );
    Ok(())
}
fn killed_at_phase(mode: &str, expected_size: u16) -> TestResult {
    let dir = directory()?;
    initial(dir.path())?;
    let before = fs::read(dir.path().join(TARGET))?;
    let mut child = OwnedChild::spawn(dir.path(), mode)?;
    child.expect(mode)?;
    assert!(matches!(
        FileStore::open(dir.path().into())?.load(),
        Err(SettingsFileError::Busy)
    ));
    child.child.kill()?;
    assert!(!child.child.wait()?.success());
    let snapshot = FileStore::open(dir.path().into())?.load()?;
    let patch = SettingsPatch::parse(snapshot.text())?;
    assert_eq!(patch.terminal.font_size, Some(expected_size));
    if mode == "temp" {
        assert_eq!(fs::read(dir.path().join(TARGET))?, before);
        assert!(fs::read_dir(dir.path())?.count() > 2);
    }
    Ok(())
}
#[test]
fn process_kill_before_rename_preserves_original_and_releases_lock() -> TestResult {
    killed_at_phase("temp", 14)
}
#[test]
fn process_kill_after_rename_recovers_complete_new_text_without_claiming_power_durability()
-> TestResult {
    killed_at_phase("installed", 18)
}
