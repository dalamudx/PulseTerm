//! Keep explicit Linux cwd stable through native chdir; portable-pty otherwise selects home.
use crate::PtyError;
use portable_pty::CommandBuilder;
use std::path::{Path, PathBuf};
pub(super) struct PreparedCwd {
    path: Option<PathBuf>,
    #[cfg(target_os = "linux")]
    _directory: Option<std::fs::File>,
}
impl PreparedCwd {
    pub(super) fn new(cwd: Option<&Path>) -> Result<Self, PtyError> {
        #[cfg(target_os = "linux")]
        {
            use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
            let directory = cwd
                .map(|path| {
                    std::fs::OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_PATH | libc::O_DIRECTORY)
                        .open(path)
                })
                .transpose()
                .map_err(|_| PtyError::WorkingDirectory)?;
            let path = directory
                .as_ref()
                .map(|file| PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())));
            // A missing/restricted procfs must reject, never activate portable-pty's home fallback.
            if path.as_ref().is_some_and(|path| !path.is_dir()) {
                return Err(PtyError::WorkingDirectory);
            }
            Ok(Self {
                path,
                _directory: directory,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            // Other platforms have preflight only; their native race acceptance is outstanding.
            if cwd.is_some_and(|path| !path.is_dir()) {
                return Err(PtyError::WorkingDirectory);
            }
            Ok(Self {
                path: cwd.map(Path::to_path_buf),
            })
        }
    }
    pub(super) fn apply(&self, builder: &mut CommandBuilder) {
        if let Some(path) = &self.path {
            builder.cwd(path);
        }
    }
}
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn explicit_missing_or_file_cwd_never_selects_home() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("missing");
        assert!(PreparedCwd::new(Some(&missing)).is_err());
        let file = temp.path().join("file");
        std::fs::write(&file, b"not a directory")?;
        assert!(PreparedCwd::new(Some(&file)).is_err());
        let inherited = PreparedCwd::new(None)?;
        assert!(inherited.path.is_none());
        Ok(())
    }
    #[test]
    fn linux_native_chdir_uses_opened_directory_after_path_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let original = temp.path().join("cwd");
        std::fs::create_dir(&original)?;
        let cwd = PreparedCwd::new(Some(&original))?;
        let moved = temp.path().join("renamed");
        std::fs::rename(&original, &moved)?;
        std::fs::create_dir(&original)?;
        let output = std::process::Command::new("/bin/pwd")
            .current_dir(cwd.path.as_ref().ok_or("cwd")?)
            .output()?;
        assert!(output.status.success());
        let expected = format!("{}\n", moved.display());
        assert_eq!(output.stdout, expected.as_bytes());
        Ok(())
    }
}
