//! Native filesystem and restricted web handoff primitives; no page or credential authority.
#![deny(unsafe_code)]

#[cfg(any(windows, test))]
mod file_state;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(any(windows, test))]
mod policy;
#[cfg(any(windows, target_os = "macos", test))]
mod web_link;
#[cfg(windows)]
pub mod windows;

/// Stable admission errors never include paths, account identifiers or native diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DirectoryError {
    /// The location, ownership, ACL or volume cannot satisfy the private-directory policy.
    #[error("platform.directory.private_location_required")]
    Location,
}

/// Result of a native private-directory admission, not application authorization.
pub type DirectoryResult<T> = Result<T, DirectoryError>;
