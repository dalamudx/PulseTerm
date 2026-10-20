use super::{native::SettingsFile, *};
use pulse_platform::windows::PrivateDirectory;
use pulse_store::{
    settings::SettingsResolver, settings_edit::TerminalSettingEdit as Edit,
    settings_remove::TerminalSettingKey as Key,
};
use std::cell::Cell;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[test]
fn missing_create_comment_preserving_save_remove_and_reopen() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let file = SettingsFile::open_private(path.clone())?;
    let missing = file.load()?;
    assert!(!missing.exists());
    assert_eq!(missing.text(), "");
    let created =
        file.save_authorized(&missing, SettingsMutation::Set(Edit::FontSize(18)), || true)?;
    assert!(created.exists());
    assert_eq!(
        SettingsResolver::resolve(Some(created.text()), None, None)?
            .font_size
            .value,
        18
    );
    assert!(SettingsFile::open_private(path.clone())?.load()? == created);
    let removed =
        file.save_authorized(&created, SettingsMutation::Remove(Key::FontSize), || true)?;
    assert_eq!(
        SettingsResolver::resolve(Some(removed.text()), None, None)?
            .font_size
            .value,
        14
    );
    drop(file);
    drop(directory);
    std::fs::write(
        path.join("settings.toml"),
        b"# retained\n[terminal]\nfont_size = 14 # keep\ncursor_blink = false\n",
    )?;
    let file = SettingsFile::open_private(path)?;
    let original = file.load()?;
    let changed =
        file.save_authorized(&original, SettingsMutation::Set(Edit::FontSize(20)), || {
            true
        })?;
    assert!(changed.text().contains("# retained") && changed.text().contains("# keep"));
    assert!(
        !SettingsResolver::resolve(Some(changed.text()), None, None)?
            .cursor_blink
            .value
    );
    Ok(())
}
#[test]
fn stale_cross_directory_and_external_edits_conflict_without_reset() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("first");
    let other = root.path().join("other");
    let _first = PrivateDirectory::create(&path)?;
    let _other = PrivateDirectory::create(&other)?;
    let file = SettingsFile::open_private(path.clone())?;
    let foreign = SettingsFile::open_private(other)?;
    let original = file.load()?;
    assert!(matches!(
        foreign.save_authorized(&original, SettingsMutation::Set(Edit::FontSize(18)), || {
            true
        }),
        Err(Error::Location)
    ));
    let current =
        file.save_authorized(&original, SettingsMutation::Set(Edit::FontSize(18)), || {
            true
        })?;
    assert!(matches!(
        file.save_authorized(&original, SettingsMutation::Set(Edit::FontSize(20)), || {
            true
        }),
        Err(Error::Conflict)
    ));
    assert!(file.load()? == current);
    std::fs::write(path.join("settings.toml"), b"[terminal]\nfont_size=22\n")?;
    assert!(matches!(
        file.save_authorized(&current, SettingsMutation::Set(Edit::FontSize(20)), || true),
        Err(Error::Conflict)
    ));
    assert_eq!(
        SettingsResolver::resolve(Some(file.load()?.text()), None, None)?
            .font_size
            .value,
        22
    );
    Ok(())
}
#[test]
fn malformed_utf8_toml_and_oversized_files_never_load_as_defaults() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("settings.toml", b"", 0)?);
    let file = SettingsFile::open_private(path.clone())?;
    for bytes in [
        vec![0xff],
        b"[terminal".to_vec(),
        b"schema_version=2".to_vec(),
        vec![b'#'; SETTINGS_TEXT_LIMIT + 1],
    ] {
        std::fs::write(path.join("settings.toml"), &bytes)?;
        assert!(matches!(file.load(), Err(Error::Document(_))));
        assert_eq!(std::fs::read(path.join("settings.toml"))?, bytes);
    }
    Ok(())
}
#[test]
fn four_authorization_checks_revoke_and_reclaim_staging() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let file = SettingsFile::open_private(path.clone())?;
    let original = file.load()?;
    for boundary in 1..=4 {
        let calls = Cell::new(0);
        let result =
            file.save_authorized(&original, SettingsMutation::Set(Edit::FontSize(18)), || {
                calls.set(calls.get() + 1);
                calls.get() != boundary
            });
        assert!(matches!(result, Err(Error::Authorization)));
        assert_eq!(calls.get(), boundary);
        assert!(file.load()? == original);
        for entry in std::fs::read_dir(&path)? {
            assert!(
                !entry?
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".pulse-atomic-")
            );
        }
    }
    directory.validate()?;
    Ok(())
}
#[test]
fn panicking_authorization_after_staging_preserves_original() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let _directory = PrivateDirectory::create(&path)?;
    let file = SettingsFile::open_private(path)?;
    let original = file.load()?;
    let calls = Cell::new(0);
    let result = file.save_authorized(&original, SettingsMutation::Set(Edit::FontSize(18)), || {
        calls.set(calls.get() + 1);
        assert!(calls.get() != 3, "fixed authorization fixture");
        true
    });
    assert!(matches!(result, Err(Error::Authorization)));
    assert!(file.load()? == original);
    Ok(())
}
#[test]
fn lock_busy_and_commit_uncertainty_remain_distinct_errors() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let file = SettingsFile::open_private(path)?;
    let absent = file.load()?;
    let original =
        file.save_authorized(&absent, SettingsMutation::Set(Edit::FontSize(18)), || true)?;
    let lock = directory.open_file(".pulse-write.lock")?;
    assert!(matches!(file.load(), Err(Error::Busy)));
    drop(lock);
    let target = directory.open_file("settings.toml")?;
    assert!(matches!(
        file.save_authorized(&original, SettingsMutation::Set(Edit::FontSize(20)), || {
            true
        }),
        Err(Error::Uncertain)
    ));
    drop(target);
    assert!(file.load()? == original);
    Ok(())
}
#[test]
fn invalid_mutation_never_modifies_file() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let _directory = PrivateDirectory::create(&path)?;
    let file = SettingsFile::open_private(path)?;
    let original = file.load()?;
    assert!(matches!(
        file.save_authorized(&original, SettingsMutation::Set(Edit::FontSize(0)), || true),
        Err(Error::Document(SettingsError::Value))
    ));
    assert!(file.load()? == original);
    Ok(())
}
