//! OS-selected runtime metadata, independent of shell environment overrides.
use crate::{DirectoryError, DirectoryResult};
use std::{os::unix::ffi::OsStringExt, path::PathBuf};

/// Read the current account's Darwin runtime root without using HOME, TMPDIR or XDG overrides.
/// Resolves OS-selected aliases (including /var) before private endpoint admission.
/// Metadata only: does not create directories or grant endpoint ownership or action authority.
pub fn local_control_root() -> DirectoryResult<PathBuf> {
    let native = PathBuf::from(std::ffi::OsString::from_vec(sys::runtime_path()?));
    if !native.is_absolute() {
        return Err(DirectoryError::Location);
    }
    let root = native
        .canonicalize()
        .map_err(|_| DirectoryError::Location)?;
    if !root.is_dir() {
        return Err(DirectoryError::Location);
    }
    Ok(root)
}

#[allow(unsafe_code)]
mod sys {
    use crate::{DirectoryError, DirectoryResult};
    use std::ffi::{c_char, c_int};

    // Darwin unistd.h: OS per-account temporary root, not getenv("TMPDIR").
    const CS_DARWIN_USER_TEMP_DIR: c_int = 65537;
    unsafe extern "C" {
        fn confstr(name: c_int, buffer: *mut c_char, length: usize) -> usize;
    }

    pub(super) fn runtime_path() -> DirectoryResult<Vec<u8>> {
        let mut bytes = vec![0_u8; 4096];
        // SAFETY: confstr receives a writable allocation with its exact capacity. The fixed
        // selector takes no variadic arguments; its returned size includes the terminating NUL.
        let length = unsafe {
            confstr(
                CS_DARWIN_USER_TEMP_DIR,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if length < 2 || length > bytes.len() || bytes[length - 1] != 0 {
            return Err(DirectoryError::Location);
        }
        bytes.truncate(length - 1);
        if bytes.contains(&0) {
            return Err(DirectoryError::Location);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_root_matches_darwin_getconf() -> Result<(), Box<dyn std::error::Error>> {
        let expected = std::process::Command::new("/usr/bin/getconf")
            .arg("DARWIN_USER_TEMP_DIR")
            .output()?;
        assert!(expected.status.success());
        let native = String::from_utf8(expected.stdout)?;
        let expected = PathBuf::from(native.trim_end_matches('\n')).canonicalize()?;
        let actual = local_control_root()?;
        assert!(actual.is_absolute() && actual.is_dir());
        assert_eq!(actual, expected);
        Ok(())
    }
}
