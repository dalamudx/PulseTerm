//! Desktop adapter. Native dependencies are opt-in for headless core checks.

#[cfg(feature = "desktop")]
mod probe;
#[cfg(feature = "desktop")]
mod raw_frame;
#[cfg(feature = "desktop")]
mod window_sessions;

#[cfg(feature = "desktop")]
pub use probe::run;
