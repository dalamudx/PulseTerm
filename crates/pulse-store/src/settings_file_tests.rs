use super::*;
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn directory() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}
fn write(dir: &Path, text: &[u8]) -> std::io::Result<()> {
    fs::write(dir.join(TARGET), text)?;
    fs::set_permissions(dir.join(TARGET), fs::Permissions::from_mode(0o600))
}
fn edit() -> SettingsMutation {
    SettingsMutation::Set(TerminalSettingEdit::FontSize(18))
}
#[test]
fn shared_open_description_does_not_keep_completed_operation_locked() -> TestResult {
    let dir = directory()?;
    let store = FileStore::open(dir.path().into())?;
    let guard = store.lock()?;
    let inherited = guard.0.try_clone()?;
    assert!(matches!(store.load(), Err(SettingsFileError::Busy)));
    drop(guard);
    assert!(store.load().is_ok());
    drop(inherited);
    Ok(())
}
#[test]
fn create_save_reopen_remove_and_conflict_preserve_original_text() -> TestResult {
    let dir = directory()?;
    let store = FileStore::open(dir.path().into())?;
    let missing = store.load()?;
    assert!(!missing.exists());
    let first = store.save(&missing, edit(), |_| Ok(()))?;
    assert!(first.exists());
    assert_eq!(first.text(), "[terminal]\nfont_size = 18\n");
    assert!(matches!(
        store.save(&missing, edit(), |_| Ok(())),
        Err(SettingsFileError::Conflict)
    ));
    let fresh = FileStore::open(dir.path().into())?.load()?;
    assert_eq!(fresh.text(), first.text());
    let removed = store.save(
        &fresh,
        SettingsMutation::Remove(TerminalSettingKey::FontSize),
        |_| Ok(()),
    )?;
    assert_eq!(removed.text(), "[terminal]\n\n");
    assert_eq!(fs::read_to_string(dir.path().join(TARGET))?, removed.text());
    Ok(())
}
#[test]
fn cooperating_handles_are_busy_and_old_inode_does_not_revive() -> TestResult {
    let dir = directory()?;
    write(dir.path(), b"# keep\nterminal.font_size=14 # original\n")?;
    let a = FileStore::open(dir.path().into())?;
    let b = FileStore::open(dir.path().into())?;
    let expected = a.load()?;
    let locked = a.lock()?;
    assert!(matches!(b.load(), Err(SettingsFileError::Busy)));
    drop(locked);
    let changed = b.save(&expected, edit(), |_| Ok(()))?;
    assert_eq!(changed.text(), "# keep\nterminal.font_size=18 # original\n");
    assert!(matches!(
        a.save(&expected, edit(), |_| Ok(())),
        Err(SettingsFileError::Conflict)
    ));
    let replacement = dir.path().join("replacement");
    fs::write(&replacement, changed.text().as_bytes())?;
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600))?;
    fs::rename(&replacement, dir.path().join(TARGET))?;
    assert!(matches!(
        a.save(&changed, edit(), |_| Ok(())),
        Err(SettingsFileError::Conflict)
    ));
    Ok(())
}
#[test]
fn invalid_size_utf8_and_schema_never_get_reset() -> TestResult {
    let dir = directory()?;
    let store = FileStore::open(dir.path().into())?;
    for data in [
        b"schema_version=2".to_vec(),
        b"secret='example'".to_vec(),
        vec![255],
        vec![b'#'; SETTINGS_TEXT_LIMIT + 1],
    ] {
        write(dir.path(), &data)?;
        assert!(store.load().is_err());
        assert_eq!(fs::read(dir.path().join(TARGET))?, data);
    }
    Ok(())
}
#[test]
fn private_parent_file_and_lock_aliases_or_readonly_intent_are_rejected() -> TestResult {
    for mode in [0o500, 0o755] {
        let dir = directory()?;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode))?;
        assert!(FileStore::open(dir.path().into()).is_err());
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
        assert!(!dir.path().join(LOCK).exists());
    }
    for name in [TARGET, LOCK] {
        for mode in [0o400, 0o644] {
            let dir = directory()?;
            let path = dir.path().join(name);
            fs::write(&path, b"# unchanged\n")?;
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
            let store = FileStore::open(dir.path().into())?;
            assert!(matches!(store.load(), Err(SettingsFileError::Location)));
            assert_eq!(fs::metadata(&path)?.mode() & 0o7777, mode);
            assert_eq!(fs::read(&path)?, b"# unchanged\n");
        }
    }
    for name in [TARGET, LOCK] {
        let dir = directory()?;
        let outside = dir.path().join("unrelated");
        fs::write(&outside, b"untouched")?;
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600))?;
        symlink(&outside, dir.path().join(name))?;
        let store = FileStore::open(dir.path().into())?;
        assert!(store.load().is_err());
        assert_eq!(fs::read(&outside)?, b"untouched");
        fs::remove_file(dir.path().join(name))?;
        fs::hard_link(&outside, dir.path().join(name))?;
        assert!(store.load().is_err());
        assert_eq!(fs::read(&outside)?, b"untouched");
    }
    Ok(())
}
#[test]
fn changed_during_preparation_and_foreign_directory_snapshot_are_refused() -> TestResult {
    let dir = directory()?;
    write(dir.path(), b"terminal.font_size=14\n")?;
    let store = FileStore::open(dir.path().into())?;
    let expected = store.load()?;
    assert!(matches!(
        store.save(&expected, edit(), |stage| {
            if matches!(stage, Stage::TempSynced) {
                write(dir.path(), b"terminal.font_size=20\n").map_err(|_| SettingsFileError::Io)?;
            }
            Ok(())
        }),
        Err(SettingsFileError::Conflict)
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join(TARGET))?,
        "terminal.font_size=20\n"
    );
    let other = directory()?;
    let foreign = FileStore::open(other.path().into())?;
    assert!(matches!(
        foreign.save(&expected, edit(), |_| Ok(())),
        Err(SettingsFileError::Location)
    ));
    Ok(())
}
#[test]
fn first_install_never_overwrites_a_noncooperating_creation() -> TestResult {
    let dir = directory()?;
    let store = FileStore::open(dir.path().into())?;
    let missing = store.load()?;
    let result = store.save(&missing, edit(), |stage| {
        if matches!(stage, Stage::Validated) {
            write(dir.path(), b"terminal.font_size=22").map_err(|_| SettingsFileError::Io)?;
        }
        Ok(())
    });
    assert!(
        matches!(result, Err(SettingsFileError::Conflict)),
        "first install classification: {:?}",
        result.err()
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(TARGET))?,
        "terminal.font_size=22"
    );
    Ok(())
}
#[test]
fn pre_install_failure_preserves_original_but_post_install_is_uncertain() -> TestResult {
    let dir = directory()?;
    write(dir.path(), b"terminal.font_size=14")?;
    let store = FileStore::open(dir.path().into())?;
    let expected = store.load()?;
    assert!(matches!(
        store.save(&expected, edit(), |stage| {
            if matches!(stage, Stage::TempSynced) {
                Err(SettingsFileError::Io)
            } else {
                Ok(())
            }
        }),
        Err(SettingsFileError::Io)
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join(TARGET))?,
        expected.text()
    );
    assert_eq!(fs::read_dir(dir.path())?.count(), 2);
    assert!(matches!(
        store.save(&expected, edit(), |stage| {
            if matches!(stage, Stage::Installed) {
                Err(SettingsFileError::Io)
            } else {
                Ok(())
            }
        }),
        Err(SettingsFileError::Uncertain)
    ));
    assert_eq!(store.load()?.text(), "terminal.font_size=18");
    Ok(())
}
#[test]
fn replaced_directory_and_replaced_lock_are_not_written() -> TestResult {
    let dir = directory()?;
    let store = FileStore::open(dir.path().into())?;
    let missing = store.load()?;
    assert!(matches!(
        store.save(&missing, edit(), |stage| {
            if matches!(stage, Stage::TempSynced) {
                fs::remove_file(dir.path().join(LOCK)).map_err(|_| SettingsFileError::Io)?;
                let file: File = sys::openat(
                    &store.dir,
                    LOCK,
                    OPEN | OFlags::CREATE | OFlags::EXCL | OFlags::RDWR,
                    Mode::RUSR | Mode::WUSR,
                )
                .map_err(|_| SettingsFileError::Io)?
                .into();
                drop(file);
            }
            Ok(())
        }),
        Err(SettingsFileError::Location)
    ));
    assert!(!dir.path().join(TARGET).exists());
    let parent = directory()?;
    let path = parent.path().join("data");
    fs::create_dir(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    let store = FileStore::open(path.clone())?;
    let snap = store.load()?;
    fs::rename(&path, parent.path().join("old"))?;
    fs::create_dir(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    assert!(store.save(&snap, edit(), |_| Ok(())).is_err());
    assert!(!path.join(TARGET).exists());
    Ok(())
}
