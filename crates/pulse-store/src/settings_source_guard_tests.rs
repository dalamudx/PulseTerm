use super::tests::{TESTS, dir, source, status};
use super::*;
use std::cell::Cell;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn current_callback_holds_metadata_lock_and_releases_it_after_return() -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let snapshot = h.snapshot()?;
    let shared = t.shared.clone();
    let value = h.with_current(&snapshot, || {
        assert!(matches!(
            shared.state.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        42
    })?;
    assert_eq!(value, 42);
    // The supervisor can briefly own this mutex after publication. A fresh authorized
    // callback proves the previous guard released without requiring zero contention.
    let again = h.with_current(&snapshot, || {
        assert!(matches!(
            shared.state.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        43
    })?;
    assert_eq!(again, 43);
    t.shutdown().await?;
    // No worker remains now: instantaneous try_lock has a deterministic meaning.
    assert!(shared.state.try_lock().is_ok());
    Ok(())
}
#[tokio::test]
async fn signal_bad_file_and_same_model_reverification_never_run_old_callback() -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let old = h.snapshot()?;
    let calls = Cell::new(0);
    source(&d, "terminal.font_size=999")?;
    h.invalidate()?;
    assert!(matches!(
        h.with_current(&old, || calls.set(1)),
        Err(SettingsSourceError::Stale)
    ));
    status(
        &h,
        SettingsSourceStatus::Rejected(SettingsReloadError::File(
            crate::settings_file::SettingsFileError::Document(
                crate::settings::SettingsError::Value,
            ),
        )),
    )
    .await?;
    source(&d, "# same semantic value\nterminal.font_size=18")?;
    h.invalidate()?;
    status(&h, SettingsSourceStatus::Ready).await?;
    let fresh = h.snapshot()?;
    assert_eq!(old.revision(), fresh.revision());
    assert!(matches!(
        h.with_current(&old, || calls.set(2)),
        Err(SettingsSourceError::Stale)
    ));
    h.with_current(&fresh, || calls.set(3))?;
    assert_eq!(calls.get(), 3);
    t.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn closed_or_other_source_proof_cannot_invoke_callback() -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let old = h.snapshot()?;
    let calls = Cell::new(0);
    t.shutdown().await?;
    assert!(matches!(
        h.with_current(&old, || calls.set(1)),
        Err(SettingsSourceError::Closed)
    ));
    let (next, task) = open_user_settings_source(d.path().into()).await?;
    assert!(matches!(
        next.with_current(&old, || calls.set(2)),
        Err(SettingsSourceError::Stale)
    ));
    assert_eq!(calls.get(), 0);
    task.shutdown().await?;
    Ok(())
}
