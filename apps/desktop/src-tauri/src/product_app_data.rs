//! Primary setup only: prepare OS-selected private storage before WebKit creates it.
//! No page path, SQLite open, permission repair, or asynchronous cancellation boundary.
use std::path::{Path, PathBuf};

pub(super) fn prepare(directory: &Path) -> Result<PathBuf, &'static str> {
    pulse_store::state_directory::prepare(directory).map_err(|_| "workspace.unavailable")
}

#[cfg(test)]
mod tests {
    use super::prepare;
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
    };

    #[test]
    fn fresh_data_is_private_before_webkit_creates_storage()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let directory = root.path().join("data").join("workspace");
        assert_eq!(prepare(&directory)?, directory);
        // Simulate SDK's ordinary create_dir_all: existing private mode is preserved.
        fs::create_dir_all(&directory)?;
        fs::DirBuilder::new()
            .mode(0o755)
            .create(directory.join("WebKitCache"))?;
        assert_eq!(fs::metadata(&directory)?.mode() & 0o7777, 0o700);
        assert_eq!(
            fs::metadata(directory.parent().ok_or("no parent")?)?.mode() & 0o7777,
            0o700
        );
        assert_eq!(prepare(&directory)?, directory);
        Ok(())
    }

    #[test]
    fn existing_unsafe_sdk_directory_is_rejected_without_repair()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let directory = root.path().join("workspace");
        fs::create_dir(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;
        let original = fs::metadata(&directory)?;
        fs::write(directory.join("existing"), b"do not modify")?;
        assert_eq!(prepare(&directory), Err("workspace.unavailable"));
        let after = fs::metadata(&directory)?;
        assert_eq!(
            (after.dev(), after.ino(), after.mode()),
            (original.dev(), original.ino(), original.mode())
        );
        assert_eq!(fs::read(directory.join("existing"))?, b"do not modify");
        Ok(())
    }

    #[test]
    fn symbolic_data_directory_cannot_redirect_sdk_storage()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let target = root.path().join("target");
        prepare(&target)?;
        let link = root.path().join("workspace");
        symlink(&target, &link)?;
        assert_eq!(prepare(&link), Err("workspace.unavailable"));
        assert!(fs::symlink_metadata(&link)?.file_type().is_symlink());
        assert_eq!(fs::metadata(target)?.mode() & 0o7777, 0o700);
        Ok(())
    }
}
