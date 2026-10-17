//! Desktop adapter. Native dependencies are opt-in for headless core checks.

#[cfg(all(feature = "workspace-cli", target_os = "linux"))]
pub mod cli_bootstrap;
#[cfg(all(feature = "workspace-cli", target_os = "linux"))]
pub mod cli_presentation;
#[cfg(all(feature = "workspace-cli", target_os = "linux"))]
pub mod cli_selection;
#[cfg(feature = "workspace-cli")]
pub mod cli_startup;
#[cfg(all(feature = "cli-window-probe", unix))]
mod cli_window_probe;
#[cfg(feature = "desktop")]
mod native_origin;
#[cfg(feature = "desktop")]
mod native_visibility;
#[cfg(feature = "desktop")]
mod probe;
#[cfg(feature = "workspace")]
mod product_workspace;
#[cfg(feature = "desktop")]
mod raw_frame;
#[cfg(all(feature = "workspace-ssh", unix))]
mod ssh_private_key_adapter;
#[cfg(all(feature = "workspace-ssh", unix))]
mod ssh_private_key_decode;
#[cfg(all(feature = "workspace-ssh", unix))]
mod ssh_session_adapter;
#[cfg(all(feature = "workspace-ssh", unix))]
mod ssh_terminal_io;
#[cfg(feature = "desktop")]
mod viewer_visibility;
#[cfg(feature = "desktop")]
mod window_sessions;

#[cfg(feature = "desktop")]
pub use probe::run;

/// Opt-in, separate product workbench; no validation command registry is mounted.
#[cfg(feature = "workspace")]
pub use product_workspace::run as run_workspace;
