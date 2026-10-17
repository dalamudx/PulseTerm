//! Isolated-process reopen and CAS evidence, never user directories or network targets.
use crate::{StateStore, StoreError, ssh_hosts::HostEdit};
use pulse_core::ssh_endpoint::SshEndpoint;
use std::process::Command;
use std::sync::Arc;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn separate_process_reopen_and_stale_capture_cannot_overwrite_new_host() -> TestResult {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let exe = std::env::current_exe()?;
    let path = d.path().join("state.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (store, task) = StateStore::open(path.clone()).await?;
        let empty = store.load_ssh_hosts().await?;
        let first = store
            .edit_ssh_hosts(
                empty,
                HostEdit::Create(SshEndpoint::new("initial", "example.com", 22, "user")?),
                Arc::new(|| true),
            )
            .await?;
        assert_eq!(first.revision(), 1);
        drop(store);
        task.join().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    let mut first = Command::new(&exe)
        .args([
            "--exact",
            "ssh_hosts_process_tests::catalogue_worker",
            "--ignored",
            "--nocapture",
        ])
        .env("PULSE_HOST_FIXTURE_DB", &path)
        .env("PULSE_HOST_FIXTURE_MODE", "stale")
        .stdout(std::process::Stdio::piped())
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    let stdout = first.stdout.take().ok_or("missing stdout")?;
    let mut lines = std::io::BufRead::lines(std::io::BufReader::new(stdout));
    let mut loaded = false;
    for line in &mut lines {
        if line? == "fixture.host.loaded" {
            loaded = true;
            break;
        }
    }
    assert!(loaded);
    let second = Command::new(&exe)
        .args([
            "--exact",
            "ssh_hosts_process_tests::catalogue_worker",
            "--ignored",
            "--nocapture",
        ])
        .env("PULSE_HOST_FIXTURE_DB", &path)
        .env("PULSE_HOST_FIXTURE_MODE", "update")
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let result = second.wait_with_output()?;
    assert!(result.status.success(), "fixed child update failed");
    assert!(String::from_utf8_lossy(&result.stdout).contains("fixture.host.updated"));
    let mut stdin = first.stdin.take().ok_or("missing stdin")?;
    std::io::Write::write_all(&mut stdin, b"go\n")?;
    drop(stdin);
    assert!(first.wait()?.success());
    let remaining: Result<Vec<_>, _> = lines.collect();
    assert!(
        remaining?
            .iter()
            .any(|s| s == "fixture.host.stale_rejected")
    );
    runtime.block_on(async {
        let (store, task) = StateStore::open(path).await?;
        let current = store.load_ssh_hosts().await?;
        assert_eq!(current.revision(), 2);
        assert_eq!(current.records().len(), 1);
        assert_eq!(current.records()[0].endpoint().label(), "updated");
        assert_eq!(current.records()[0].endpoint().target().port(), 2222);
        drop(store);
        task.join().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
#[test]
#[ignore = "only launched by the independent-process fixture"]
fn catalogue_worker() -> TestResult {
    let path = std::env::var_os("PULSE_HOST_FIXTURE_DB").ok_or("missing private DB fixture")?;
    let mode = std::env::var("PULSE_HOST_FIXTURE_MODE")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (store, task) = StateStore::open(path.into()).await?;
        let current = store.load_ssh_hosts().await?;
        assert_eq!(current.revision(), 1);
        let id = current.records()[0].id();
        match mode.as_str() {
            "stale" => {
                println!("fixture.host.loaded");
                std::io::Write::flush(&mut std::io::stdout())?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                assert_eq!(line, "go\n");
                assert!(matches!(
                    store
                        .edit_ssh_hosts(
                            current,
                            HostEdit::Update(
                                id,
                                SshEndpoint::new("stale", "example.com", 22, "user")?
                            ),
                            Arc::new(|| true)
                        )
                        .await,
                    Err(StoreError::HostConflict)
                ));
                println!("fixture.host.stale_rejected");
            }
            "update" => {
                let new = store
                    .edit_ssh_hosts(
                        current,
                        HostEdit::Update(
                            id,
                            SshEndpoint::new("updated", "example.com", 2222, "user")?,
                        ),
                        Arc::new(|| true),
                    )
                    .await?;
                assert_eq!(new.revision(), 2);
                assert_eq!(new.records()[0].id(), id);
                println!("fixture.host.updated");
            }
            _ => return Err("invalid fixed mode".into()),
        }
        drop(store);
        task.join().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
