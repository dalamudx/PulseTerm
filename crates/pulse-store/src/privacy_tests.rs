use super::*;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn dir() -> std::io::Result<tempfile::TempDir> {
    let d = tempfile::tempdir()?;
    fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700))?;
    Ok(d)
}
fn database(path: &Path) -> TestResult<Vec<u8>> {
    let c = super::database::open(path)?;
    drop(c);
    Ok(fs::read(path)?)
}
fn reject(path: &Path, before: &[u8]) -> TestResult {
    assert!(matches!(
        super::database::open(path),
        Err(StoreError::Location)
    ));
    assert_eq!(fs::read(path)?, before);
    Ok(())
}
struct RestoreFixtureMode {
    path: std::path::PathBuf,
    mode: u32,
}
impl Drop for RestoreFixtureMode {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}
#[test]
fn read_only_directory_and_database_are_refused_without_chmod_or_creation() -> TestResult {
    for mode in [0o500, 0] {
        let d = dir()?;
        let path = d.path().join("pulse.db");
        let restore = RestoreFixtureMode {
            path: d.path().to_path_buf(),
            mode: 0o700,
        };
        fs::set_permissions(d.path(), fs::Permissions::from_mode(mode))?;
        assert!(matches!(
            super::database::open(&path),
            Err(StoreError::Location)
        ));
        assert_eq!(fs::symlink_metadata(d.path())?.mode() & 0o777, mode);
        drop(restore);
        assert!(
            !path.exists(),
            "refusal must not create a file even when effective UID can bypass OS mode"
        );
    }
    let d = dir()?;
    let path = d.path().join("pulse.db");
    let before = database(&path)?;
    let restore = RestoreFixtureMode {
        path: path.clone(),
        mode: 0o600,
    };
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400))?;
    assert!(matches!(
        super::database::open(&path),
        Err(StoreError::Location)
    ));
    assert_eq!(fs::symlink_metadata(&path)?.mode() & 0o777, 0o400);
    drop(restore);
    assert_eq!(fs::read(path)?, before);
    Ok(())
}

#[test]
fn database_hardlink_is_rejected_without_mutating_either_name() -> TestResult {
    let d = dir()?;
    let path = d.path().join("pulse.db");
    let before = database(&path)?;
    let alias = d.path().join("alias.db");
    fs::hard_link(&path, &alias)?;
    assert_eq!(fs::symlink_metadata(&path)?.nlink(), 2);
    reject(&path, &before)?;
    assert_eq!(fs::read(alias)?, before);
    Ok(())
}
#[test]
fn sidecar_symlink_hardlink_nonprivate_modes_and_directories_are_rejected_without_touching_database()
-> TestResult {
    for suffix in ["-wal", "-shm", "-journal"] {
        for kind in ["symlink", "hardlink", "public", "directory"] {
            let d = dir()?;
            let path = d.path().join("pulse.db");
            let before = database(&path)?;
            let side = d.path().join(format!("pulse.db{suffix}"));
            let target = d.path().join("external-fixture");
            fs::write(&target, b"unchanged fixture")?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
            match kind {
                "symlink" => symlink(&target, &side)?,
                "hardlink" => fs::hard_link(&target, &side)?,
                "directory" => fs::create_dir(&side)?,
                _ => fs::write(&side, b"unchanged public fixture")?,
            }
            if kind == "public" {
                fs::set_permissions(&side, fs::Permissions::from_mode(0o644))?;
            }
            reject(&path, &before)?;
            assert_eq!(fs::read(&target)?, b"unchanged fixture");
            if kind == "public" {
                assert_eq!(fs::read(&side)?, b"unchanged public fixture");
            }
        }
    }
    Ok(())
}
#[test]
#[ignore = "Requires effective UID 0 to chown only private disposable test fixtures; no privilege escalation"]
fn root_foreign_owner_is_rejected_without_repairing_parent_database_or_sidecars() -> TestResult {
    let uid = rustix::process::geteuid();
    if !uid.is_root() {
        return Err("root-owned disposable fixture test requires effective UID 0".into());
    }
    let foreign = rustix::process::Uid::from_raw(65534);
    let d = dir()?;
    rustix::fs::chown(d.path(), Some(foreign), None)?;
    let path = d.path().join("pulse.db");
    assert!(matches!(
        super::database::open(&path),
        Err(StoreError::Location)
    ));
    assert!(!path.exists());
    assert_eq!(fs::symlink_metadata(d.path())?.uid(), foreign.as_raw());
    let d = dir()?;
    let path = d.path().join("pulse.db");
    let before = database(&path)?;
    rustix::fs::chown(&path, Some(foreign), None)?;
    reject(&path, &before)?;
    assert_eq!(fs::symlink_metadata(&path)?.uid(), foreign.as_raw());
    for suffix in ["-wal", "-shm", "-journal"] {
        let d = dir()?;
        let path = d.path().join("pulse.db");
        let before = database(&path)?;
        let side = d.path().join(format!("pulse.db{suffix}"));
        fs::write(&side, b"owned by foreign UID fixture")?;
        fs::set_permissions(&side, fs::Permissions::from_mode(0o600))?;
        rustix::fs::chown(&side, Some(foreign), None)?;
        reject(&path, &before)?;
        assert_eq!(fs::read(&side)?, b"owned by foreign UID fixture");
        assert_eq!(fs::symlink_metadata(&side)?.uid(), foreign.as_raw());
    }
    Ok(())
}
