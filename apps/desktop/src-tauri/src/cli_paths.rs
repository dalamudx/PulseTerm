//! Closed control endpoint selected by the backend, shared by the GUI and pulse.
use std::path::PathBuf;
/// Identifier of the fixed workspace configuration, checked again by the GUI before publication.
pub const APPLICATION_ID: &str = "dev.pulseterm.workspace-preview";
/// Resolve metadata only. Platform transport separately validates private ancestors, token and peer.
/// No override, endpoint creation, stale cleanup or authority is granted by this path.
pub fn directory() -> Result<PathBuf, &'static str> {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .map(|root| root.join("pulseterm-workspace-cli"))
            .ok_or("pulse.runtime_unavailable")
    }
    #[cfg(windows)]
    {
        pulse_platform::windows::local_application_data_directory()
            .map(|root| root.join(APPLICATION_ID).join("cli"))
            .map_err(|_| "pulse.runtime_unavailable")
    }
    #[cfg(target_os = "macos")]
    {
        pulse_platform::macos::local_control_root()
            .map(|root| root.join("pulseterm-workspace-cli"))
            .map_err(|_| "pulse.runtime_unavailable")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err("pulse.runtime_unavailable")
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn endpoint_identity_matches_the_fixed_workspace_configuration()
    -> Result<(), Box<dyn std::error::Error>> {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../workspace/tauri.conf.json"))?;
        assert_eq!(config["identifier"], super::APPLICATION_ID);
        Ok(())
    }
}
