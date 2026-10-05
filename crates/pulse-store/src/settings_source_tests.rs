use super::*;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
type TestResult = Result<(), Box<dyn std::error::Error>>;
static TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn dir() -> std::io::Result<tempfile::TempDir> {
    let d = tempfile::tempdir()?;
    fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700))?;
    Ok(d)
}
fn source(d: &tempfile::TempDir, text: &str) -> std::io::Result<()> {
    fs::write(d.path().join("settings.toml"), text)?;
    fs::set_permissions(
        d.path().join("settings.toml"),
        fs::Permissions::from_mode(0o600),
    )
}
async fn status(h: &SettingsSourceHandle, expected: SettingsSourceStatus) -> TestResult {
    let mut rx = h.subscribe()?;
    tokio::time::timeout(Duration::from_secs(2), rx.wait_for(|s| *s == expected)).await??;
    Ok(())
}
async fn rejected(h: &SettingsSourceHandle) -> TestResult {
    let mut rx = h.subscribe()?;
    tokio::time::timeout(
        Duration::from_secs(2),
        rx.wait_for(|s| matches!(s, SettingsSourceStatus::Rejected(_))),
    )
    .await??;
    Ok(())
}
fn gate(
    h: &SettingsSourceHandle,
    phase: PausePhase,
) -> Result<(std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>), Box<dyn std::error::Error>>
{
    let shared = h.shared.upgrade().ok_or("source gone")?;
    let (enter, entered) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    shared.state.lock().map_err(|_| "state poisoned")?.pause = Some(Pause {
        phase,
        entered: enter,
        release: wait,
    });
    Ok((entered, release))
}
fn entered(rx: &std::sync::mpsc::Receiver<()>) -> TestResult {
    rx.recv_timeout(Duration::from_secs(2))?;
    Ok(())
}
#[tokio::test]
async fn initial_bad_file_refuses_source_without_reset_and_returns_quota() -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "schema_version=2")?;
    assert!(matches!(
        open_user_settings_source(d.path().into()).await,
        Err(SettingsSourceError::Reload(_))
    ));
    assert_eq!(
        fs::read_to_string(d.path().join("settings.toml"))?,
        "schema_version=2"
    );
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    assert!(h.is_current(&h.snapshot()?));
    t.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn current_capture_then_semantic_update_and_comment_verification_do_not_revive_old_receipts()
-> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let old = h.snapshot()?;
    source(&d, "# comment\nterminal.font_size=18")?;
    h.invalidate()?;
    status(&h, SettingsSourceStatus::Ready).await?;
    let comment = h.snapshot()?;
    assert_eq!(old.revision(), comment.revision());
    assert!(!h.is_current(&old));
    assert!(h.is_current(&comment));
    source(&d, "terminal.font_size=22")?;
    h.invalidate()?;
    status(&h, SettingsSourceStatus::Ready).await?;
    assert_eq!(h.snapshot()?.revision(), 2);
    assert_eq!(h.snapshot()?.patch().terminal.font_size, Some(22));
    assert!(!h.is_current(&comment));
    t.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn rejected_read_keeps_last_good_and_never_retries_without_new_signal() -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let old = h.snapshot()?;
    source(&d, "terminal.font_size=999")?;
    h.invalidate()?;
    rejected(&h).await?;
    assert_eq!(h.snapshot()?.patch(), old.patch());
    assert!(!h.is_current(&old));
    source(&d, "terminal.font_size=20")?;
    assert_eq!(h.snapshot()?.patch().terminal.font_size, Some(18));
    h.invalidate()?;
    status(&h, SettingsSourceStatus::Ready).await?;
    assert_eq!(h.snapshot()?.patch().terminal.font_size, Some(20));
    t.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn stale_success_followed_by_latest_rejection_cannot_replace_published_last_good_and_signals_coalesce()
-> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let (ready, release) = gate(&h, PausePhase::AfterRead)?;
    source(&d, "terminal.font_size=20")?;
    h.invalidate()?;
    entered(&ready)?;
    source(&d, "terminal.font_size=999")?;
    for _ in 0..1000 {
        h.invalidate()?;
    }
    release.send(())?;
    rejected(&h).await?;
    assert_eq!(h.snapshot()?.revision(), 1);
    assert_eq!(h.snapshot()?.patch().terminal.font_size, Some(18));
    {
        let s = h.shared.upgrade().ok_or("source gone")?;
        assert_eq!(s.state.lock().map_err(|_| "state poisoned")?.reads, 3);
    }
    source(&d, "terminal.font_size=20")?;
    h.invalidate()?;
    status(&h, SettingsSourceStatus::Ready).await?;
    assert_eq!(h.snapshot()?.revision(), 2);
    t.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn close_after_read_revokes_before_publication_but_keeps_global_quota_until_actual_exit()
-> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let old = h.snapshot()?;
    let (ready, release) = gate(&h, PausePhase::AfterRead)?;
    source(&d, "terminal.font_size=20")?;
    h.invalidate()?;
    entered(&ready)?;
    t.close();
    assert!(matches!(h.snapshot(), Err(SettingsSourceError::Closed)));
    assert!(!h.is_current(&old));
    assert!(matches!(
        open_user_settings_source(d.path().into()).await,
        Err(SettingsSourceError::Busy)
    ));
    release.send(())?;
    t.shutdown().await?;
    let (h2, t2) = open_user_settings_source(d.path().into()).await?;
    assert_eq!(h2.snapshot()?.patch().terminal.font_size, Some(20));
    assert!(!h2.is_current(&old));
    t2.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn closing_queued_work_skips_read_and_handle_cannot_resurrect_owner() -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, mut t) = open_user_settings_source(d.path().into()).await?;
    let shared = t.shared.clone();
    let (ready, release) = gate(&h, PausePhase::BeforeRead)?;
    h.invalidate()?;
    entered(&ready)?;
    let join = t.task.take().ok_or("supervisor gone")?;
    drop(t);
    assert!(matches!(h.invalidate(), Err(SettingsSourceError::Closed)));
    assert!(matches!(
        open_user_settings_source(d.path().into()).await,
        Err(SettingsSourceError::Busy)
    ));
    release.send(())?;
    join.await??;
    assert_eq!(shared.state.lock().map_err(|_| "state poisoned")?.reads, 1);
    assert!(matches!(h.snapshot(), Err(SettingsSourceError::Closed)));
    Ok(())
}
#[tokio::test]
async fn deletion_restores_sparse_defaults_and_explicit_false_preserves_source_delta() -> TestResult
{
    let _serial = TESTS.lock().await;
    let d = dir()?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    assert_eq!(h.snapshot()?.patch().terminal.cursor_blink, None);
    source(&d, "terminal.cursor_blink=false")?;
    h.invalidate()?;
    status(&h, SettingsSourceStatus::Ready).await?;
    assert_eq!(h.snapshot()?.patch().terminal.cursor_blink, Some(false));
    assert_eq!(h.snapshot()?.revision(), 2);
    fs::remove_file(d.path().join("settings.toml"))?;
    h.invalidate()?;
    status(&h, SettingsSourceStatus::Ready).await?;
    assert_eq!(h.snapshot()?.patch().terminal.cursor_blink, None);
    assert_eq!(h.snapshot()?.revision(), 3);
    t.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn requested_counter_exhaustion_closes_without_wrapping() -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let old = h.snapshot()?;
    h.shared
        .upgrade()
        .ok_or("source gone")?
        .state
        .lock()
        .map_err(|_| "state poisoned")?
        .requested = u64::MAX;
    assert!(matches!(
        h.invalidate(),
        Err(SettingsSourceError::Exhausted)
    ));
    assert!(!h.is_current(&old));
    t.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn model_revision_exhaustion_and_supervised_worker_failure_close_the_source() -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let mut observed = h.subscribe()?;
    h.shared
        .upgrade()
        .ok_or("source gone")?
        .state
        .lock()
        .map_err(|_| "state poisoned")?
        .revision = u64::MAX;
    source(&d, "terminal.font_size=20")?;
    h.invalidate()?;
    tokio::time::timeout(
        Duration::from_secs(2),
        observed.wait_for(|s| *s == SettingsSourceStatus::Failed),
    )
    .await??;
    assert!(matches!(h.snapshot(), Err(SettingsSourceError::Closed)));
    assert!(matches!(
        t.shutdown().await,
        Err(SettingsSourceError::Exhausted)
    ));
    Ok(())
}
async fn supervised_failure(phase: PausePhase, panic: bool) -> TestResult {
    let _serial = TESTS.lock().await;
    let d = dir()?;
    source(&d, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(d.path().into()).await?;
    let shared = t.shared.clone();
    let mut observed = h.subscribe()?;
    let (ready, release) = gate(&h, phase)?;
    source(&d, "terminal.font_size=20")?;
    h.invalidate()?;
    entered(&ready)?;
    if panic {
        release.send(())?;
    } else {
        drop(release);
    }
    tokio::time::timeout(
        Duration::from_secs(2),
        observed.wait_for(|s| *s == SettingsSourceStatus::Failed),
    )
    .await??;
    assert!(matches!(
        t.shutdown().await,
        Err(SettingsSourceError::Worker)
    ));
    assert_eq!(*observed.borrow(), SettingsSourceStatus::Failed);
    assert_eq!(
        shared
            .state
            .lock()
            .map_err(|_| "state poisoned")?
            .patch
            .terminal
            .font_size,
        Some(18)
    );
    assert!(matches!(h.snapshot(), Err(SettingsSourceError::Closed)));
    let (_, next) = open_user_settings_source(d.path().into()).await?;
    next.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn worker_panic_is_supervised_without_publishing_or_leaking_quota() -> TestResult {
    supervised_failure(PausePhase::Crash, true).await
}
#[tokio::test]
async fn worker_error_is_supervised_and_drop_does_not_hide_failure() -> TestResult {
    supervised_failure(PausePhase::AfterRead, false).await
}

#[test]
fn missing_runtime_does_not_queue_or_open_a_directory() {
    let mut future = std::pin::pin!(open_user_settings_source(PathBuf::from(
        "/unused-settings-path"
    )));
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(matches!(
        std::future::Future::poll(future.as_mut(), &mut cx),
        std::task::Poll::Ready(Err(SettingsSourceError::Runtime))
    ));
}
