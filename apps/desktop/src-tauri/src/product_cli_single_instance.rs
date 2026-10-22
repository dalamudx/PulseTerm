//! Native single-instance adapter. D-Bus forwarding is not action admission.
//! Upstream may continue after some D-Bus setup errors; verify actual owner PID
//! before creating a window or CLI endpoint. Never log forwarded argv or cwd.
use super::{LABEL, Workspace, local_workspace};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Wry, plugin::TauriPlugin};

pub(super) fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_single_instance::init(|app, args, _cwd| {
        if args.len() != 1 {
            if !cfg!(target_os = "linux") {
                return;
            }
            let accepted = (|| {
                let action = crate::cli_startup::from_args(
                    args.into_iter().skip(1).map(std::ffi::OsString::from),
                )
                .ok()??;
                if !action.supports_cold_readonly_transport() {
                    return None;
                }
                let managed = app.try_state::<Workspace>()?;
                if let Some(window) = app.get_webview_window(LABEL) {
                    if !local_workspace(&window) {
                        return None;
                    }
                    managed
                        .0
                        .bootstrap
                        .enqueue_current(action, &managed.0.documents, managed.0.owner)
                        .ok()?;
                } else {
                    // Root has not created its first window. Only that first
                    // document can satisfy this metadata wait; no scope yet.
                    managed.0.bootstrap.enqueue_first(action).ok()?;
                }
                Some(())
            })();
            if accepted.is_none() {
                eprintln!("workspace.single_instance_action_unavailable");
            }
            // No effect here. The authenticated original CLI must claim its id.
            return;
        }
        let Some(managed) = app.try_state::<Workspace>() else {
            return;
        };
        let state = managed.inner().clone();
        let Ok(original) = state.0.documents.current_native(state.0.owner) else {
            return;
        };
        let gui = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Ok(current) = state.0.documents.current_native(state.0.owner) else {
                return;
            };
            if !Arc::ptr_eq(&current, &original) {
                return;
            }
            let Some(window) = gui.get_webview_window(LABEL) else {
                return;
            };
            if !local_workspace(&window) {
                return;
            }
            if window.show().and_then(|()| window.set_focus()).is_ok() {
                eprintln!("workspace.single_instance_focus_requested");
            }
        });
    })
}
#[cfg(target_os = "linux")]
pub(super) fn verify_owner(app: &AppHandle) -> Result<(), &'static str> {
    let name = format!("{}.SingleInstance", app.config().identifier);
    verify_name(&name, std::process::id())
}
#[cfg(windows)]
pub(super) fn verify_owner(app: &AppHandle) -> Result<(), &'static str> {
    pulse_platform::windows::verify_single_instance_owner(&app.config().identifier)
        .map_err(|_| "workspace.single_instance_denied")
}
#[cfg(target_os = "linux")]
fn verify_name(name: &str, expected_pid: u32) -> Result<(), &'static str> {
    if expected_pid == 0 {
        return Err("workspace.single_instance_denied");
    }
    let connection = zbus::blocking::connection::Builder::session()
        .map_err(|_| "workspace.single_instance_denied")?
        .method_timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|_| "workspace.single_instance_denied")?;
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixProcessID",
            &name,
        )
        .map_err(|_| "workspace.single_instance_denied")?;
    let pid: u32 = reply
        .body()
        .deserialize()
        .map_err(|_| "workspace.single_instance_denied")?;
    if pid != expected_pid {
        return Err("workspace.single_instance_denied");
    }
    Ok(())
}
#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn single_instance_owner_proof_uses_a_private_broker() -> Result<(), Box<dyn std::error::Error>>
    {
        let output = std::process::Command::new("dbus-run-session")
            .arg("--").arg(std::env::current_exe()?)
            .args(["--exact", "product_workspace::single_instance::tests::single_instance_owner_proof_requires_real_name_pid_and_lifetime", "--ignored", "--nocapture"])
            .output()?;
        let stdout = std::str::from_utf8(&output.stdout)?;
        assert!(
            output.status.success()
                && stdout.contains("running 1 test")
                && stdout.contains("1 passed"),
            "private broker proof child did not pass exactly one test"
        );
        Ok(())
    }
    // This helper is actually executed by the parent; zero-test children cannot pass.
    #[test]
    #[ignore = "launched under a private broker by the parent test"]
    fn single_instance_owner_proof_requires_real_name_pid_and_lifetime()
    -> Result<(), Box<dyn std::error::Error>> {
        let name = format!(
            "dev.pulseterm.Ownership_{}.SingleInstance",
            uuid::Uuid::new_v4().simple()
        );
        assert!(verify_name(&name, std::process::id()).is_err());
        let owner = zbus::blocking::Connection::session()?;
        owner.request_name(name.as_str())?;
        verify_name(&name, std::process::id())?;
        assert!(verify_name(&name, 0).is_err());
        let wrong_pid = std::process::id()
            .checked_add(1)
            .ok_or("test.pid_overflow")?;
        assert!(verify_name(&name, wrong_pid).is_err());
        owner.release_name(name.as_str())?;
        assert!(verify_name(&name, std::process::id()).is_err());
        Ok(())
    }
}
