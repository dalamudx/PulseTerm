use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::atomic::{AtomicUsize, Ordering},
};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct Fixture {
    root: tempfile::TempDir,
    directory: PathBuf,
    file: KeymapFile,
}
impl Fixture {
    fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let directory = root.path().join("private");
        fs::create_dir(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let file = KeymapFile::open_private(directory.clone())?;
        Ok(Self {
            root,
            directory,
            file,
        })
    }
    fn target(&self) -> PathBuf {
        self.directory.join("keymap.toml")
    }
}
fn set(action: KeymapAction, keys: &[&str]) -> KeymapMutation {
    KeymapMutation::Set(KeymapOverride {
        action,
        keys: keys.iter().map(|key| (*key).into()).collect(),
        when: "workspaceFocus && !modalOpen".into(),
    })
}
#[test]
fn missing_disable_restore_and_reopen_are_distinct() -> TestResult {
    let f = Fixture::new()?;
    let initial = f.file.load()?;
    let original = initial.projection()?;
    let find = original
        .bindings
        .iter()
        .filter(|binding| binding.action == KeymapAction::Find)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(find.len(), 2);
    assert_eq!(
        original
            .bindings
            .iter()
            .filter(|binding| binding.action == KeymapAction::Palette)
            .count(),
        2
    );
    assert!(!f.target().exists());
    let disabled = f
        .file
        .save_authorized(&initial, set(KeymapAction::Palette, &[]), || true)?;
    assert_eq!(disabled.projection()?.bindings, find);
    let reopened = KeymapFile::open_private(f.directory.clone())?;
    assert_eq!(reopened.load()?.projection()?, disabled.projection()?);
    let restored = f.file.save_authorized(
        &disabled,
        KeymapMutation::Restore(KeymapAction::Palette),
        || true,
    )?;
    assert_eq!(restored.projection()?, initial.projection()?);
    assert_eq!(
        fs::metadata(f.target())?.permissions().mode() & 0o7777,
        0o600
    );
    Ok(())
}
#[test]
fn empty_snapshot_cannot_cross_instance_even_at_the_same_directory() -> TestResult {
    let f = Fixture::new()?;
    let original = f.file.load()?;
    let other = KeymapFile::open_private(f.directory.clone())?;
    assert!(matches!(
        other.save_authorized(
            &original,
            set(KeymapAction::Settings, &["ctrl+shift+s"]),
            || true
        ),
        Err(KeymapFileError::Conflict)
    ));
    assert!(!f.target().exists());
    let foreign = Fixture::new()?;
    assert!(matches!(
        foreign.file.save_authorized(
            &original,
            set(KeymapAction::Settings, &["ctrl+shift+s"]),
            || true
        ),
        Err(KeymapFileError::Conflict)
    ));
    assert!(!foreign.target().exists());
    assert!(
        f.file
            .save_authorized(
                &original.clone(),
                set(KeymapAction::Settings, &["ctrl+shift+s"]),
                || true
            )
            .is_ok()
    );
    Ok(())
}
#[test]
fn external_change_rejects_stale_snapshot_without_overwrite() -> TestResult {
    let f = Fixture::new()?;
    let expected = f.file.save_authorized(
        &f.file.load()?,
        set(KeymapAction::Settings, &["ctrl+shift+s"]),
        || true,
    )?;
    let changed = format!(
        "# controlled external change\n{}",
        fs::read_to_string(f.target())?
    );
    fs::write(f.target(), &changed)?;
    assert!(matches!(
        f.file.save_authorized(
            &expected,
            set(KeymapAction::Hosts, &["ctrl+shift+h"]),
            || true
        ),
        Err(KeymapFileError::Conflict)
    ));
    assert_eq!(fs::read_to_string(f.target())?, changed);
    assert!(
        f.file
            .load()?
            .document()?
            .bindings
            .iter()
            .all(|entry| entry.action != KeymapAction::Hosts)
    );
    Ok(())
}
#[test]
fn revoked_or_panicking_authorization_never_installs_before_rename() -> TestResult {
    for cutoff in [0, 1, 2] {
        let f = Fixture::new()?;
        let initial = f.file.load()?;
        let before = f.file.save_authorized(
            &initial,
            set(KeymapAction::Settings, &["ctrl+shift+s"]),
            || true,
        )?;
        let original = fs::read(f.target())?;
        let calls = AtomicUsize::new(0);
        assert!(matches!(
            f.file.save_authorized(
                &before,
                set(KeymapAction::Hosts, &["ctrl+shift+h"]),
                || calls.fetch_add(1, Ordering::SeqCst) < cutoff
            ),
            Err(KeymapFileError::Authorization)
        ));
        assert_eq!(fs::read(f.target())?, original);
        assert!(fs::read_dir(&f.directory)?.all(|entry| {
            entry.is_ok_and(|entry| !entry.file_name().to_string_lossy().ends_with(".tmp"))
        }));
    }
    let f = Fixture::new()?;
    assert!(matches!(
        f.file
            .save_authorized(&f.file.load()?, set(KeymapAction::Palette, &[]), || panic!(
                "controlled authorization failure"
            )),
        Err(KeymapFileError::Authorization)
    ));
    assert!(!f.target().exists());
    Ok(())
}
#[test]
fn invalid_existing_empty_or_future_source_has_no_default_fallback() -> TestResult {
    let f = Fixture::new()?;
    let expected = f.file.load()?;
    for source in [
        "",
        "schema_version = 2",
        "schema_version = 1\npath = '/bin/sh'",
    ] {
        fs::write(f.target(), source)?;
        fs::set_permissions(f.target(), fs::Permissions::from_mode(0o600))?;
        assert!(matches!(f.file.load(), Err(KeymapFileError::Document(_))));
        assert!(matches!(
            f.file
                .save_authorized(&expected, set(KeymapAction::Palette, &[]), || true),
            Err(KeymapFileError::Document(_))
        ));
        assert_eq!(fs::read_to_string(f.target())?, source);
    }
    Ok(())
}
#[test]
fn typed_action_edit_preserves_other_overrides_and_failed_conflict_preserves_file() -> TestResult {
    let f = Fixture::new()?;
    let settings = f.file.save_authorized(
        &f.file.load()?,
        set(KeymapAction::Settings, &["ctrl+shift+s"]),
        || true,
    )?;
    let both = f.file.save_authorized(
        &settings,
        set(KeymapAction::Hosts, &["ctrl+shift+h"]),
        || true,
    )?;
    let before = fs::read(f.target())?;
    assert!(matches!(
        f.file
            .save_authorized(&both, set(KeymapAction::Hosts, &["ctrl+shift+s"]), || true),
        Err(KeymapFileError::Document(KeymapError::Conflict))
    ));
    assert_eq!(fs::read(f.target())?, before);
    let restored =
        f.file
            .save_authorized(&both, KeymapMutation::Restore(KeymapAction::Hosts), || true)?;
    assert_eq!(restored.document()?, settings.document()?);
    Ok(())
}
#[test]
fn untrusted_alias_or_mode_rejected_without_changing_originals() -> TestResult {
    let f = Fixture::new()?;
    let expected =
        f.file
            .save_authorized(&f.file.load()?, set(KeymapAction::Palette, &[]), || true)?;
    let original = fs::read(f.target())?;
    fs::set_permissions(f.target(), fs::Permissions::from_mode(0o644))?;
    assert!(matches!(f.file.load(), Err(KeymapFileError::Location)));
    assert!(matches!(
        f.file.save_authorized(
            &expected,
            set(KeymapAction::Settings, &["ctrl+shift+s"]),
            || true
        ),
        Err(KeymapFileError::Location)
    ));
    assert_eq!(
        fs::metadata(f.target())?.permissions().mode() & 0o7777,
        0o644
    );
    fs::set_permissions(f.target(), fs::Permissions::from_mode(0o600))?;
    fs::hard_link(f.target(), f.root.path().join("alias"))?;
    assert!(matches!(f.file.load(), Err(KeymapFileError::Location)));
    assert_eq!(fs::read(f.target())?, original);
    Ok(())
}
#[test]
fn current_confirmation_checks_image_before_context_and_holds_source_lock() -> TestResult {
    let f = Fixture::new()?;
    let expected = f.file.load()?;
    let calls = AtomicUsize::new(0);
    f.file.confirm_current_authorized(&expected, || {
        calls.fetch_add(1, Ordering::SeqCst);
        matches!(
            KeymapFile::open_private(f.directory.clone()).and_then(|file| file.load()),
            Err(KeymapFileError::Busy)
        )
    })?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        f.file.confirm_current_authorized(&expected, || false),
        Err(KeymapFileError::Authorization)
    ));
    let other = KeymapFile::open_private(f.directory.clone())?;
    assert!(matches!(
        other.confirm_current_authorized(&expected, || {
            calls.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Err(KeymapFileError::Conflict)
    ));
    let current = f.file.save_authorized(
        &expected,
        set(KeymapAction::Settings, &["ctrl+shift+s"]),
        || true,
    )?;
    assert!(matches!(
        f.file.confirm_current_authorized(&expected, || {
            calls.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Err(KeymapFileError::Conflict)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    f.file.confirm_current_authorized(&current, || true)?;
    let changed = format!(
        "# controlled same-values external edit\n{}",
        fs::read_to_string(f.target())?
    );
    fs::write(f.target(), changed)?;
    assert!(matches!(
        f.file.confirm_current_authorized(&current, || true),
        Err(KeymapFileError::Conflict)
    ));
    Ok(())
}

#[test]
fn symlink_and_directory_replacement_do_not_redirect_reads() -> TestResult {
    let f = Fixture::new()?;
    let victim = f.root.path().join("victim.toml");
    fs::write(&victim, "schema_version = 1")?;
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o600))?;
    symlink(&victim, f.target())?;
    assert!(matches!(f.file.load(), Err(KeymapFileError::Location)));
    assert_eq!(fs::read_to_string(&victim)?, "schema_version = 1");
    fs::remove_file(f.target())?;
    let expected = f.file.load()?;
    fs::rename(&f.directory, f.root.path().join("retired"))?;
    fs::create_dir(&f.directory)?;
    fs::set_permissions(&f.directory, fs::Permissions::from_mode(0o700))?;
    assert!(matches!(f.file.load(), Err(KeymapFileError::Location)));
    assert!(matches!(
        f.file
            .save_authorized(&expected, set(KeymapAction::Palette, &[]), || true),
        Err(KeymapFileError::Location)
    ));
    assert!(!f.target().exists());
    Ok(())
}
