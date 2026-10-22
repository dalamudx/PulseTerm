use super::*;

fn fixture() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("bin");
    let _directory = PrivateDirectory::prepare(&path)?;
    Ok((temp, path))
}
fn image(parent: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let directory = PrivateDirectory::open(parent)?;
    drop(directory.create_file(
        "pulseterm-workspace.exe",
        b"metadata fixture; never executed",
        64,
    )?);
    Ok(parent.join("pulseterm-workspace.exe"))
}

#[test]
fn fixed_sibling_is_pinned_until_lease_release() -> Result<(), Box<dyn std::error::Error>> {
    let (_temp, parent) = fixture()?;
    let path = image(&parent)?;
    let gui = WorkspaceGui::beside(&parent.join("pulse.exe"))?;
    assert_eq!(gui.path(), path);
    gui.validate()?;
    let renamed = parent.join("renamed.exe");
    assert!(std::fs::rename(&path, &renamed).is_err());
    assert!(std::fs::remove_file(&path).is_err());
    assert!(std::fs::write(&path, b"changed").is_err());
    drop(gui);
    std::fs::rename(path, renamed)?;
    Ok(())
}

#[test]
fn missing_directory_and_hardlinked_candidates_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let (_temp, parent) = fixture()?;
    let cli = parent.join("pulse.exe");
    assert!(WorkspaceGui::beside(&cli).is_err());
    let path = parent.join("pulseterm-workspace.exe");
    std::fs::create_dir(&path)?;
    assert!(WorkspaceGui::beside(&cli).is_err());
    std::fs::remove_dir(&path)?;
    let path = image(&parent)?;
    std::fs::hard_link(&path, parent.join("alias.exe"))?;
    assert!(WorkspaceGui::beside(&cli).is_err());
    assert!(WorkspaceGui::beside(Path::new("relative/pulse.exe")).is_err());
    Ok(())
}

#[test]
fn read_only_image_remains_admissible() -> Result<(), Box<dyn std::error::Error>> {
    let (_temp, parent) = fixture()?;
    let path = image(&parent)?;
    let original = std::fs::metadata(&path)?.permissions();
    let mut read_only = original.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&path, read_only)?;
    let result = (|| {
        let gui = WorkspaceGui::beside(&parent.join("pulse.exe"))?;
        gui.validate()
    })();
    std::fs::set_permissions(&path, original)?;
    result?;
    Ok(())
}

#[test]
fn endpoint_observation_does_not_create_or_repair_missing_and_partial_namespaces()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, _) = fixture()?;
    let parent = temp.path().join("empty");
    assert!(matches!(
        local_control_presence(&parent),
        Ok(LocalControlPresence::Absent)
    ));
    assert!(!parent.exists());
    let directory = PrivateDirectory::prepare(&parent)?;
    assert!(matches!(
        local_control_presence(&parent),
        Ok(LocalControlPresence::Absent)
    ));
    drop(directory.create_file("control.token", b"partial", 64)?);
    assert!(local_control_presence(&parent).is_err());
    assert_eq!(std::fs::read(parent.join("control.token"))?, b"partial");
    assert!(!parent.join("control.sock").exists());
    assert!(local_control_presence(Path::new("relative/cli")).is_err());
    Ok(())
}
