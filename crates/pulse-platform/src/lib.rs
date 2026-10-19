//! Native filesystem primitives; no window, page, credential or execution authority.
#![deny(unsafe_code)]

#[cfg(any(windows, test))]
mod file_state;
#[cfg(any(windows, test))]
mod policy;
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
