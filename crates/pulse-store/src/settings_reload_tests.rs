use super::*;
use crate::settings::{SettingsResolver, SettingsSource};
use crate::settings_edit::TerminalSettingEdit;
use crate::settings_file::SettingsMutation;
use std::{fs, os::unix::fs::PermissionsExt};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn directory() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}
fn source(dir: &std::path::Path, text: &str) -> std::io::Result<()> {
    fs::write(dir.join("settings.toml"), text)?;
    fs::set_permissions(dir.join("settings.toml"), fs::Permissions::from_mode(0o600))
}
fn cache(dir: &std::path::Path) -> Result<SettingsReloadCache, SettingsFileError> {
    Ok(SettingsReloadCache::new(SettingsFile::open_private(
        dir.into(),
    )?))
}
#[test]
fn malformed_initial_document_is_not_reset_or_a_valid_source() -> TestResult {
    let dir = directory()?;
    source(dir.path(), "schema_version=2")?;
    let mut cache = cache(dir.path())?;
    assert!(matches!(cache.refresh(), Err(SettingsReloadError::File(_))));
    assert_eq!(*cache.user_patch(), SettingsPatch::default());
    assert!(cache.snapshot().is_none());
    assert_eq!(cache.revision(), 0);
    assert_eq!(
        fs::read_to_string(dir.path().join("settings.toml"))?,
        "schema_version=2"
    );
    Ok(())
}
#[test]
fn last_good_survives_invalid_input_and_recovers_only_on_new_signal() -> TestResult {
    let dir = directory()?;
    source(dir.path(), "terminal.font_size=18")?;
    let mut cache = cache(dir.path())?;
    cache.refresh()?;
    let good = cache.snapshot().ok_or("snapshot missing")?.clone();
    source(dir.path(), "terminal.font_size=999")?;
    cache.invalidate()?;
    assert!(matches!(cache.refresh(), Err(SettingsReloadError::File(_))));
    assert_eq!(cache.user_patch().terminal.font_size, Some(18));
    assert_eq!(
        cache.snapshot().ok_or("snapshot missing")?.text(),
        good.text()
    );
    assert_eq!(cache.revision(), 1);
    source(dir.path(), "terminal.font_size=20")?;
    assert_eq!(cache.refresh()?, SettingsReloadReceipt::Unchanged);
    cache.invalidate()?;
    assert!(matches!(
        cache.refresh()?,
        SettingsReloadReceipt::Applied { revision: 2, .. }
    ));
    assert_eq!(cache.user_patch().terminal.font_size, Some(20));
    Ok(())
}
#[test]
fn coalesced_invalidations_consume_one_read_not_a_retry_queue() -> TestResult {
    let dir = directory()?;
    source(dir.path(), "terminal.cursor_blink=false")?;
    let mut cache = cache(dir.path())?;
    for _ in 0..1000 {
        cache.invalidate()?;
    }
    assert!(matches!(
        cache.refresh()?,
        SettingsReloadReceipt::Applied { revision: 1, .. }
    ));
    source(dir.path(), "not a document")?;
    for _ in 0..10 {
        assert_eq!(cache.refresh()?, SettingsReloadReceipt::Unchanged);
    }
    assert_eq!(cache.revision(), 1);
    Ok(())
}
#[test]
fn comment_only_updates_source_without_republishing_or_losing_save_baseline() -> TestResult {
    let dir = directory()?;
    source(dir.path(), "# before\nterminal.font_size=18")?;
    let mut cache = cache(dir.path())?;
    cache.refresh()?;
    source(
        dir.path(),
        "# after, not an event payload\nterminal.font_size=18",
    )?;
    cache.invalidate()?;
    assert_eq!(cache.refresh()?, SettingsReloadReceipt::Unchanged);
    assert_eq!(cache.revision(), 1);
    let store = SettingsFile::open_private(dir.path().into())?;
    let source = cache.snapshot().ok_or("snapshot missing")?;
    assert!(source.text().starts_with("# after"));
    let saved = store.save(
        source,
        SettingsMutation::Set(TerminalSettingEdit::FontSize(20)),
    )?;
    assert!(saved.text().starts_with("# after"));
    Ok(())
}
#[test]
fn deletion_restores_lower_source_and_explicit_false_is_a_real_change() -> TestResult {
    let dir = directory()?;
    source(dir.path(), "terminal.cursor_blink=false")?;
    let mut cache = cache(dir.path())?;
    let receipt = cache.refresh()?;
    assert!(matches!(
        receipt,
        SettingsReloadReceipt::Applied {
            fields: SettingsChangedFields {
                cursor_blink: true,
                font_size: false,
                font_family: false
            },
            ..
        }
    ));
    let resolved = SettingsResolver::resolve(
        Some(cache.snapshot().ok_or("snapshot missing")?.text()),
        None,
        None,
    )?;
    assert_eq!(resolved.cursor_blink.source, SettingsSource::User);
    fs::remove_file(dir.path().join("settings.toml"))?;
    cache.invalidate()?;
    assert!(matches!(
        cache.refresh()?,
        SettingsReloadReceipt::Applied { revision: 2, .. }
    ));
    assert!(!cache.snapshot().ok_or("snapshot missing")?.exists());
    let resolved = SettingsResolver::resolve(
        Some(cache.snapshot().ok_or("snapshot missing")?.text()),
        None,
        None,
    )?;
    assert_eq!(resolved.cursor_blink.source, SettingsSource::Default);
    Ok(())
}
#[test]
fn busy_reads_retain_values_without_automatic_replay() -> TestResult {
    let dir = directory()?;
    source(dir.path(), "terminal.font_size=18")?;
    let mut cache = cache(dir.path())?;
    cache.refresh()?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join(".settings.lock"))?;
    lock.try_lock()?;
    cache.invalidate()?;
    assert!(matches!(
        cache.refresh(),
        Err(SettingsReloadError::File(SettingsFileError::Busy))
    ));
    lock.unlock()?;
    assert_eq!(cache.user_patch().terminal.font_size, Some(18));
    assert_eq!(cache.refresh()?, SettingsReloadReceipt::Unchanged);
    cache.invalidate()?;
    assert_eq!(cache.refresh()?, SettingsReloadReceipt::Unchanged);
    Ok(())
}
#[test]
fn closed_cache_never_reads_or_publishes_again() -> TestResult {
    let dir = directory()?;
    source(dir.path(), "terminal.font_size=18")?;
    let mut cache = cache(dir.path())?;
    cache.refresh()?;
    cache.close();
    source(dir.path(), "terminal.font_size=20")?;
    assert_eq!(cache.invalidate(), Err(SettingsReloadError::Closed));
    assert_eq!(cache.refresh(), Err(SettingsReloadError::Closed));
    assert_eq!(cache.user_patch().terminal.font_size, Some(18));
    Ok(())
}
#[test]
fn revision_exhaustion_retains_old_model_and_closes_instead_of_wrapping() -> TestResult {
    let dir = directory()?;
    source(dir.path(), "terminal.font_size=18")?;
    let mut cache = cache(dir.path())?;
    cache.refresh()?;
    cache.revision = u64::MAX;
    source(dir.path(), "terminal.font_size=20")?;
    cache.invalidate()?;
    assert_eq!(cache.refresh(), Err(SettingsReloadError::Exhausted));
    assert_eq!(cache.revision(), u64::MAX);
    assert_eq!(cache.user_patch().terminal.font_size, Some(18));
    assert_eq!(cache.invalidate(), Err(SettingsReloadError::Closed));
    Ok(())
}
