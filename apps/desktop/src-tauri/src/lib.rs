//! Desktop adapter. Native dependencies are opt-in for headless core checks.

#[cfg(feature = "desktop")]
mod probe;

#[cfg(feature = "desktop")]
pub use probe::run;
