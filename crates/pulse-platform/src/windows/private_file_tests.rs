use super::{PrivateDirectory, PrivateFileError, sys};
use std::io::Write;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn exact_bytes_reopen_and_read_lease_prevents_write_and_rename() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let data = b"private\0\xff\n";
    let mut created = directory.create_file("settings.toml", data, 64)?;
    assert_eq!(created.read_bounded(64)?, data);
    assert_eq!(created.read_bounded(64)?, data);
    drop(created);
    let mut reader = directory.open_file("settings.toml")?;
    assert!(std::fs::write(path.join("settings.toml"), b"replacement").is_err());
    assert!(std::fs::rename(path.join("settings.toml"), path.join("moved")).is_err());
    assert_eq!(reader.read_bounded(64)?, data);
    Ok(())
}
#[test]
fn new_creation_never_truncates_existing_bytes_or_acl() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("settings.toml", b"original", 64)?);
    let before = sys::inspect(&sys::open_regular(&path.join("settings.toml"), None)?)?;
    assert!(matches!(
        directory.create_file("settings.toml", b"changed", 64),
        Err(PrivateFileError::Conflict)
    ));
    let after = sys::inspect(&sys::open_regular(&path.join("settings.toml"), None)?)?;
    assert!(
        before.identity == after.identity && before.owner == after.owner && before.acl == after.acl
    );
    assert_eq!(std::fs::read(path.join("settings.toml"))?, b"original");
    Ok(())
}
#[test]
fn oversized_or_invalid_names_never_create_and_reads_stay_bounded() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    assert!(matches!(
        directory.create_file("too-big", b"12", 1),
        Err(PrivateFileError::Limit)
    ));
    assert!(!path.join("too-big").exists());
    assert!(matches!(
        directory.open_file("absent"),
        Err(PrivateFileError::NotFound)
    ));
    for name in ["..", "child\\file", "file:stream", "NUL", "CON .txt"] {
        assert!(matches!(
            directory.create_file(name, b"x", 1),
            Err(PrivateFileError::Location)
        ));
    }
    let mut file = directory.create_file("settings.toml", b"12", 2)?;
    assert!(matches!(file.read_bounded(1), Err(PrivateFileError::Limit)));
    assert!(matches!(
        file.read_bounded(usize::MAX),
        Err(PrivateFileError::Limit)
    ));
    assert_eq!(file.read_bounded(2)?, b"12");
    Ok(())
}
#[test]
fn hard_links_are_rejected_without_changing_the_shared_file() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("settings.toml", b"original", 64)?);
    std::fs::hard_link(path.join("settings.toml"), path.join("alias"))?;
    for name in ["settings.toml", "alias"] {
        assert!(matches!(
            directory.open_file(name),
            Err(PrivateFileError::Location)
        ));
    }
    assert_eq!(std::fs::read(path.join("alias"))?, b"original");
    Ok(())
}
#[test]
fn readonly_intent_and_broad_file_acl_are_rejected_without_repair() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("readonly", b"original", 64)?);
    let original_permissions = std::fs::metadata(path.join("readonly"))?.permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(path.join("readonly"), permissions)?;
    assert!(matches!(
        directory.open_file("readonly"),
        Err(PrivateFileError::Location)
    ));
    assert!(
        std::fs::metadata(path.join("readonly"))?
            .permissions()
            .readonly()
    );
    std::fs::set_permissions(path.join("readonly"), original_permissions)?;
    sys::create_broad_file_fixture(&path.join("broad"))?;
    let before = sys::inspect(&sys::open_regular(&path.join("broad"), None)?)?;
    assert!(matches!(
        directory.open_file("broad"),
        Err(PrivateFileError::Location)
    ));
    let after = sys::inspect(&sys::open_regular(&path.join("broad"), None)?)?;
    assert!(before.owner == after.owner && before.acl == after.acl);
    Ok(())
}
#[test]
fn failed_new_file_is_deleted_by_its_original_handle() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let mut file = sys::open_regular(&path.join("partial"), Some(&directory.user))?;
    file.write_all(b"partial fixture")?;
    sys::discard_file(&file)?;
    drop(file);
    assert!(!path.join("partial").exists());
    directory.validate()?;
    Ok(())
}
#[test]
fn final_symlink_and_directory_are_never_followed() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("target", b"original", 64)?);
    // A runner without symlink creation privilege fails this prerequisite instead of silently skipping it.
    std::os::windows::fs::symlink_file(path.join("target"), path.join("link"))?;
    assert!(matches!(
        directory.open_file("link"),
        Err(PrivateFileError::Location)
    ));
    std::fs::create_dir(path.join("folder"))?;
    assert!(matches!(
        directory.open_file("folder"),
        Err(PrivateFileError::Location)
    ));
    assert_eq!(std::fs::read(path.join("target"))?, b"original");
    Ok(())
}
