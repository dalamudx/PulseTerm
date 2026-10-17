//! Closed Linux cold launcher. Fixed GUI sibling, no client path or shell.
//! Endpoint observation may repeat, action/claim invocation may not. Correlation
//! does not replace token authentication, original document or Capability checks.
use pulseterm_desktop::cli_startup::StartupAction;
use serde_json::{Value, json};
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
type Result<T> = std::result::Result<T, &'static str>;
const DENIED: &str = "pulse.cold_denied";
#[derive(PartialEq, Eq)]
enum Endpoint {
    Missing,
    Present,
}
fn ancestors(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(DENIED);
    }
    let uid = rustix::process::geteuid().as_raw();
    let mut current = PathBuf::new();
    for component in path.components() {
        if !matches!(component, Component::RootDir | Component::Normal(_)) {
            return Err(DENIED);
        }
        current.push(component.as_os_str());
        let meta = fs::symlink_metadata(&current).map_err(|_| DENIED)?;
        if !meta.is_dir()
            || (meta.uid() != uid && meta.uid() != 0)
            || (meta.mode() & 0o022 != 0 && !(meta.uid() == 0 && meta.mode() & 0o1777 == 0o1777))
        {
            return Err(DENIED);
        }
    }
    Ok(())
}
fn private_directory(path: &Path) -> Result<()> {
    ancestors(path)?;
    let meta = fs::symlink_metadata(path).map_err(|_| DENIED)?;
    if meta.uid() != rustix::process::geteuid().as_raw() || meta.mode() & 0o7777 != 0o700 {
        return Err(DENIED);
    }
    Ok(())
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(DENIED),
    }
}
fn endpoint(directory: &Path) -> Result<Endpoint> {
    private_directory(directory.parent().ok_or(DENIED)?)?;
    if !exists(directory)? {
        return Ok(Endpoint::Missing);
    }
    private_directory(directory)?;
    match (
        exists(&directory.join("control.sock"))?,
        exists(&directory.join("control.token"))?,
    ) {
        (false, false) => Ok(Endpoint::Missing),
        (true, true) => Ok(Endpoint::Present),
        _ => Err(DENIED), // Partial/stale/untrusted endpoints are never repaired.
    }
}
fn gui_sibling(executable: &Path) -> Result<PathBuf> {
    let parent = executable.parent().ok_or(DENIED)?;
    ancestors(parent)?;
    let gui = parent.join("pulseterm-workspace");
    let meta = fs::symlink_metadata(&gui).map_err(|_| DENIED)?;
    if !meta.is_file()
        || meta.nlink() != 1
        || (meta.uid() != 0 && meta.uid() != rustix::process::geteuid().as_raw())
        || meta.mode() & 0o022 != 0
        || meta.mode() & 0o111 == 0
        || meta.mode() & 0o6000 != 0
    {
        return Err(DENIED);
    }
    Ok(gui)
}
pub(super) async fn dispatch_connected(
    directory: &Path,
    method: &str,
    params: Value,
) -> Result<(Value, pulse_app::cli_control::Client)> {
    if endpoint(directory)? == Endpoint::Present {
        // Warm failure never retries via cold launch or another instance.
        let mut client = pulse_app::cli_control::Client::connect(directory)
            .await
            .map_err(|_| DENIED)?;
        let value = client.invoke(method, params).await.map_err(|_| DENIED)?;
        return Ok((value, client));
    }
    let action = StartupAction::new(method, &params).map_err(|_| DENIED)?;
    if !action.supports_cold_transport() {
        return Err(DENIED);
    }
    let parcel = action.encode().map_err(|_| DENIED)?;
    let gui = gui_sibling(&std::env::current_exe().map_err(|_| DENIED)?)?;
    // No shell, PATH lookup, client executable, cwd or credential parameter.
    // Single-instance plugin handles a simultaneous launch before window/setup.
    let mut child = Command::new(gui)
        .args(["--startup-actions", &parcel])
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| DENIED)?;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Some(status) = child.try_wait().map_err(|_| DENIED)?
                && !status.success()
            {
                return Err(DENIED);
            }
            // Readiness is metadata only. No authenticated action has been sent.
            // During backend publication a partial pair is not ready; the later
            // invoke still validates type, owner, mode, links, peer and token.
            if private_directory(directory).is_ok()
                && exists(&directory.join("control.sock"))?
                && exists(&directory.join("control.token"))?
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| DENIED)??;
    // Claim on the same authenticated transport used for later chain steps.
    let mut client = pulse_app::cli_control::Client::connect(directory)
        .await
        .map_err(|_| DENIED)?;
    let value = client
        .invoke(
            "workspace.bootstrap",
            json!({"startup":action.request_id()}),
        )
        .await
        .map_err(|_| DENIED)?;
    Ok((value, client))
}
pub(super) async fn dispatch(directory: &Path, method: &str, params: Value) -> Result<Value> {
    if endpoint(directory)? == Endpoint::Present {
        return pulse_app::cli_control::invoke(directory, method, params)
            .await
            .map_err(|_| DENIED);
    }
    dispatch_connected(directory, method, params)
        .await
        .map(|(value, _)| value)
}
#[cfg(test)]
#[path = "pulse_cold_tests.rs"]
mod tests;
