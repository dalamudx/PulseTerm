//! Private read-only daemon election fixture; not Tauri single-instance/cold GUI startup.
#[cfg(test)]
#[path = "cli_startup_probe_tests.rs"]
mod tests;
use super::*;
use std::os::unix::fs::FileTypeExt;
use tokio::io::{AsyncBufReadExt, BufReader};
fn validate_root(root: &Path) -> Result<u32> {
    let m = std::fs::symlink_metadata(root)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink() && m.permissions().mode() & 0o777 == 0o700,
        "unsafe startup directory"
    );
    Ok(m.uid())
}
fn clean_owned_endpoint(root: &Path) -> Result<()> {
    let uid = validate_root(root)?;
    let socket = root.join("rpc.sock");
    match std::fs::symlink_metadata(&socket) {
        Ok(m) => ensure!(
            m.file_type().is_socket() && m.uid() == uid && m.permissions().mode() & 0o777 == 0o600,
            "refuse unknown socket-path object"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    };
    let token = root.join("token");
    match std::fs::symlink_metadata(&token) {
        Ok(m) => {
            ensure!(
                m.is_file()
                    && !m.file_type().is_symlink()
                    && m.uid() == uid
                    && m.nlink() == 1
                    && m.permissions().mode() & 0o777 == 0o600
                    && m.len() == 64,
                "refuse unsafe old token"
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    };
    if token.exists() {
        let _ = private_token(&token)?;
    }
    if socket.exists() {
        std::fs::remove_file(socket)?;
    }
    if token.exists() {
        std::fs::remove_file(token)?;
    }
    Ok(())
}
async fn wait_file(path: &Path) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    Ok(())
}
struct Candidate {
    process: tokio::process::Child,
    role: Value,
}
async fn launch(root: &Path, id: &str) -> Result<Candidate> {
    let mut process = Command::new(std::env::current_exe()?)
        .arg("cli-startup-helper")
        .arg(root)
        .arg(id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = process
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("candidate stdout missing"))?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut line)).await??;
    ensure!(line.len() <= 256, "role receipt cap");
    let role: Value = serde_json::from_str(&line)?;
    ensure!(
        role["pid"].as_u64() == process.id().map(u64::from) && role["primary"].is_boolean(),
        "candidate identity receipt"
    );
    Ok(Candidate { process, role })
}
async fn stopped(candidate: &mut Candidate) -> Result<()> {
    ensure!(
        tokio::time::timeout(Duration::from_secs(3), candidate.process.wait())
            .await??
            .success(),
        "candidate failed"
    );
    Ok(())
}
async fn probe() -> Result<Value> {
    let directory = tempfile::Builder::new()
        .prefix("pulse-startup-")
        .tempdir()?;
    let root = directory.path();
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    let lock_path = root.join("election.lock");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&lock_path)?;
    drop(file);
    let (a, b) = tokio::try_join!(launch(root, "a"), launch(root, "b"))?;
    ensure!(
        a.role["primary"].as_bool() != b.role["primary"].as_bool(),
        "two competing launches elected two primaries"
    );
    let (mut primary, mut secondary) = if a.role["primary"] == true {
        (a, b)
    } else {
        (b, a)
    };
    stopped(&mut secondary).await?;
    wait_file(&root.join("ready")).await?;
    let socket = root.join("rpc.sock");
    let token = root.join("token");
    let before_inode = std::fs::symlink_metadata(&socket)?.ino();
    let before_token = private_token(&token)?;
    let mut live = launch(root, "live").await?;
    ensure!(
        live.role["primary"] == false,
        "live endpoint ownership stolen"
    );
    stopped(&mut live).await?;
    ensure!(
        std::fs::symlink_metadata(&socket)?.ino() == before_inode
            && *private_token(&token)? == *before_token,
        "secondary changed live socket/token"
    );
    let executable = std::env::current_exe()?;
    for _ in 0..2 {
        child(&executable, &socket, &token, "good").await?;
    }
    wait_file(&root.join("served")).await?;
    std::fs::write(root.join("release"), b"release")?;
    stopped(&mut primary).await?;
    ensure!(
        !socket.exists() && !token.exists(),
        "normal primary left endpoint files"
    );
    std::fs::remove_file(root.join("ready"))?;
    std::fs::remove_file(root.join("served"))?;
    std::fs::remove_file(root.join("release"))?;
    let mut crashed = launch(root, "crash").await?;
    ensure!(
        crashed.role["primary"] == true,
        "lock not reusable after primary exit"
    );
    wait_file(&root.join("ready")).await?;
    let old_token = private_token(&token)?;
    crashed.process.start_kill()?;
    let status = tokio::time::timeout(Duration::from_secs(3), crashed.process.wait()).await??;
    ensure!(
        !status.success() && socket.exists() && token.exists(),
        "crash fixture did not leave actual stale endpoint"
    );
    std::fs::remove_file(root.join("ready"))?;
    let mut recovered = launch(root, "recover").await?;
    ensure!(
        recovered.role["primary"] == true,
        "crashed owner lock was not released by OS"
    );
    wait_file(&root.join("ready")).await?;
    ensure!(
        *private_token(&token)? != *old_token,
        "crash recovery reused old token"
    );
    let old_path = root.join("old-token");
    let mut old_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&old_path)?;
    old_file.write_all(old_token.as_slice())?;
    drop(old_file);
    child(&executable, &socket, &old_path, "denied-token").await?;
    child(&executable, &socket, &token, "good").await?;
    wait_file(&root.join("served")).await?;
    ensure!(
        std::fs::read(root.join("denials"))? == b"1",
        "old token not explicitly rejected before fresh token success"
    );
    std::fs::write(root.join("release"), b"release")?;
    stopped(&mut recovered).await?;
    ensure!(
        !socket.exists() && !token.exists(),
        "recovered primary failed cleanup"
    );
    let path = root.to_path_buf();
    directory.close()?;
    ensure!(!path.exists(), "startup runtime not removed");
    Ok(
        json!({"schemaVersion":1,"candidateProcesses":5,"authenticatedReadOnlyClients":3,"deniedOldTokenClients":1,"independentProcesses":9,"exactlyOneConcurrentPrimary":true,"secondaryPreservedLiveSocketAndToken":true,"normalExitReleasesLockAndEndpoint":true,"actualKilledPrimaryLeavesStaleSocket":true,"osReleasesCrashedOwnerLock":true,"ownedStaleEndpointRecovered":true,"startupTokenRotated":true,"oldTokenActuallyRejected":true,"newServiceSurvivesRejectedAuthentication":true,"allProcessesAndDirectoryCleaned":true,"limits":["Native Unix read-only daemon fixture, not Tauri plugin/single-instance or cold GUI launch","No startup-actions arbitrary command or production Capability execution","Current-user directory trust; cross-user/Windows named-pipe ACL remains unverified"]}),
    )
}
fn validate(r: &Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1
            && r["candidateProcesses"] == 5
            && r["authenticatedReadOnlyClients"] == 3
            && r["deniedOldTokenClients"] == 1
            && r["independentProcesses"] == 9,
        "CLI startup process counts"
    );
    for key in [
        "exactlyOneConcurrentPrimary",
        "secondaryPreservedLiveSocketAndToken",
        "normalExitReleasesLockAndEndpoint",
        "actualKilledPrimaryLeavesStaleSocket",
        "osReleasesCrashedOwnerLock",
        "ownedStaleEndpointRecovered",
        "startupTokenRotated",
        "oldTokenActuallyRejected",
        "newServiceSurvivesRejectedAuthentication",
        "allProcessesAndDirectoryCleaned",
    ] {
        ensure!(r[key] == true, "CLI startup evidence missing");
    }
    ensure!(
        r["limits"].as_array().is_some_and(|v| !v.is_empty()),
        "startup limitations missing"
    );
    Ok(())
}

pub(super) fn run() -> Result<()> {
    let root = super::super::root()?;
    let path = root.join("target/probes/cli-startup.json");
    std::fs::create_dir_all(root.join("target/probes"))?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let r = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(25), probe()).await })??;
    validate(&r)?;
    std::fs::write(path, serde_json::to_vec_pretty(&r)?)?;
    println!("PASS fixed CLI runtime election, live owner protection and killed-owner recovery");
    Ok(())
}
pub(super) fn helper() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    ensure!(args.len() == 4, "startup helper arguments");
    let root = PathBuf::from(&args[2]);
    let mode = args[3]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("startup mode encoding"))?;
    ensure!(
        ["a", "b", "live", "crash", "recover"].contains(&mode),
        "startup fixed mode"
    );
    let uid = validate_root(&root)?;
    let lock_path = root.join("election.lock");
    let metadata = std::fs::symlink_metadata(&lock_path)?;
    ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == uid
            && metadata.nlink() == 1
            && metadata.permissions().mode() & 0o777 == 0o600,
        "unsafe startup lock"
    );
    let lock = OpenOptions::new().read(true).write(true).open(&lock_path)?;
    let primary = match lock.try_lock() {
        Ok(()) => true,
        Err(std::fs::TryLockError::WouldBlock) => false,
        Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
    };
    println!("{}", json!({"pid":std::process::id(),"primary":primary}));
    std::io::stdout().flush()?;
    if !primary {
        return Ok(());
    }
    clean_owned_endpoint(&root)?;
    let secret = Zeroizing::new(format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    ));
    let token = Zeroizing::new(<[u8; 64]>::try_from(secret.as_bytes())?);
    let path = root.join("token");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(token.as_slice())?;
    file.sync_all()?;
    drop(file);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let socket = root.join("rpc.sock");
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        std::fs::write(root.join("ready"), b"ready")?;
        let calls = if mode == "recover" { 1 } else { 2 };
        let (mut admitted, mut attempts, mut denied) = (0, 0, 0);
        while admitted < calls {
            ensure!(attempts < 4, "startup attempt budget exhausted");
            attempts += 1;
            let (stream, _) =
                tokio::time::timeout(Duration::from_secs(5), listener.accept()).await??;
            match tokio::time::timeout(
                Duration::from_secs(1),
                serve(stream, uid, Zeroizing::new(*token)),
            )
            .await?
            {
                Ok("ping") => admitted += 1,
                Err(error) => {
                    ensure!(
                        reason(error)? == "authentication",
                        "unexpected startup rejection"
                    );
                    denied += 1;
                }
                _ => anyhow::bail!("startup fixed ping required"),
            }
        }
        std::fs::write(root.join("denials"), denied.to_string())?;
        std::fs::write(root.join("served"), b"served")?;
        wait_file(&root.join("release")).await?;
        drop(listener);
        clean_owned_endpoint(&root)?;
        Ok::<_, anyhow::Error>(())
    })?;
    drop(lock);
    Ok(())
}
