use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn protected_creation_reopen_and_original_handle_validation() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let guard = PrivateDirectory::create(&path)?;
    guard.validate()?;
    let reopened = PrivateDirectory::open(&path)?;
    reopened.validate()?;
    Ok(())
}
#[test]
fn prepare_creates_private_components_and_pins_the_entire_chain() -> TestResult {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("data");
    let path = parent.join("workspace");
    let guard = PrivateDirectory::prepare(&path)?;
    guard.validate()?;
    PrivateDirectory::open(&parent)?.validate()?;
    PrivateDirectory::prepare(&path)?.validate()?;
    assert!(std::fs::rename(&parent, root.path().join("moved")).is_err());
    drop(guard);
    std::fs::rename(&parent, root.path().join("moved"))?;
    Ok(())
}
#[test]
fn prepare_rejects_existing_broad_leaf_and_file_ancestors_without_repair() -> TestResult {
    let root = tempfile::tempdir()?;
    let broad = root.path().join("broad");
    sys::create_broad_fixture(&broad, false)?;
    let before = sys::inspect(&open(&broad)?)?;
    assert!(matches!(PrivateDirectory::prepare(&broad), Err(Location)));
    let after = sys::inspect(&open(&broad)?)?;
    assert!(before.owner == after.owner && before.acl == after.acl);
    let file = root.path().join("file");
    std::fs::write(&file, b"unchanged")?;
    assert!(matches!(
        PrivateDirectory::prepare(file.join("child")),
        Err(Location)
    ));
    assert_eq!(std::fs::read(file)?, b"unchanged");
    assert!(!broad.join("child").exists());
    Ok(())
}
#[test]
fn held_parent_and_leaf_cannot_be_renamed() -> TestResult {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("parent");
    let first = PrivateDirectory::create(&parent)?;
    let path = parent.join("private");
    let guard = PrivateDirectory::create(&path)?;
    assert!(std::fs::rename(&path, parent.join("moved")).is_err());
    assert!(std::fs::rename(&parent, root.path().join("moved-parent")).is_err());
    assert!(std::fs::remove_dir(&path).is_err());
    guard.validate()?;
    first.validate()?;

    drop(guard);
    std::fs::rename(&path, parent.join("moved"))?;
    first.validate()?;
    drop(first);
    std::fs::rename(&parent, root.path().join("moved-parent"))?;
    Ok(())
}
#[test]
fn existing_broad_acl_is_rejected_without_repair() -> TestResult {
    let root = tempfile::tempdir()?;
    for (name, inherit_only) in [("broad", false), ("broad-inheritance", true)] {
        let path = root.path().join(name);
        sys::create_broad_fixture(&path, inherit_only)?;
        let before = sys::inspect(&open(&path)?)?;
        assert!(matches!(PrivateDirectory::open(&path), Err(Location)));
        assert!(matches!(PrivateDirectory::create(&path), Err(Location)));
        let after = sys::inspect(&open(&path)?)?;
        assert!(before.owner == after.owner && before.acl == after.acl);
    }
    Ok(())
}
#[test]
fn regular_file_and_missing_parents_reject() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("file");
    std::fs::write(&path, b"fixed fixture")?;
    assert!(matches!(PrivateDirectory::open(&path), Err(Location)));
    assert!(matches!(
        PrivateDirectory::create(root.path().join("missing").join("child")),
        Err(Location)
    ));
    Ok(())
}
#[test]
fn remote_device_relative_stream_and_normalization_aliases_reject() {
    for p in [
        r"relative\state",
        r"C:state",
        r"\\server\share\state",
        r"\\.\pipe\state",
        r"C:\state\..\private",
        r"C:\state\.\private",
        r"C:\state:stream",
        r"C:\CON.txt",
    ] {
        assert!(
            matches!(PrivateDirectory::open(p), Err(Location)),
            "unexpected admission"
        );
    }
}
