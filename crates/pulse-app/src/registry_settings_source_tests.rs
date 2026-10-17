use super::*;
use pulse_store::settings_source::{
    SettingsSourceError, SettingsSourceHandle, SettingsSourceStatus, SettingsSourceTask,
    UserSettingsSnapshot, open_user_settings_source,
};
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
type Layer = (
    SettingsSourceHandle,
    SettingsSourceTask,
    UserSettingsSnapshot,
);
fn write(f: &Fixture, text: &str) -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let path = f.dir.path().join("settings.toml");
    std::fs::write(&path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}
async fn layer(f: &Fixture) -> std::result::Result<Layer, Box<dyn std::error::Error>> {
    write(f, "terminal.font_size=18")?;
    let (h, t) = open_user_settings_source(f.dir.path().into()).await?;
    let user = h.snapshot()?;
    Ok((h, t, user))
}
async fn ready(h: &SettingsSourceHandle) -> TestResult {
    let mut watch = h.subscribe()?;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        watch.wait_for(|s| *s == SettingsSourceStatus::Ready),
    )
    .await??;
    Ok(())
}
async fn read(
    f: &Fixture,
    user: &SettingsPatch,
) -> std::result::Result<RegisteredSettingsCandidate, Box<dyn std::error::Error>> {
    f.registry.configure_view_settings(
        f.owner,
        f.id,
        f.binding.generation,
        target(2)?,
        user.clone(),
    )?;
    Ok(f.registry
        .begin_view_settings(f.owner, f.id, f.binding.generation)?
        .read(&f.store)
        .await?)
}
fn confirm(
    f: &Fixture,
    h: &SettingsSourceHandle,
    user: &UserSettingsSnapshot,
    c: RegisteredSettingsCandidate,
) -> Result<SourceBoundSettingsSnapshot> {
    f.registry
        .confirm_view_settings_from_source(f.owner, f.id, f.binding.generation, h, user, c)
}
fn current(f: &Fixture, h: &SettingsSourceHandle, s: &SourceBoundSettingsSnapshot) -> Result<bool> {
    f.registry
        .source_view_settings_current(f.owner, f.id, f.binding.generation, h, s)
}
#[tokio::test]
async fn atomic_source_projection_captures_values_then_refuses_reverified_source() -> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let (h, t, user) = layer(&f).await?;
    let c = read(&f, user.patch()).await?;
    let s = confirm(&f, &h, &user, c)?;
    let captured =
        f.registry
            .read_view_settings_from_source(f.owner, f.id, f.binding.generation, &h, &s)?;
    assert_eq!(captured.settings().font_size.value, 22);
    assert_eq!(captured.database_revisions(), (None, Some(1)));
    h.invalidate()?;
    ready(&h).await?;
    assert_eq!(h.snapshot()?.revision(), 1);
    assert!(matches!(
        f.registry
            .read_view_settings_from_source(f.owner, f.id, f.binding.generation, &h, &s),
        Err(RegistrySettingsError::Source(SettingsSourceError::Stale))
    ));
    assert_eq!(captured.settings().font_size.value, 22); // capture remains an observation, not current authority
    t.shutdown().await?;
    f.finish().await
}
#[tokio::test]
async fn atomic_source_projection_rejects_same_generation_reconfiguration() -> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let (h, t, user) = layer(&f).await?;
    let c = read(&f, user.patch()).await?;
    let s = confirm(&f, &h, &user, c)?;
    f.registry.configure_view_settings(
        f.owner,
        f.id,
        f.binding.generation,
        target(2)?,
        user.patch().clone(),
    )?;
    assert!(
        f.registry
            .read_view_settings_from_source(f.owner, f.id, f.binding.generation, &h, &s)
            .is_ok()
    ); // exact configure is a no-op
    f.registry.configure_view_settings(
        f.owner,
        f.id,
        f.binding.generation,
        target(3)?,
        user.patch().clone(),
    )?;
    assert!(h.is_current(&user));
    assert!(matches!(
        f.registry
            .read_view_settings_from_source(f.owner, f.id, f.binding.generation, &h, &s),
        Err(RegistrySettingsError::Registry(RegistryError::Stale))
    ));
    t.shutdown().await?;
    f.finish().await
}
#[tokio::test]
async fn atomic_source_projection_authorizes_before_revealing_source_closed() -> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let other = f.registry.open_window()?;
    let (h, t, user) = layer(&f).await?;
    let c = read(&f, user.patch()).await?;
    let s = confirm(&f, &h, &user, c)?;
    t.shutdown().await?;
    assert!(matches!(
        f.registry
            .read_view_settings_from_source(other, f.id, f.binding.generation, &h, &s),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    assert!(matches!(
        f.registry
            .read_view_settings_from_source(f.owner, f.id, f.binding.generation, &h, &s),
        Err(RegistrySettingsError::Source(SettingsSourceError::Closed))
    ));
    assert_eq!(f.observer.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}
#[tokio::test]
async fn current_source_and_view_confirm_with_exact_provenance() -> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let (h, t, user) = layer(&f).await?;
    let c = read(&f, user.patch()).await?;
    let s = confirm(&f, &h, &user, c)?;
    assert!(current(&f, &h, &s)?);
    assert_eq!(s.settings().settings().font_size.value, 22);
    assert_eq!(s.user_revision(), 1);
    assert_eq!(f.observer.state.borrow().parsed_bytes, 0);
    t.shutdown().await?;
    assert!(!current(&f, &h, &s)?);
    assert_eq!(f.observer.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}
#[tokio::test]
async fn masked_wrong_user_patch_is_rejected_even_when_resolved_host_font_is_identical()
-> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let (h, t, user) = layer(&f).await?;
    let mut wrong = user.patch().clone();
    wrong.terminal.font_size = Some(19);
    let c = read(&f, &wrong).await?;
    assert!(matches!(
        confirm(&f, &h, &user, c),
        Err(RegistrySettingsError::SourceMismatch)
    ));
    assert!(
        f.registry
            .last_view_settings(f.owner, f.id, f.binding.generation)?
            .is_none()
    );
    let c = read(&f, user.patch()).await?;
    let s = confirm(&f, &h, &user, c)?;
    assert!(current(&f, &h, &s)?);
    t.shutdown().await?;
    f.finish().await
}
#[tokio::test]
async fn same_semantic_source_reverification_cannot_revive_old_candidate_or_joint_receipt()
-> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let (h, t, user) = layer(&f).await?;
    let c = read(&f, user.patch()).await?;
    let old = confirm(&f, &h, &user, c)?;
    let c = read(&f, user.patch()).await?;
    write(&f, "# unchanged semantic layer\nterminal.font_size=18")?;
    h.invalidate()?;
    ready(&h).await?;
    assert!(matches!(
        confirm(&f, &h, &user, c),
        Err(RegistrySettingsError::Source(SettingsSourceError::Stale))
    ));
    assert!(!current(&f, &h, &old)?);
    let fresh = h.snapshot()?;
    assert_eq!(fresh.revision(), user.revision());
    let c = read(&f, fresh.patch()).await?;
    let new = confirm(&f, &h, &fresh, c)?;
    assert!(current(&f, &h, &new)?);
    t.shutdown().await?;
    f.finish().await
}
#[tokio::test]
async fn rejected_file_retains_last_good_but_never_authorizes_a_joint_publication() -> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let (h, t, user) = layer(&f).await?;
    let c = read(&f, user.patch()).await?;
    let old = confirm(&f, &h, &user, c)?;
    let c = read(&f, user.patch()).await?;
    write(&f, "terminal.font_size=999")?;
    let mut events = h.subscribe()?;
    h.invalidate()?;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        events.wait_for(|s| matches!(s, SettingsSourceStatus::Rejected(_))),
    )
    .await??;
    assert_eq!(h.snapshot()?.patch(), user.patch());
    assert!(!current(&f, &h, &old)?);
    assert!(matches!(
        confirm(&f, &h, &user, c),
        Err(RegistrySettingsError::Source(SettingsSourceError::Stale))
    ));
    assert!(
        f.registry
            .last_view_settings(f.owner, f.id, f.binding.generation)?
            .is_some()
    );
    t.shutdown().await?;
    f.finish().await
}
#[tokio::test]
async fn actual_window_and_retired_binding_permissions_are_checked_before_source_state()
-> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let (h, t, user) = layer(&f).await?;
    let c = read(&f, user.patch()).await?;
    t.shutdown().await?;
    assert!(matches!(
        f.registry.confirm_view_settings_from_source(
            f.other,
            f.id,
            f.binding.generation,
            &h,
            &user,
            c
        ),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ));
    let (h, t) = open_user_settings_source(f.dir.path().into()).await?;
    let fresh = h.snapshot()?;
    let c = read(&f, fresh.patch()).await?;
    let s = confirm(&f, &h, &fresh, c)?;
    let c = read(&f, fresh.patch()).await?;
    f.registry.retire_window_views(f.owner)?;
    assert!(matches!(
        confirm(&f, &h, &fresh, c),
        Err(RegistrySettingsError::Registry(RegistryError::Stale))
    ));
    assert!(matches!(
        current(&f, &h, &s),
        Err(RegistrySettingsError::Registry(RegistryError::Stale))
    ));
    assert_eq!(f.observer.state.borrow().lifecycle, Lifecycle::Running);
    t.shutdown().await?;
    f.finish().await
}
#[tokio::test]
async fn replacing_source_with_identical_revision_and_content_cannot_accept_foreign_proof()
-> TestResult {
    let _serial = SERIAL.lock().await;
    let f = Fixture::new().await?;
    let (h, t, user) = layer(&f).await?;
    let c = read(&f, user.patch()).await?;
    let old = confirm(&f, &h, &user, c)?;
    t.shutdown().await?;
    let (next, task) = open_user_settings_source(f.dir.path().into()).await?;
    let fresh = next.snapshot()?;
    assert_eq!(fresh.patch(), user.patch());
    assert_eq!(fresh.revision(), user.revision());
    let c = read(&f, user.patch()).await?;
    assert!(matches!(
        confirm(&f, &next, &user, c),
        Err(RegistrySettingsError::Source(SettingsSourceError::Stale))
    ));
    assert!(!current(&f, &next, &old)?);
    let c = read(&f, fresh.patch()).await?;
    let new = confirm(&f, &next, &fresh, c)?;
    assert!(current(&f, &next, &new)?);
    task.shutdown().await?;
    f.finish().await
}
