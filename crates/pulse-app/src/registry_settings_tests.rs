#[tokio::test]
async fn other_session_in_same_window_cannot_accept_candidate_or_lose_its_pending_slot()
-> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let candidate = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?
        .read(&f.store)
        .await?;
    let (handle, task, driver) = session();
    let id = f.registry.register(f.owner, handle)?;
    let mut view = f.registry.attach(f.owner, id, config()).await?;
    bootstrap(&f.registry, f.owner, id, &mut view).await;
    f.registry.configure_view_settings(
        f.owner,
        id,
        view.generation,
        target(2)?,
        SettingsPatch::default(),
    )?;
    let pending = f
        .registry
        .begin_view_settings(f.owner, id, view.generation)?;
    assert!(matches!(
        f.registry
            .confirm_view_settings(f.owner, id, view.generation, candidate),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    assert!(matches!(
        f.registry.begin_view_settings(f.owner, id, view.generation),
        Err(RegistrySettingsError::Context(SettingsContextError::Busy))
    ));
    let snapshot = f.registry.confirm_view_settings(
        f.owner,
        id,
        view.generation,
        pending.read(&f.store).await?,
    )?;
    assert!(
        f.registry
            .view_settings_current(f.owner, id, view.generation, &snapshot)?
    );
    f.finish().await?;
    assert_eq!(task.join().await?.lifecycle, Lifecycle::Closed);
    drop(driver);
    Ok(())
}
#[tokio::test]
async fn foreign_registry_pointer_is_checked_even_if_private_test_forces_matching_metadata()
-> TestResult {
    let a = Fixture::new().await?;
    let b = Fixture::new().await?;
    a.configure()?;
    b.configure()?;
    let mut candidate = a
        .registry
        .begin_view_settings(a.owner, a.id, a.binding.generation)?
        .read(&a.store)
        .await?;
    candidate.owner = b.owner;
    candidate.id = b.id;
    candidate.generation = b.binding.generation;
    let pending = b
        .registry
        .begin_view_settings(b.owner, b.id, b.binding.generation)?;
    assert!(matches!(
        b.registry
            .confirm_view_settings(b.owner, b.id, b.binding.generation, candidate),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    assert!(matches!(
        b.registry
            .begin_view_settings(b.owner, b.id, b.binding.generation),
        Err(RegistrySettingsError::Context(SettingsContextError::Busy))
    ));
    let snapshot = b.registry.confirm_view_settings(
        b.owner,
        b.id,
        b.binding.generation,
        pending.read(&b.store).await?,
    )?;
    assert!(
        b.registry
            .view_settings_current(b.owner, b.id, b.binding.generation, &snapshot)?
    );
    a.finish().await?;
    b.finish().await
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "registry_settings_source_tests.rs"]
mod source_tests;
use super::*;
use crate::registry::tests::{bootstrap, config, session};
use pulse_store::{
    StateStoreTask,
    settings_edit::TerminalSettingEdit,
    settings_scope::{ScopedSettingsEdit, SettingsScope},
};
use pulse_term::session::{DriverEndpoint, Lifecycle, SessionTask};
use uuid::Uuid;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn target(id: u128) -> std::result::Result<SettingsContextTarget, SettingsContextError> {
    SettingsContextTarget::new(None, Some(SettingsScope::Host(Uuid::from_u128(id))))
}
struct Fixture {
    dir: tempfile::TempDir,
    store: StateStore,
    store_task: StateStoreTask,
    registry: SessionRegistry,
    owner: WindowToken,
    other: WindowToken,
    id: SessionId,
    binding: RegisteredAttachment,
    actor: SessionTask,
    observer: SessionHandle,
    driver: DriverEndpoint,
}
impl Fixture {
    async fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        let (store, store_task) = StateStore::open(dir.path().join("state.db")).await?;
        for (id, size) in [(2, 22), (3, 26)] {
            store
                .edit_scoped_settings(
                    SettingsScope::Host(Uuid::from_u128(id)),
                    ScopedSettingsEdit::Set(TerminalSettingEdit::FontSize(size)),
                )
                .await?;
        }
        let registry = SessionRegistry::new();
        let owner = registry.open_window()?;
        let other = registry.open_window()?;
        let (handle, actor, driver) = session();
        let observer = handle.clone();
        let id = registry.register(owner, handle)?;
        let mut binding = registry.attach(owner, id, config()).await?;
        bootstrap(&registry, owner, id, &mut binding).await;
        Ok(Self {
            dir,
            store,
            store_task,
            registry,
            owner,
            other,
            id,
            binding,
            actor,
            observer,
            driver,
        })
    }
    fn configure(&self) -> std::result::Result<(), RegistrySettingsError> {
        self.registry.configure_view_settings(
            self.owner,
            self.id,
            self.binding.generation,
            target(2)?,
            SettingsPatch::default(),
        )
    }
    async fn finish(self) -> TestResult {
        self.registry.shutdown()?;
        assert_eq!(self.actor.join().await?.lifecycle, Lifecycle::Closed);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), self.binding.completion)
            .await?;
        self.store_task.shutdown().await?;
        drop(self.driver);
        drop(self.store);
        drop(self.dir);
        Ok(())
    }
}
#[tokio::test]
async fn authorized_window_and_generation_get_sources_without_terminal_io_or_credit() -> TestResult
{
    let mut f = Fixture::new().await?;
    assert!(matches!(
        f.registry
            .begin_view_settings(f.owner, f.id, f.binding.generation),
        Err(RegistrySettingsError::Unconfigured)
    ));
    f.configure()?;
    let candidate = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?
        .read(&f.store)
        .await?;
    assert!(
        f.registry
            .last_view_settings(f.owner, f.id, f.binding.generation)?
            .is_none()
    );
    let snapshot =
        f.registry
            .confirm_view_settings(f.owner, f.id, f.binding.generation, candidate)?;
    assert_eq!(snapshot.settings().settings().font_size.value, 22);
    assert!(
        f.registry
            .view_settings_current(f.owner, f.id, f.binding.generation, &snapshot)?
    );
    assert!(f.binding.wire.try_recv().is_err());
    assert!(f.driver.input.try_recv().is_err());
    assert_eq!(f.observer.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}
#[tokio::test]
async fn wrong_owner_generation_and_invalid_config_cannot_reset_pending_context() -> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let pending = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?;
    assert!(matches!(
        f.registry.configure_view_settings(
            f.other,
            f.id,
            f.binding.generation,
            target(3)?,
            SettingsPatch::default()
        ),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    assert!(matches!(
        f.registry.configure_view_settings(
            f.owner,
            f.id,
            f.binding.generation + 1,
            target(3)?,
            SettingsPatch::default()
        ),
        Err(RegistrySettingsError::Registry(RegistryError::Stale))
    ));
    let mut bad = SettingsPatch::default();
    bad.terminal.font_size = Some(999);
    assert!(matches!(
        f.registry
            .configure_view_settings(f.owner, f.id, f.binding.generation, target(3)?, bad),
        Err(RegistrySettingsError::Context(
            SettingsContextError::Invalid
        ))
    ));
    f.configure()?;
    assert!(matches!(
        f.registry
            .begin_view_settings(f.owner, f.id, f.binding.generation),
        Err(RegistrySettingsError::Context(SettingsContextError::Busy))
    ));
    let snapshot = f.registry.confirm_view_settings(
        f.owner,
        f.id,
        f.binding.generation,
        pending.read(&f.store).await?,
    )?;
    assert_eq!(snapshot.settings().settings().font_size.value, 22);
    f.finish().await
}
#[tokio::test]
async fn trusted_target_switch_reuses_owner_and_keeps_old_slot_until_retired() -> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let pending = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?;
    f.registry.configure_view_settings(
        f.owner,
        f.id,
        f.binding.generation,
        target(3)?,
        SettingsPatch::default(),
    )?;
    assert!(matches!(
        f.registry
            .begin_view_settings(f.owner, f.id, f.binding.generation),
        Err(RegistrySettingsError::Context(SettingsContextError::Busy))
    ));
    assert!(matches!(
        pending.read(&f.store).await,
        Err(RegistrySettingsError::Context(SettingsContextError::Stale))
    ));
    let snapshot = f.registry.confirm_view_settings(
        f.owner,
        f.id,
        f.binding.generation,
        f.registry
            .begin_view_settings(f.owner, f.id, f.binding.generation)?
            .read(&f.store)
            .await?,
    )?;
    assert_eq!(snapshot.settings().settings().font_size.value, 26);
    f.finish().await
}
#[tokio::test]
async fn detached_view_rejects_admitted_capture_and_keeps_actor_running() -> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let pending = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?;
    f.registry.detach(f.owner, f.id, f.binding.generation)?;
    assert!(matches!(
        pending.read(&f.store).await,
        Err(RegistrySettingsError::Registry(RegistryError::Stale))
    ));
    assert_eq!(f.observer.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}
#[tokio::test]
async fn fresh_generation_does_not_inherit_previous_settings_candidate_or_configuration()
-> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let candidate = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?
        .read(&f.store)
        .await?;
    let mut next = f.registry.attach(f.owner, f.id, config()).await?;
    bootstrap(&f.registry, f.owner, f.id, &mut next).await;
    assert!(matches!(
        f.registry
            .confirm_view_settings(f.owner, f.id, f.binding.generation, candidate),
        Err(RegistrySettingsError::Registry(RegistryError::Stale))
    ));
    assert!(matches!(
        f.registry
            .begin_view_settings(f.owner, f.id, next.generation),
        Err(RegistrySettingsError::Unconfigured)
    ));
    f.registry.configure_view_settings(
        f.owner,
        f.id,
        next.generation,
        target(3)?,
        SettingsPatch::default(),
    )?;
    let current = f.registry.confirm_view_settings(
        f.owner,
        f.id,
        next.generation,
        f.registry
            .begin_view_settings(f.owner, f.id, next.generation)?
            .read(&f.store)
            .await?,
    )?;
    assert_eq!(current.settings().settings().font_size.value, 26);
    f.finish().await
}
#[tokio::test]
async fn navigation_retirement_rejects_old_confirm_without_closing_window_session_or_store()
-> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let candidate = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?
        .read(&f.store)
        .await?;
    assert_eq!(f.registry.retire_window_views(f.owner)?, 1);
    assert!(matches!(
        f.registry
            .confirm_view_settings(f.owner, f.id, f.binding.generation, candidate),
        Err(RegistrySettingsError::Registry(RegistryError::Stale))
    ));
    assert_eq!(
        f.registry.state(f.owner, f.id)?.lifecycle,
        Lifecycle::Running
    );
    assert_eq!(
        f.store
            .load_scoped_settings(SettingsScope::Host(Uuid::from_u128(2)))
            .await?
            .revision(),
        1
    );
    f.finish().await
}
#[tokio::test]
async fn trusted_transfer_revokes_old_candidate_and_new_window_can_rebind_same_actor() -> TestResult
{
    let f = Fixture::new().await?;
    f.configure()?;
    let old = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?
        .read(&f.store)
        .await?;
    f.registry.transfer(f.owner, f.other, f.id)?;
    assert!(matches!(
        f.registry
            .confirm_view_settings(f.owner, f.id, f.binding.generation, old),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    let mut next = f.registry.attach(f.other, f.id, config()).await?;
    bootstrap(&f.registry, f.other, f.id, &mut next).await;
    f.registry.configure_view_settings(
        f.other,
        f.id,
        next.generation,
        target(2)?,
        SettingsPatch::default(),
    )?;
    let current = f.registry.confirm_view_settings(
        f.other,
        f.id,
        next.generation,
        f.registry
            .begin_view_settings(f.other, f.id, next.generation)?
            .read(&f.store)
            .await?,
    )?;
    assert_eq!(current.settings().settings().font_size.value, 22);
    f.registry.close_window(f.owner)?;
    assert!(
        f.registry
            .view_settings_current(f.other, f.id, next.generation, &current)?
    );
    assert_eq!(f.observer.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}
#[tokio::test]
async fn destroyed_window_token_cannot_confirm_in_new_incarnation() -> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let candidate = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?
        .read(&f.store)
        .await?;
    f.registry.close_window(f.owner)?;
    let replacement = f.registry.open_window()?;
    assert_ne!(replacement, f.owner);
    assert!(matches!(
        f.registry
            .confirm_view_settings(f.owner, f.id, f.binding.generation, candidate),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    assert!(matches!(
        f.registry
            .begin_view_settings(replacement, f.id, f.binding.generation),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    assert_eq!(f.observer.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}
#[tokio::test]
async fn explicit_invalidation_retains_last_good_but_cannot_apply_old_snapshot() -> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let old = f.registry.confirm_view_settings(
        f.owner,
        f.id,
        f.binding.generation,
        f.registry
            .begin_view_settings(f.owner, f.id, f.binding.generation)?
            .read(&f.store)
            .await?,
    )?;
    assert!(matches!(
        f.registry
            .invalidate_view_settings(f.other, f.id, f.binding.generation),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    assert!(
        f.registry
            .view_settings_current(f.owner, f.id, f.binding.generation, &old)?
    );
    f.registry
        .invalidate_view_settings(f.owner, f.id, f.binding.generation)?;
    assert!(
        !f.registry
            .view_settings_current(f.owner, f.id, f.binding.generation, &old)?
    );
    assert_eq!(
        f.registry
            .last_view_settings(f.owner, f.id, f.binding.generation)?
            .ok_or("missing last good")?
            .settings()
            .settings()
            .font_size
            .value,
        22
    );
    f.finish().await
}
#[tokio::test]
async fn wrong_confirm_window_is_rejected_before_publication() -> TestResult {
    let f = Fixture::new().await?;
    f.configure()?;
    let candidate = f
        .registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?
        .read(&f.store)
        .await?;
    assert!(matches!(
        f.registry
            .confirm_view_settings(f.other, f.id, f.binding.generation, candidate),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    assert!(
        f.registry
            .last_view_settings(f.owner, f.id, f.binding.generation)?
            .is_none()
    );
    let current = f.registry.confirm_view_settings(
        f.owner,
        f.id,
        f.binding.generation,
        f.registry
            .begin_view_settings(f.owner, f.id, f.binding.generation)?
            .read(&f.store)
            .await?,
    )?;
    assert!(
        f.registry
            .view_settings_current(f.owner, f.id, f.binding.generation, &current)?
    );
    f.finish().await
}
