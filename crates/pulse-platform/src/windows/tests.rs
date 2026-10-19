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
fn held_parent_and_leaf_cannot_be_renamed() -> TestResult {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("parent");
    let first = PrivateDirectory::create(&parent)?;
    let path = parent.join("private");
    let guard = PrivateDirectory::create(&path)?;
    assert!(std::fs::rename(&path, parent.join("moved")).is_err());
    assert!(std::fs::rename(&parent, root.path().join("moved-parent")).is_err());
    guard.validate()?;
    first.validate()?;
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
