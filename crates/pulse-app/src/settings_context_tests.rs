use super::*;
use pulse_store::{
    StateStoreTask, settings::SettingsSource, settings_edit::TerminalSettingEdit,
    settings_scope::ScopedSettingsEdit,
};
use uuid::Uuid;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn target(host: u128) -> Result<SettingsContextTarget> {
    SettingsContextTarget::new(
        Some(SettingsScope::Group(Uuid::from_u128(1))),
        Some(SettingsScope::Host(Uuid::from_u128(host))),
    )
}
struct Fixture {
    directory: tempfile::TempDir,
    store: StateStore,
    task: StateStoreTask,
}
impl Fixture {
    async fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let directory = crate::test_tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        let (store, task) = StateStore::open(directory.path().join("state.db")).await?;
        for (scope, size) in [
            (SettingsScope::Group(Uuid::from_u128(1)), 18),
            (SettingsScope::Host(Uuid::from_u128(2)), 22),
            (SettingsScope::Host(Uuid::from_u128(3)), 26),
        ] {
            store
                .edit_scoped_settings(
                    scope,
                    ScopedSettingsEdit::Set(TerminalSettingEdit::FontSize(size)),
                )
                .await?;
        }
        Ok(Self {
            directory,
            store,
            task,
        })
    }
    async fn finish(self) -> TestResult {
        let Self {
            directory,
            store,
            task,
        } = self;
        task.shutdown().await?;
        drop(store);
        drop(directory);
        Ok(())
    }
}
#[test]
fn target_positions_and_nil_rejected_before_state_creation() -> TestResult {
    assert!(matches!(
        SettingsContextTarget::new(Some(SettingsScope::Host(Uuid::from_u128(1))), None),
        Err(SettingsContextError::Invalid)
    ));
    assert!(matches!(
        SettingsContextTarget::new(None, Some(SettingsScope::Group(Uuid::from_u128(1)))),
        Err(SettingsContextError::Invalid)
    ));
    assert!(matches!(
        SettingsContextTarget::new(Some(SettingsScope::Group(Uuid::nil())), None),
        Err(SettingsContextError::Invalid)
    ));
    Ok(())
}
#[tokio::test]
async fn real_layers_require_confirmation_and_keep_exact_database_revisions() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(
        target(2)?,
        SettingsPatch::parse("terminal.font_family='Fira Code'")?,
    )?;
    let handle = owner.handle();
    let candidate = handle.begin()?.read(&fixture.store).await?;
    assert!(handle.last_good()?.is_none());
    assert!(matches!(handle.begin(), Err(SettingsContextError::Busy)));
    let snapshot = handle.confirm(candidate)?;
    assert!(handle.is_current(&snapshot));
    assert_eq!(snapshot.database_revisions(), (Some(1), Some(1)));
    assert_eq!(snapshot.settings().font_size.value, 22);
    assert_eq!(snapshot.settings().font_size.source, SettingsSource::Host);
    assert_eq!(snapshot.settings().font_family.source, SettingsSource::User);
    fixture.finish().await
}
#[tokio::test]
async fn switch_before_read_retains_slot_and_never_publishes_old_host() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let capture = handle.begin()?;
    owner.select(target(3)?, SettingsPatch::default())?;
    assert!(matches!(handle.begin(), Err(SettingsContextError::Busy)));
    assert!(matches!(
        capture.read(&fixture.store).await,
        Err(SettingsContextError::Stale)
    ));
    let current = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    assert_eq!(current.settings().font_size.value, 26);
    fixture.finish().await
}
#[tokio::test]
async fn late_candidate_cannot_overwrite_new_target_or_release_its_future_slot() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let old = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    let candidate = handle.begin()?.read(&fixture.store).await?;
    owner.select(target(3)?, SettingsPatch::default())?;
    assert!(!handle.is_current(&old));
    assert!(handle.last_good()?.is_none());
    assert!(matches!(
        handle.confirm(candidate),
        Err(SettingsContextError::Stale)
    ));
    let current = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    assert_eq!(current.settings().font_size.value, 26);
    fixture.finish().await
}
#[tokio::test]
async fn foreign_controller_rejects_same_target_and_preserves_own_slot() -> TestResult {
    let fixture = Fixture::new().await?;
    let a = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let b = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let candidate = a.handle().begin()?.read(&fixture.store).await?;
    let pending = b.handle().begin()?;
    assert!(matches!(
        b.handle().confirm(candidate),
        Err(SettingsContextError::Foreign)
    ));
    assert!(matches!(
        b.handle().begin(),
        Err(SettingsContextError::Busy)
    ));
    let snapshot = b.handle().confirm(pending.read(&fixture.store).await?)?;
    assert!(!a.handle().is_current(&snapshot));
    assert!(b.handle().is_current(&snapshot));
    fixture.finish().await
}
#[tokio::test]
async fn invalid_selection_keeps_epoch_current_model_and_permit() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let snapshot = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    let mut bad = SettingsPatch::default();
    bad.terminal.font_size = Some(999);
    assert_eq!(
        owner.select(target(3)?, bad),
        Err(SettingsContextError::Invalid)
    );
    assert!(handle.is_current(&snapshot));
    fixture.finish().await
}
#[tokio::test]
async fn owner_drop_and_close_revoke_candidate_but_shared_store_survives() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let candidate = handle.begin()?.read(&fixture.store).await?;
    drop(owner);
    assert!(matches!(
        handle.confirm(candidate),
        Err(SettingsContextError::Closed)
    ));
    assert!(matches!(handle.begin(), Err(SettingsContextError::Closed)));
    let owner = SettingsContextOwner::new(target(3)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let snapshot = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    owner.close()?;
    assert!(!handle.is_current(&snapshot));
    assert!(matches!(
        handle.last_good(),
        Err(SettingsContextError::Closed)
    ));
    assert_eq!(
        fixture
            .store
            .load_scoped_settings(SettingsScope::Host(Uuid::from_u128(2)))
            .await?
            .revision(),
        1
    );
    fixture.finish().await
}
#[tokio::test]
async fn dropped_capture_and_candidate_release_only_their_slots() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    drop(handle.begin()?);
    let candidate = handle.begin()?.read(&fixture.store).await?;
    drop(candidate);
    assert!(handle.last_good()?.is_none());
    let snapshot = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    assert!(handle.is_current(&snapshot));
    fixture.finish().await
}
#[test]
fn finished_old_guard_cannot_clear_new_job_in_same_epoch() -> TestResult {
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let old = handle.begin()?;
    owner.state.lock().map_err(|_| "poison")?.pending = None;
    let new = handle.begin()?;
    assert_eq!(old.epoch, new.epoch);
    assert_ne!(old.token, new.token);
    drop(old);
    assert!(matches!(handle.begin(), Err(SettingsContextError::Busy)));
    drop(new);
    drop(handle.begin()?);
    Ok(())
}
#[tokio::test]
async fn same_context_failed_refresh_retains_last_good_but_not_current_authority() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let old = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    owner.invalidate()?;
    assert!(!handle.is_current(&old));
    assert_eq!(
        handle
            .last_good()?
            .ok_or("missing last good")?
            .settings()
            .font_size
            .value,
        22
    );
    fixture.task.shutdown().await?;
    assert!(matches!(
        handle.begin()?.read(&fixture.store).await,
        Err(SettingsContextError::Store(StoreError::Closed))
    ));
    assert_eq!(
        handle
            .last_good()?
            .ok_or("missing retained value")?
            .settings()
            .font_size
            .value,
        22
    );
    drop(handle.begin()?);
    drop(fixture.store);
    drop(fixture.directory);
    Ok(())
}
#[test]
fn epoch_and_token_exhaustion_close_without_wrapping_or_new_slot() -> TestResult {
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    owner.state.lock().map_err(|_| "poison")?.epoch = u64::MAX;
    assert_eq!(owner.invalidate(), Err(SettingsContextError::Exhausted));
    assert!(matches!(
        owner.handle().begin(),
        Err(SettingsContextError::Closed)
    ));
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    owner.state.lock().map_err(|_| "poison")?.next_token = u64::MAX;
    assert!(matches!(
        owner.handle().begin(),
        Err(SettingsContextError::Exhausted)
    ));
    assert!(matches!(
        owner.handle().begin(),
        Err(SettingsContextError::Closed)
    ));
    Ok(())
}
#[tokio::test]
async fn a_to_b_to_a_does_not_revive_old_same_target_candidate() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let old = handle.begin()?.read(&fixture.store).await?;
    owner.select(target(3)?, SettingsPatch::default())?;
    owner.select(target(2)?, SettingsPatch::default())?;
    assert!(matches!(
        handle.confirm(old),
        Err(SettingsContextError::Stale)
    ));
    let current = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    assert_eq!(current.generation(), 3);
    assert_eq!(current.settings().font_size.value, 22);
    fixture.finish().await
}
#[tokio::test]
async fn newer_publication_invalidates_old_snapshot_even_with_same_epoch() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let old = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    let current = handle.confirm(handle.begin()?.read(&fixture.store).await?)?;
    assert_eq!(old.generation(), current.generation());
    assert!(!handle.is_current(&old));
    assert!(handle.is_current(&current));
    fixture.finish().await
}
#[tokio::test]
async fn delayed_thread_candidate_after_selection_is_stale_without_automatic_retry() -> TestResult {
    let fixture = Fixture::new().await?;
    let owner = SettingsContextOwner::new(target(2)?, SettingsPatch::default())?;
    let handle = owner.handle();
    let candidate = handle.begin()?.read(&fixture.store).await?;
    let (confirm_tx, confirm_rx) = std::sync::mpsc::channel();
    let worker_handle = handle.clone();
    let worker = std::thread::spawn(move || {
        confirm_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "thread deadline")?;
        Ok::<_, &str>(matches!(
            worker_handle.confirm(candidate),
            Err(SettingsContextError::Stale)
        ))
    });
    owner.select(target(3)?, SettingsPatch::default())?;
    assert!(matches!(handle.begin(), Err(SettingsContextError::Busy)));
    confirm_tx.send(())?;
    assert!(worker.join().map_err(|_| "thread panicked")??);
    assert!(handle.last_good()?.is_none());
    drop(handle.begin()?);
    fixture.finish().await
}
