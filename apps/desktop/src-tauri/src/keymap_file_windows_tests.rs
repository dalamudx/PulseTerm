use super::*;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn fixture() -> std::result::Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("data");
    drop(PrivateDirectory::create(&path)?);
    Ok((root, path))
}
fn restore() -> KeymapMutation {
    KeymapMutation::Restore(KeymapAction::Settings)
}

#[test]
fn missing_roundtrip_and_confirmation_keep_exact_instance_and_source() -> TestResult {
    let (_root, path) = fixture()?;
    let file = KeymapFile::open_private(path.clone())?;
    let missing = file.load()?;
    missing.projection()?;
    file.confirm_current_authorized(&missing, || true)?;
    assert!(!path.join("keymap.toml").exists());
    let installed = file.save_authorized(&missing, restore(), || true)?;
    assert!(installed.document()?.bindings.is_empty());
    file.confirm_current_authorized(&installed, || true)?;
    let other = KeymapFile::open_private(path)?;
    assert!(matches!(
        other.confirm_current_authorized(&installed, || true),
        Err(KeymapFileError::Conflict)
    ));
    assert!(matches!(
        file.save_authorized(&missing, restore(), || true),
        Err(KeymapFileError::Conflict)
    ));
    Ok(())
}

#[test]
fn external_bytes_empty_files_and_hard_links_fail_without_repair() -> TestResult {
    let (_root, path) = fixture()?;
    let file = KeymapFile::open_private(path.clone())?;
    let current = file.save_authorized(&file.load()?, restore(), || true)?;
    let external = "schema_version = 1\nbindings = []\n# externally edited\n";
    std::fs::write(path.join("keymap.toml"), external)?;
    assert!(matches!(
        file.confirm_current_authorized(&current, || true),
        Err(KeymapFileError::Conflict)
    ));
    assert!(matches!(
        file.save_authorized(&current, restore(), || true),
        Err(KeymapFileError::Conflict)
    ));
    assert_eq!(std::fs::read_to_string(path.join("keymap.toml"))?, external);
    std::fs::write(path.join("keymap.toml"), "")?;
    assert!(matches!(file.load(), Err(KeymapFileError::Document(_))));
    std::fs::write(path.join("keymap.toml"), external)?;
    std::fs::hard_link(path.join("keymap.toml"), path.join("alias"))?;
    assert!(matches!(file.load(), Err(KeymapFileError::Location)));
    Ok(())
}

#[test]
fn revocation_and_callback_panic_preserve_missing_file_and_remove_staging() -> TestResult {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for boundary in 1..=4 {
        let (_root, path) = fixture()?;
        let file = KeymapFile::open_private(path.clone())?;
        let missing = file.load()?;
        let calls = AtomicUsize::new(0);
        assert!(matches!(
            file.save_authorized(&missing, restore(), || calls.fetch_add(1, Ordering::SeqCst)
                + 1
                != boundary),
            Err(KeymapFileError::Authorization)
        ));
        assert!(!path.join("keymap.toml").exists());
        assert!(std::fs::read_dir(&path)?.all(|e| e.is_ok_and(|e| {
            !e.file_name()
                .to_string_lossy()
                .starts_with(".pulse-atomic-")
        })));
        assert!(matches!(
            file.confirm_current_authorized(&missing, || panic!("revoked")),
            Err(KeymapFileError::Authorization)
        ));
        file.confirm_current_authorized(&missing, || true)?;
    }
    Ok(())
}

#[test]
fn typed_binding_is_resolved_and_roundtrips_without_executable_authority() -> TestResult {
    let (_root, path) = fixture()?;
    let file = KeymapFile::open_private(path.clone())?;
    let installed = file.save_authorized(
        &file.load()?,
        KeymapMutation::Set(KeymapOverride {
            action: KeymapAction::Settings,
            keys: Vec::new(),
            when: "workspaceFocus && !editableFocus && !modalOpen".into(),
        }),
        || true,
    )?;
    let patch = installed.document()?;
    assert_eq!(patch.bindings.len(), 1);
    assert_eq!(patch.bindings[0].action, KeymapAction::Settings);
    assert!(patch.bindings[0].keys.is_empty());
    installed.projection()?;
    assert_eq!(file.load()?.document()?, patch);
    Ok(())
}
