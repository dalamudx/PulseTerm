use super::prepare;
use pulse_platform::windows::PrivateDirectory;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn production_preparation_creates_nested_private_directories_and_reopens() -> TestResult {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("data");
    let directory = parent.join("workspace");
    assert_eq!(prepare(&directory)?, directory);
    PrivateDirectory::open(&parent)?.validate()?;
    PrivateDirectory::open(&directory)?.validate()?;
    assert_eq!(prepare(&directory)?, directory);
    Ok(())
}

#[test]
fn production_preparation_rejects_files_and_relative_paths_without_modifying_them() -> TestResult {
    let root = tempfile::tempdir()?;
    let file = root.path().join("file");
    std::fs::write(&file, b"preserve existing bytes")?;
    assert_eq!(prepare(&file.join("child")), Err("workspace.unavailable"));
    assert_eq!(std::fs::read(&file)?, b"preserve existing bytes");
    assert_eq!(
        prepare(std::path::Path::new(r"relative\state")),
        Err("workspace.unavailable")
    );
    Ok(())
}
