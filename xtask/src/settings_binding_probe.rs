//! Fixed registry-token/actor exercise, not a native-window or frontend render probe.
use super::*;
use pulse_app::{
    attachment::AttachmentConfig,
    registry::{
        RegisteredAttachment, RegistryError, RegistrySettingsError, SessionId, SessionRegistry,
        SourceBoundSettingsSnapshot, WindowToken,
    },
    settings_context::{SettingsContextError, SettingsContextTarget},
};
use pulse_core::TermSize;
use pulse_store::settings_source::{
    SettingsSourceHandle, SettingsSourceStatus, UserSettingsSnapshot,
};
use pulse_term::session::{self, ChannelTransport, Lifecycle};
fn config() -> AttachmentConfig {
    AttachmentConfig {
        fragment: 1024,
        frames: 1,
        bytes: 1100,
        ack_timeout: Duration::from_secs(2),
    }
}
async fn bootstrap(
    registry: &SessionRegistry,
    owner: WindowToken,
    id: SessionId,
    view: &mut RegisteredAttachment,
) -> Result<()> {
    loop {
        let bytes = tokio::time::timeout(Duration::from_secs(1), view.wire.recv())
            .await?
            .context("bootstrap stream ended")?;
        let frame = pulse_ipc::decode(&bytes)?;
        let part = pulse_ipc::attach::decode_fragment(frame.payload)?.part;
        registry.acknowledge(owner, id, view.generation, frame.sequence)?;
        if part == pulse_ipc::attach::Part::Commit {
            return Ok(());
        }
    }
}
fn group_target() -> std::result::Result<SettingsContextTarget, SettingsContextError> {
    SettingsContextTarget::new(Some(group()), None)
}
fn check_snapshot(
    registry: &SessionRegistry,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    source: &SettingsSourceHandle,
    snapshot: &SourceBoundSettingsSnapshot,
) -> Result<()> {
    let value = snapshot.settings();
    if !registry.source_view_settings_current(owner, id, generation, source, snapshot)?
        || value.settings().font_size.value != 20
        || value.settings().font_size.source != SettingsSource::Group
        || value.database_revisions() != (Some(3), None)
    {
        bail!("current view setting publication failed");
    }
    Ok(())
}
pub(super) async fn check(
    store: &StateStore,
    source: &SettingsSourceHandle,
    initial_proof: &UserSettingsSnapshot,
) -> Result<UserSettingsSnapshot> {
    let mut proof = initial_proof.clone();
    let user = proof.patch().clone();
    let registry = SessionRegistry::new();
    let first = registry.open_window()?;
    let second = registry.open_window()?;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = session::spawn_session(TermSize::new(32, 8)?, 64, transport);
    let observer = handle.clone();
    let id = registry.register(first, handle)?;
    let mut initial = registry.attach(first, id, config()).await?;
    bootstrap(&registry, first, id, &mut initial).await?;
    registry.configure_view_settings(
        first,
        id,
        initial.generation,
        SettingsContextTarget::new(Some(group()), Some(host()))?,
        user.clone(),
    )?;
    if !matches!(
        registry.begin_view_settings(second, id, initial.generation),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ) || !matches!(
        registry.begin_view_settings(first, id, initial.generation + 1),
        Err(RegistrySettingsError::Registry(RegistryError::Stale))
    ) {
        bail!("view settings admission ignored owner or generation");
    }
    let old = registry
        .begin_view_settings(first, id, initial.generation)?
        .read(store)
        .await?;
    registry.configure_view_settings(
        first,
        id,
        initial.generation,
        group_target()?,
        user.clone(),
    )?;
    if !matches!(
        registry.begin_view_settings(first, id, initial.generation),
        Err(RegistrySettingsError::Context(SettingsContextError::Busy))
    ) || !matches!(
        registry.confirm_view_settings_from_source(
            first,
            id,
            initial.generation,
            source,
            &proof,
            old
        ),
        Err(RegistrySettingsError::Context(SettingsContextError::Stale))
    ) {
        bail!("late context enabled or slot released early");
    }
    let current = registry.confirm_view_settings_from_source(
        first,
        id,
        initial.generation,
        source,
        &proof,
        registry
            .begin_view_settings(first, id, initial.generation)?
            .read(store)
            .await?,
    )?;
    check_snapshot(&registry, first, id, initial.generation, source, &current)?;
    let stale_user_candidate = registry
        .begin_view_settings(first, id, initial.generation)?
        .read(store)
        .await?;
    let mut ready = source.subscribe()?;
    source.invalidate()?;
    tokio::time::timeout(
        Duration::from_secs(1),
        ready.wait_for(|s| *s == SettingsSourceStatus::Ready),
    )
    .await??;
    if !matches!(
        registry.confirm_view_settings_from_source(
            first,
            id,
            initial.generation,
            source,
            &proof,
            stale_user_candidate
        ),
        Err(RegistrySettingsError::Source(SettingsSourceError::Stale))
    ) || registry.source_view_settings_current(
        first,
        id,
        initial.generation,
        source,
        &current,
    )? {
        bail!("same-model source verification revived old publication");
    }
    let fresh = source.snapshot()?;
    if fresh.revision() != proof.revision() || fresh.patch() != proof.patch() {
        bail!("verification changed semantic fixture");
    }
    proof = fresh;
    let verified = registry.confirm_view_settings_from_source(
        first,
        id,
        initial.generation,
        source,
        &proof,
        registry
            .begin_view_settings(first, id, initial.generation)?
            .read(store)
            .await?,
    )?;
    check_snapshot(&registry, first, id, initial.generation, source, &verified)?;
    let pending = registry
        .begin_view_settings(first, id, initial.generation)?
        .read(store)
        .await?;
    registry.transfer(first, second, id)?;
    if !matches!(
        registry.confirm_view_settings_from_source(
            first,
            id,
            initial.generation,
            source,
            &proof,
            pending
        ),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ) {
        bail!("old window confirmed after transfer");
    }
    let mut moved = registry.attach(second, id, config()).await?;
    bootstrap(&registry, second, id, &mut moved).await?;
    if !matches!(
        registry.begin_view_settings(second, id, moved.generation),
        Err(RegistrySettingsError::Unconfigured)
    ) {
        bail!("new view inherited old context");
    }
    registry.configure_view_settings(
        second,
        id,
        moved.generation,
        group_target()?,
        user.clone(),
    )?;
    let moved_settings = registry.confirm_view_settings_from_source(
        second,
        id,
        moved.generation,
        source,
        &proof,
        registry
            .begin_view_settings(second, id, moved.generation)?
            .read(store)
            .await?,
    )?;
    registry.close_window(first)?;
    check_snapshot(
        &registry,
        second,
        id,
        moved.generation,
        source,
        &moved_settings,
    )?;
    let pending = registry.begin_view_settings(second, id, moved.generation)?;
    if registry.retire_window_views(second)? != 1
        || !matches!(
            pending.read(store).await,
            Err(RegistrySettingsError::Registry(RegistryError::Stale))
        )
    {
        bail!("retired page capture survived");
    }
    driver.output.send(b"BACKGROUND".to_vec()).await?;
    let mut state = observer.state.clone();
    tokio::time::timeout(Duration::from_secs(1), async {
        while state.borrow().parsed_bytes != 10 {
            state.changed().await?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let mut restored = registry.attach(second, id, config()).await?;
    bootstrap(&registry, second, id, &mut restored).await?;
    registry.configure_view_settings(second, id, restored.generation, group_target()?, user)?;
    let restored_settings = registry.confirm_view_settings_from_source(
        second,
        id,
        restored.generation,
        source,
        &proof,
        registry
            .begin_view_settings(second, id, restored.generation)?
            .read(store)
            .await?,
    )?;
    check_snapshot(
        &registry,
        second,
        id,
        restored.generation,
        source,
        &restored_settings,
    )?;
    if registry.source_view_settings_current(
        second,
        id,
        restored.generation,
        source,
        &moved_settings,
    )? {
        bail!("old-generation publication revived");
    }
    registry.close_window(second)?;
    if !matches!(
        registry.source_view_settings_current(
            second,
            id,
            restored.generation,
            source,
            &restored_settings
        ),
        Err(RegistrySettingsError::Registry(RegistryError::Forbidden))
    ) || observer.state.borrow().lifecycle != Lifecycle::Running
        || store.load_scoped_settings(host()).await?.revision() != 1
    {
        bail!("window close failed lifetime separation");
    }
    registry.shutdown()?;
    if task.join().await?.lifecycle != Lifecycle::Closed {
        bail!("actor not supervised to explicit close");
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), restored.completion).await?;
    drop(driver);
    Ok(proof)
}
