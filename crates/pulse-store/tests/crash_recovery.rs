//! Process-kill recovery fixtures. These do not simulate machine or power failure.
use pulse_store::{
    StateStore,
    profiles::{AutoProfilePreferences as Prefs, AutomaticShell as Shell},
};
use rusqlite::Connection;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const PATH_ENV: &str = "PULSE_STORE_CRASH_FIXTURE_PATH";
const MODE_ENV: &str = "PULSE_STORE_CRASH_FIXTURE_MODE";
const BEFORE: &str = "before-commit";
const AFTER: &str = "after-commit";
fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}
fn initial() -> std::result::Result<Prefs, pulse_store::StoreError> {
    let mut p = Prefs::default();
    p.set_hidden(Shell::Bash, true)?;
    p.suppress(Shell::Zsh);
    Ok(p)
}
fn changed() -> std::result::Result<Prefs, pulse_store::StoreError> {
    let mut p = Prefs::default();
    p.set_hidden(Shell::Fish, true)?;
    Ok(p)
}
#[test]
#[ignore = "Parent tests invoke this fixture in a private subprocess, then force termination"]
fn fixture_child() -> TestResult {
    let path = std::env::var_os(PATH_ENV).ok_or("missing private fixture path")?;
    let mode = std::env::var(MODE_ENV)?;
    if mode != BEFORE && mode != AFTER {
        return Err("invalid fixture phase".into());
    }
    let runtime = runtime()?;
    let (store, owner) = runtime.block_on(StateStore::open(path.clone().into()))?;
    assert_eq!(runtime.block_on(store.load_auto_profiles())?, initial()?);
    let connection = Connection::open(Path::new(&path))?;
    connection
        .execute_batch("PRAGMA synchronous=FULL; PRAGMA wal_autocheckpoint=0; BEGIN IMMEDIATE;")?;
    assert_eq!(
        connection.execute(
            "UPDATE app_state SET value=?1 WHERE key='profiles.automatic.unix.v1'",
            [r#"{"schemaVersion":1,"hidden":["fish"],"deleted":[]}"#]
        )?,
        1
    );
    if mode == AFTER {
        connection.execute_batch("COMMIT;")?;
    }
    // Both connections remain open. The process cannot quietly checkpoint WAL on close.
    let _live = (connection, store, owner);
    // libtest may print a test-name prefix without a trailing newline.
    println!("\nPULSE_STORE_CRASH_READY:{mode}:{}", std::process::id());
    std::io::stdout().flush()?;
    thread::sleep(Duration::from_secs(12));
    Err("fixture watchdog reached instead of parent-forced termination".into())
}
struct OwnedChild {
    child: Child,
    pipe: Option<thread::JoinHandle<()>>,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(pipe) = self.pipe.take() {
            let _ = pipe.join();
        }
    }
}
fn kill_at_phase(path: &Path, mode: &str) -> TestResult {
    let child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "fixture_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PATH_ENV, path)
        .env(MODE_ENV, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut owned = OwnedChild { child, pipe: None };
    let expected = format!("PULSE_STORE_CRASH_READY:{mode}:{}", owned.child.id());
    let stdout = owned.child.stdout.take().ok_or("fixture stdout missing")?;
    let (ready, result) = mpsc::sync_channel(1);
    owned.pipe = Some(
        thread::Builder::new()
            .name("store-crash-ready".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut sent = false;
                for _ in 0..32 {
                    let mut line = Vec::new();
                    match Read::by_ref(&mut reader)
                        .take(1024)
                        .read_until(b'\n', &mut line)
                    {
                        Ok(n) if n > 0 && n < 1024 => {
                            if !sent
                                && String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n'])
                                    == expected
                            {
                                let _ = ready.try_send(true);
                                sent = true;
                            }
                        }
                        _ => break,
                    }
                }
                if !sent {
                    let _ = ready.try_send(false);
                }
            })?,
    );
    if !result.recv_timeout(Duration::from_secs(5))? {
        return Err("fixture did not reach the exact phase".into());
    }
    if mode == AFTER {
        assert!(
            std::fs::metadata(path.with_file_name("pulse.db-wal"))?.len() > 32,
            "committed WAL evidence missing"
        );
    }
    if owned.child.try_wait()?.is_some() {
        return Err("fixture exited before forced termination".into());
    }
    owned.child.kill()?;
    let status = owned.child.wait()?;
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.signal(),
            Some(9),
            "fixture must terminate by SIGKILL, not its watchdog"
        );
    }
    #[cfg(windows)]
    {
        assert_eq!(
            status.code(),
            Some(1),
            "fixture must terminate by Child::kill, not its watchdog"
        );
    }
    assert!(!status.success());
    Ok(())
}
fn check(mode: &str, expected: Prefs) -> TestResult {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.path().join("pulse.db");
    let runtime = runtime()?;
    runtime.block_on(async {
        let (store, owner) = StateStore::open(path.clone()).await?;
        store
            .save_auto_profiles(Prefs::default(), initial()?)
            .await?;
        owner.shutdown().await?;
        Ok::<_, pulse_store::StoreError>(())
    })?;
    kill_at_phase(&path, mode)?;
    runtime.block_on(async {
        let (store, owner) = StateStore::open(path).await?;
        assert_eq!(store.load_auto_profiles().await?, expected);
        owner.shutdown().await?;
        Ok::<_, pulse_store::StoreError>(())
    })?;
    Ok(())
}
#[test]
fn forced_exit_before_commit_preserves_previous_preferences() -> TestResult {
    check(BEFORE, initial()?)
}
#[test]
fn forced_exit_after_commit_recovers_uncheckpointed_wal() -> TestResult {
    check(AFTER, changed()?)
}
