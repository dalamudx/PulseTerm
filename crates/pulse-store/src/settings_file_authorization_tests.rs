use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    directory: Directory,
    file: SettingsFile,
    expected: SettingsSnapshot,
}
impl Fixture {
    fn new() -> TestResult<Self> {
        let directory = Directory(
            std::env::temp_dir().join(format!("pulse-settings-auth-{}", uuid::Uuid::new_v4())),
        );
        fs::create_dir(&directory.0)?;
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700))?;
        fs::write(
            directory.0.join("settings.toml"),
            "# fixed fixture comment\n[terminal]\nfont_size = 12\n",
        )?;
        fs::set_permissions(
            directory.0.join("settings.toml"),
            fs::Permissions::from_mode(0o600),
        )?;
        let file = SettingsFile::open_private(directory.0.clone())?;
        let expected = file.load()?;
        Ok(Self {
            directory,
            file,
            expected,
        })
    }
    fn changed() -> SettingsMutation {
        SettingsMutation::Set(TerminalSettingEdit::FontSize(16))
    }
    fn unchanged(&self) -> TestResult {
        assert_eq!(self.file.load()?.text(), self.expected.text());
        for entry in fs::read_dir(&self.directory.0)? {
            let name = entry?.file_name();
            assert!(name == "settings.toml" || name == ".settings.lock");
        }
        Ok(())
    }
}
#[test]
fn revoked_at_entry_never_installs() -> TestResult {
    let f = Fixture::new()?;
    assert!(matches!(
        f.file
            .save_authorized(&f.expected, Fixture::changed(), || false),
        Err(SettingsFileError::Authorization)
    ));
    f.unchanged()
}
#[test]
fn revoked_after_temp_fsync_or_before_install_preserves_original_and_removes_temp() -> TestResult {
    for denied_call in [2, 3] {
        let f = Fixture::new()?;
        let calls = AtomicUsize::new(0);
        assert!(matches!(
            f.file
                .save_authorized(&f.expected, Fixture::changed(), || calls
                    .fetch_add(1, Ordering::SeqCst)
                    + 1
                    != denied_call),
            Err(SettingsFileError::Authorization)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), denied_call);
        f.unchanged()?;
    }
    Ok(())
}
#[test]
fn panicking_execution_predicate_is_denied_and_cleans_temp() -> TestResult {
    for panic_call in [1, 2, 3] {
        let f = Fixture::new()?;
        let calls = AtomicUsize::new(0);
        assert!(matches!(
            f.file.save_authorized(&f.expected, Fixture::changed(), || {
                assert_ne!(
                    calls.fetch_add(1, Ordering::SeqCst) + 1,
                    panic_call,
                    "fixed authorization panic"
                );
                true
            }),
            Err(SettingsFileError::Authorization)
        ));
        f.unchanged()?;
    }
    Ok(())
}
#[test]
fn accepted_install_is_not_rolled_back_after_final_execution_grant() -> TestResult {
    let f = Fixture::new()?;
    let current = AtomicBool::new(true);
    let calls = AtomicUsize::new(0);
    let saved = f
        .file
        .save_authorized(&f.expected, Fixture::changed(), || {
            let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
            let accepted = current.load(Ordering::SeqCst);
            if n == 3 {
                current.store(false, Ordering::SeqCst);
            }
            accepted
        })?;
    assert!(!current.load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(saved.text().contains("font_size = 16"));
    assert_eq!(f.file.load()?.text(), saved.text());
    assert!(matches!(
        f.file
            .save_authorized(&f.expected, Fixture::changed(), || true),
        Err(SettingsFileError::Conflict)
    ));
    Ok(())
}
#[test]
fn no_op_still_requires_execution_authorization() -> TestResult {
    let f = Fixture::new()?;
    let change = || SettingsMutation::Set(TerminalSettingEdit::FontSize(12));
    assert!(matches!(
        f.file.save_authorized(&f.expected, change(), || false),
        Err(SettingsFileError::Authorization)
    ));
    assert_eq!(
        f.file
            .save_authorized(&f.expected, change(), || true)?
            .text(),
        f.expected.text()
    );
    f.unchanged()
}
