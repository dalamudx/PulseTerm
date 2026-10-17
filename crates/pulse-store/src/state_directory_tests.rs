use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[test]
fn creates_private_directory_and_reuses_it() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("new").join("state");
    assert_eq!(prepare(&path)?, path);
    assert_eq!(prepare(&path)?, path);
    assert_eq!(
        fs::symlink_metadata(&path)?.permissions().mode() & 0o7777,
        0o700
    );
    Ok(())
}

#[test]
fn rejects_relative_or_root_paths() -> TestResult {
    assert!(matches!(
        prepare(Path::new("relative")),
        Err(StoreError::Location)
    ));
    assert!(matches!(prepare(Path::new("/")), Err(StoreError::Location)));
    Ok(())
}

#[test]
fn refuses_symlink_at_any_component_without_modifying_target() -> TestResult {
    let root = tempfile::tempdir()?;
    let target = root.path().join("real");
    fs::create_dir(&target)?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
    symlink(&target, root.path().join("alias"))?;
    assert!(matches!(
        prepare(&root.path().join("alias").join("state")),
        Err(StoreError::Location)
    ));
    assert!(matches!(
        prepare(&root.path().join("alias")),
        Err(StoreError::Location)
    ));
    assert!(!target.join("state").exists());
    Ok(())
}

#[test]
fn refuses_world_writable_ancestor_or_existing_public_final_without_chmod() -> TestResult {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("unsafe");
    fs::create_dir(&parent)?;
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777))?;
    assert!(matches!(
        prepare(&parent.join("state")),
        Err(StoreError::Location)
    ));
    assert!(!parent.join("state").exists());
    let final_dir = root.path().join("public");
    fs::create_dir(&final_dir)?;
    fs::set_permissions(&final_dir, fs::Permissions::from_mode(0o755))?;
    assert!(matches!(prepare(&final_dir), Err(StoreError::Location)));
    assert_eq!(
        fs::symlink_metadata(final_dir)?.permissions().mode() & 0o777,
        0o755
    );
    Ok(())
}

#[test]
fn refuses_parent_traversal_before_creating_components() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("must-not-create").join("..").join("state");
    assert!(matches!(prepare(&path), Err(StoreError::Location)));
    assert!(!root.path().join("must-not-create").exists());
    assert!(!root.path().join("state").exists());
    Ok(())
}

#[test]
fn root_does_not_override_read_only_parent_intent() -> TestResult {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("readonly");
    fs::create_dir(&parent)?;
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o500))?;
    assert!(matches!(
        prepare(&parent.join("state")),
        Err(StoreError::Location)
    ));
    assert!(!parent.join("state").exists());
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[test]
fn rejects_file_in_directory_chain() -> TestResult {
    let root = tempfile::tempdir()?;
    let file = root.path().join("file");
    fs::write(&file, b"unchanged")?;
    assert!(matches!(
        prepare(&file.join("state")),
        Err(StoreError::Location)
    ));
    assert_eq!(fs::read(file)?, b"unchanged");
    Ok(())
}
