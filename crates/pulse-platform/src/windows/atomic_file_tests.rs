use super::*;
use std::{cell::Cell, path::Path};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn count_staged(path: &Path) -> std::io::Result<usize> {
    Ok(std::fs::read_dir(path)?
        .collect::<std::io::Result<Vec<_>>>()?
        .iter()
        .filter(|e| e.file_name().to_string_lossy().starts_with(TEMP_PREFIX))
        .count())
}
#[test]
fn absent_creation_replacement_and_noop_have_exact_snapshots() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let store = directory.atomic_file("settings.toml", 65536)?;
    let absent = store.load()?;
    assert!(!absent.exists());
    let first = store.save(&absent, b"first\0\xff", || true)?;
    assert!(first.exists());
    assert_eq!(first.bytes(), b"first\0\xff");
    assert!(store.load()? == first);
    let second = store.save(&first, b"second", || true)?;
    assert_eq!(second.bytes(), b"second");
    assert!(second != first);
    let same = store.save(&second, b"second", || true)?;
    assert!(same == second);
    assert_eq!(count_staged(&path)?, 0);
    assert_eq!(
        directory.open_file("settings.toml")?.read_bounded(65536)?,
        b"second"
    );
    Ok(())
}
#[test]
fn empty_creation_is_distinct_from_missing_and_stale_saves_reject() -> TestResult {
    let root = tempfile::tempdir()?;
    let directory = PrivateDirectory::create(root.path().join("private"))?;
    let store = directory.atomic_file("settings.toml", 64)?;
    let absent = store.load()?;
    let empty = store.save(&absent, b"", || true)?;
    assert!(empty.exists() && empty != absent);
    assert!(matches!(
        store.save(&absent, b"stale", || true),
        Err(Error::Conflict)
    ));
    assert!(store.load()? == empty);
    Ok(())
}
#[test]
fn snapshots_cannot_move_to_other_directories_names_or_budgets() -> TestResult {
    let root = tempfile::tempdir()?;
    let first = PrivateDirectory::create(root.path().join("first"))?;
    let second = PrivateDirectory::create(root.path().join("second"))?;
    let snapshot = first.atomic_file("settings.toml", 64)?.load()?;
    for store in [
        first.atomic_file("other.toml", 64)?,
        first.atomic_file("settings.toml", 63)?,
        second.atomic_file("settings.toml", 64)?,
    ] {
        assert!(matches!(
            store.save(&snapshot, b"changed", || true),
            Err(Error::Location)
        ));
    }
    for name in [LOCK, ".PULSE-WRITE.LOCK", ".PULSE-ATOMIC-collision"] {
        assert!(matches!(first.atomic_file(name, 64), Err(Error::Location)));
    }
    Ok(())
}
#[test]
fn each_authorization_boundary_can_revoke_without_changing_original_or_leaking_staging()
-> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("settings.toml", b"original", 64)?);
    let store = directory.atomic_file("settings.toml", 64)?;
    let snapshot = store.load()?;
    for revoked in 1..=3 {
        let calls = Cell::new(0);
        assert!(matches!(
            store.save(&snapshot, b"new", || {
                calls.set(calls.get() + 1);
                calls.get() != revoked
            }),
            Err(Error::Revoked)
        ));
        assert_eq!(calls.get(), revoked);
        assert_eq!(count_staged(&path)?, 0);
        assert!(store.load()? == snapshot);
    }
    Ok(())
}
#[test]
fn authorization_panic_after_staging_cleans_up_and_releases_lock() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let store = directory.atomic_file("settings.toml", 64)?;
    let snapshot = store.load()?;
    let calls = Cell::new(0);
    let result = store.save(&snapshot, b"new", || {
        calls.set(calls.get() + 1);
        assert!(calls.get() != 2, "fixed authorization fixture");
        true
    });
    assert!(matches!(result, Err(Error::Revoked)));
    assert_eq!(count_staged(&path)?, 0);
    assert!(store.load()? == snapshot);
    Ok(())
}
#[test]
fn external_change_after_staging_conflicts_and_preserves_new_external_bytes() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("settings.toml", b"original", 64)?);
    let store = directory.atomic_file("settings.toml", 64)?;
    let snapshot = store.load()?;
    let calls = Cell::new(0);
    let result = store.save(&snapshot, b"replacement", || {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            assert!(std::fs::write(path.join("settings.toml"), b"external").is_ok());
        }
        true
    });
    assert!(matches!(result, Err(Error::Conflict)));
    assert_eq!(count_staged(&path)?, 0);
    assert_eq!(store.load()?.bytes(), b"external");
    Ok(())
}
#[test]
fn directory_wide_lock_is_exclusive_and_nonempty_lock_is_never_truncated() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let first = directory.atomic_file("settings.toml", 64)?;
    let second = directory.atomic_file("keymap.toml", 64)?;
    let guard = first.lock()?;
    assert!(matches!(second.load(), Err(Error::Busy)));
    drop(guard);
    assert!(!second.load()?.exists());
    std::fs::write(path.join(LOCK), b"foreign lock bytes")?;
    assert!(matches!(first.load(), Err(Error::Location)));
    assert_eq!(std::fs::read(path.join(LOCK))?, b"foreign lock bytes");
    Ok(())
}
#[test]
fn native_commit_failure_is_uncertain_and_does_not_delete_original_or_staged_file() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("settings.toml", b"original", 64)?);
    let store = directory.atomic_file("settings.toml", 64)?;
    let snapshot = store.load()?;
    let mut held = directory.open_file("settings.toml")?;
    assert!(matches!(
        store.save(&snapshot, b"replacement", || true),
        Err(Error::Uncertain)
    ));
    assert_eq!(held.read_bounded(64)?, b"original");
    drop(held);
    assert_eq!(count_staged(&path)?, 1);
    assert_eq!(store.load()?.bytes(), b"original");
    Ok(())
}
#[test]
fn newly_appearing_target_is_not_overwritten_after_the_final_check() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    let store = directory.atomic_file("settings.toml", 64)?;
    let snapshot = store.load()?;
    let calls = Cell::new(0);
    let result = store.save(&snapshot, b"replacement", || {
        calls.set(calls.get() + 1);
        if calls.get() == 3 {
            let created = directory.create_file("settings.toml", b"raced", 64);
            assert!(created.is_ok());
            drop(created);
        }
        true
    });
    assert!(matches!(result, Err(Error::Uncertain)));
    assert_eq!(store.load()?.bytes(), b"raced");
    assert_eq!(count_staged(&path)?, 1);
    Ok(())
}
#[test]
fn a_new_hard_link_before_commit_revokes_file_admission() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("private");
    let directory = PrivateDirectory::create(&path)?;
    drop(directory.create_file("settings.toml", b"original", 64)?);
    let store = directory.atomic_file("settings.toml", 64)?;
    let snapshot = store.load()?;
    let calls = Cell::new(0);
    let result = store.save(&snapshot, b"replacement", || {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            assert!(std::fs::hard_link(path.join("settings.toml"), path.join("alias")).is_ok());
        }
        true
    });
    assert!(matches!(result, Err(Error::Location)));
    assert_eq!(count_staged(&path)?, 0);
    assert_eq!(std::fs::read(path.join("alias"))?, b"original");
    Ok(())
}
