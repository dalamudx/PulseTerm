//! Fixed Windows cold companion or authenticated warm transport; no stale endpoint repair.
use pulse_platform::windows::{
    LocalControlPresence, WorkspaceGui, local_control_presence, system_directory,
};
use pulseterm_desktop::cli_startup::StartupAction;
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
const DENIED: &str = "pulse.cold_denied";
type Result<T> = std::result::Result<T, &'static str>;

async fn presence(directory: &Path) -> Result<LocalControlPresence> {
    let directory = directory.to_path_buf();
    tokio::task::spawn_blocking(move || local_control_presence(&directory))
        .await
        .map_err(|_| DENIED)?
        .map_err(|_| DENIED)
}

pub(super) async fn dispatch(directory: &Path, method: &str, params: Value) -> Result<Value> {
    let (value, client) = dispatch_connected(directory, method, params).await?;
    drop(client);
    Ok(value)
}

pub(super) async fn dispatch_connected(
    directory: &Path,
    method: &str,
    params: Value,
) -> Result<(Value, pulse_app::cli_control::Client)> {
    if presence(directory).await? == LocalControlPresence::Present {
        let mut client = pulseterm_desktop::native_cli_windows::connect(directory)
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
    let mut child = tokio::task::spawn_blocking(move || {
        let executable = std::env::current_exe().map_err(|_| DENIED)?;
        let gui = WorkspaceGui::beside(&executable).map_err(|_| DENIED)?;
        let cwd = system_directory().map_err(|_| DENIED)?;
        gui.validate().map_err(|_| DENIED)?;
        // The image and its original directory remain pinned until CreateProcess completes.
        Command::new(gui.path())
            .args(["--startup-actions", &parcel])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| DENIED)
    })
    .await
    .map_err(|_| DENIED)??;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Some(status) = child.try_wait().map_err(|_| DENIED)?
                && !status.success()
            {
                return Err(DENIED);
            }
            // Publication may temporarily be partial. No action or authentication has been sent.
            if matches!(presence(directory).await, Ok(LocalControlPresence::Present)) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| DENIED)??;
    let mut client = pulseterm_desktop::native_cli_windows::connect(directory)
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
