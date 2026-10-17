//! Feature-only native-window configuration receipt. No configuration RPC or user files.
use super::*;
use pulse_app::registry::{RegisteredSettingsCandidate, SourceBoundSettingsSnapshot};
use pulse_app::settings_context::SettingsContextTarget;
use pulse_store::settings::SettingsSource;
use pulse_store::settings_edit::TerminalSettingEdit;
use pulse_store::settings_scope::{ScopedSettingsEdit, SettingsScope};
use pulse_store::settings_source::{
    SettingsSourceHandle, SettingsSourceTask, UserSettingsSnapshot,
};
use pulse_store::{StateStore, StateStoreTask};

pub(super) struct Fixture {
    theme: theme_probe::Fixture,
    source_task: SettingsSourceTask,
    store_task: StateStoreTask,
    _dir: tempfile::TempDir,
    store: StateStore,
    source: SettingsSourceHandle,
    user: UserSettingsSnapshot,
    published: SourceBoundSettingsSnapshot,
    pending: Option<RegisteredSettingsCandidate>,
    main: WebviewWindow,
    owner: WindowToken,
    generation: u64,
    nonce: String,
    page_nonce: String,
}
fn target(host: bool) -> Result<SettingsContextTarget> {
    SettingsContextTarget::new(
        Some(SettingsScope::Group(uuid::Uuid::from_u128(1))),
        host.then_some(SettingsScope::Host(uuid::Uuid::from_u128(2))),
    )
    .map_err(error)
}
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err("session.native_settings_invalid".into())
    }
}
fn private_fixture() -> Result<tempfile::TempDir> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let dir = tempfile::tempdir().map_err(error)?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .map_err(error)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.path().join("settings.toml"))
            .map_err(error)?;
        file.write_all(b"[terminal]\nfont_size=18\n")
            .map_err(error)?;
        Ok(dir)
    }
    #[cfg(not(unix))]
    {
        Err("session.native_settings_unsupported".into())
    }
}
fn verify(snapshot: &SourceBoundSettingsSnapshot, host: bool) -> Result<()> {
    let p = snapshot.settings();
    require(
        snapshot.user_revision() == 1
            && p.settings().font_size.value == if host { 22 } else { 20 }
            && p.settings().font_size.source
                == if host {
                    SettingsSource::Host
                } else {
                    SettingsSource::Group
                }
            && p.database_revisions() == (Some(1), host.then_some(1)),
    )
}
pub(super) async fn prepare(
    state: &WindowSessions,
    window: &WebviewWindow,
    id: SessionId,
    generation: u64,
    pages: &reload::PageTracker,
) -> Result<Fixture> {
    let owner = state.caller(window)?;
    let nonce = state
        .0
        .active
        .lock()
        .map_err(error)?
        .as_ref()
        .ok_or("session.forbidden")?
        .nonce
        .clone();
    let page_nonce = pages.injection()?.0;
    let (actual, actual_id, _) = peer_view::owned(state, window, &nonce, &page_nonce)?;
    require(actual == owner && actual_id == id)?;
    let dir = tokio::task::spawn_blocking(private_fixture)
        .await
        .map_err(error)??;
    let (store, store_task) = StateStore::open(dir.path().join("state.db"))
        .await
        .map_err(error)?;
    for (scope, size) in [
        (SettingsScope::Group(uuid::Uuid::from_u128(1)), 20),
        (SettingsScope::Host(uuid::Uuid::from_u128(2)), 22),
    ] {
        store
            .edit_scoped_settings(
                scope,
                ScopedSettingsEdit::Set(TerminalSettingEdit::FontSize(size)),
            )
            .await
            .map_err(error)?;
    }
    let (source, source_task) =
        pulse_store::settings_source::open_user_settings_source(dir.path().to_path_buf())
            .await
            .map_err(error)?;
    let user = source.snapshot().map_err(error)?;
    let registry = &state.0.registry;
    registry
        .configure_view_settings(owner, id, generation, target(true)?, user.patch().clone())
        .map_err(error)?;
    let candidate = registry
        .begin_view_settings(owner, id, generation)
        .map_err(error)?
        .read(&store)
        .await
        .map_err(error)?;
    // Recheck the actual runtime page after asynchronous database work.
    peer_view::owned(state, window, &nonce, &page_nonce)?;
    let published = registry
        .confirm_view_settings_from_source(owner, id, generation, &source, &user, candidate)
        .map_err(error)?;
    verify(&published, true)?;
    require(
        registry
            .source_view_settings_current(owner, id, generation, &source, &published)
            .map_err(error)?,
    )?;
    let main = state
        .0
        .active
        .lock()
        .map_err(error)?
        .as_ref()
        .ok_or("session.forbidden")?
        .owner;
    require(main != owner && registry.begin_view_settings(main, id, generation).is_err())?;
    let pending = registry
        .begin_view_settings(owner, id, generation)
        .map_err(error)?
        .read(&store)
        .await
        .map_err(error)?;
    let theme = theme_probe::Fixture::prepare(
        registry,
        owner,
        main,
        id,
        generation,
        dir.path().to_path_buf(),
    )
    .await?;
    peer_view::owned(state, window, &nonce, &page_nonce)?;
    theme_delivery::deliver(state, window, &nonce, &page_nonce, generation).await?;
    eprintln!("session.native_settings.initial_checked");
    Ok(Fixture {
        theme,
        source_task,
        store_task,
        _dir: dir,
        store,
        source,
        user,
        published,
        pending: Some(pending),
        main: window
            .app_handle()
            .get_webview_window("main")
            .ok_or("session.native_settings_main_missing")?,
        owner,
        generation,
        nonce,
        page_nonce,
    })
}
impl Fixture {
    // Called only after actual target Channel draw/ACK, before its lease detaches.
    pub(super) async fn transferred(
        mut self,
        state: &WindowSessions,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<()> {
        require(
            state.caller(&self.main)? == owner
                && owner != self.owner
                && generation > self.generation,
        )?;
        let registry = &state.0.registry;
        require(registry.state(self.owner, id).is_err())?;
        require(self.source.is_current(&self.user))?;
        require(
            registry
                .source_view_settings_current(
                    self.owner,
                    id,
                    self.generation,
                    &self.source,
                    &self.published,
                )
                .is_err(),
        )?;
        require(
            registry
                .confirm_view_settings_from_source(
                    self.owner,
                    id,
                    self.generation,
                    &self.source,
                    &self.user,
                    self.pending
                        .take()
                        .ok_or("session.native_settings_pending_missing")?,
                )
                .is_err(),
        )?;
        registry
            .configure_view_settings(
                owner,
                id,
                generation,
                SettingsContextTarget::new(None, None).map_err(error)?,
                self.user.patch().clone(),
            )
            .map_err(error)?;
        let candidate = registry
            .begin_view_settings(owner, id, generation)
            .map_err(error)?
            .read(&self.store)
            .await
            .map_err(error)?;
        require(state.caller(&self.main)? == owner)?;
        let fresh = registry
            .confirm_view_settings_from_source(
                owner,
                id,
                generation,
                &self.source,
                &self.user,
                candidate,
            )
            .map_err(error)?;
        let p = fresh.settings();
        require(
            fresh.user_revision() == 1
                && p.settings().font_size.value == 18
                && p.settings().font_size.source == SettingsSource::User
                && p.database_revisions() == (None, None),
        )?;
        require(
            registry
                .source_view_settings_current(owner, id, generation, &self.source, &fresh)
                .map_err(error)?,
        )?;
        // Hold a candidate from the old source across a real private-file CAS edit.
        let stale = registry
            .begin_view_settings(owner, id, generation)
            .map_err(error)?
            .read(&self.store)
            .await
            .map_err(error)?;
        let directory = self._dir.path().to_path_buf();
        tokio::task::spawn_blocking(move || -> Result<()> {
            use pulse_store::settings_file::{SettingsFile, SettingsMutation};
            let file = SettingsFile::open_private(directory).map_err(error)?;
            let expected = file.load().map_err(error)?;
            file.save(
                &expected,
                SettingsMutation::Set(TerminalSettingEdit::FontSize(19)),
            )
            .map_err(error)?;
            Ok(())
        })
        .await
        .map_err(error)??;
        let mut changes = self.source.subscribe().map_err(error)?;
        self.source.invalidate().map_err(error)?;
        require(
            !registry
                .source_view_settings_current(owner, id, generation, &self.source, &fresh)
                .map_err(error)?,
        )?;
        let latest = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let status = *changes.borrow_and_update();
                match status {
                    pulse_store::settings_source::SettingsSourceStatus::Ready => {
                        let user = self.source.snapshot().map_err(error)?;
                        require(
                            user.revision() == 2 && user.patch().terminal.font_size == Some(19),
                        )?;
                        break Ok::<_, String>(user);
                    }
                    pulse_store::settings_source::SettingsSourceStatus::Pending => {}
                    _ => return Err("session.native_settings_source_rejected".into()),
                }
                changes.changed().await.map_err(error)?;
            }
        })
        .await
        .map_err(|_| "session.native_settings_source_deadline".to_string())??;
        require(matches!(
            registry.confirm_view_settings_from_source(
                owner,
                id,
                generation,
                &self.source,
                &self.user,
                stale
            ),
            Err(pulse_app::registry::RegistrySettingsError::Source(
                pulse_store::settings_source::SettingsSourceError::Stale
            ))
        ))?;
        require(state.caller(&self.main)? == owner)?;
        registry
            .configure_view_settings(
                owner,
                id,
                generation,
                SettingsContextTarget::new(None, None).map_err(error)?,
                latest.patch().clone(),
            )
            .map_err(error)?;
        let candidate = registry
            .begin_view_settings(owner, id, generation)
            .map_err(error)?
            .read(&self.store)
            .await
            .map_err(error)?;
        let fresh = registry
            .confirm_view_settings_from_source(
                owner,
                id,
                generation,
                &self.source,
                &latest,
                candidate,
            )
            .map_err(error)?;
        let projection = registry
            .read_view_settings_from_source(owner, id, generation, &self.source, &fresh)
            .map_err(error)?;
        require(
            fresh.user_revision() == 2
                && projection.settings().font_size.value == 19
                && projection.settings().font_size.source == SettingsSource::User
                && projection.database_revisions() == (None, None),
        )?;
        require(
            registry
                .source_view_settings_current(owner, id, generation, &self.source, &fresh)
                .map_err(error)?,
        )?;
        eprintln!("session.native_settings.source_revalidated");
        self.theme
            .transferred(state, &self.main, &self.nonce, owner, id, generation)
            .await?;
        require(state.caller(&self.main)? == owner)?;
        self.source_task.shutdown().await.map_err(error)?;
        require(
            !registry
                .source_view_settings_current(owner, id, generation, &self.source, &fresh)
                .map_err(error)?,
        )?;
        self.store
            .load_settings_layers(None, None)
            .await
            .map_err(error)?;
        self.store_task.shutdown().await.map_err(error)?;
        require(registry.state(owner, id).map_err(error)?.lifecycle == Lifecycle::Running)?;
        eprintln!("session.native_settings.target_checked");
        Ok(())
    }
    pub(super) async fn refreshed(
        mut self,
        state: &WindowSessions,
        window: &WebviewWindow,
        id: SessionId,
        generation: u64,
        pages: &reload::PageTracker,
    ) -> Result<Self> {
        require(state.caller(window)? == self.owner && generation > self.generation)?;
        require(peer_view::owned(state, window, &self.nonce, &self.page_nonce).is_err())?;
        let registry = &state.0.registry;
        require(
            registry
                .source_view_settings_current(
                    self.owner,
                    id,
                    self.generation,
                    &self.source,
                    &self.published,
                )
                .is_err(),
        )?;
        require(
            registry
                .confirm_view_settings_from_source(
                    self.owner,
                    id,
                    self.generation,
                    &self.source,
                    &self.user,
                    self.pending
                        .take()
                        .ok_or("session.native_settings_pending_missing")?,
                )
                .is_err(),
        )?;
        require(self.source.is_current(&self.user))?;
        let page_nonce = pages.injection()?.0;
        let (owner, actual_id, _) = peer_view::owned(state, window, &self.nonce, &page_nonce)?;
        require(owner == self.owner && actual_id == id)?;
        registry
            .configure_view_settings(
                owner,
                id,
                generation,
                target(false)?,
                self.user.patch().clone(),
            )
            .map_err(error)?;
        let candidate = registry
            .begin_view_settings(owner, id, generation)
            .map_err(error)?
            .read(&self.store)
            .await
            .map_err(error)?;
        peer_view::owned(state, window, &self.nonce, &page_nonce)?;
        let fresh = registry
            .confirm_view_settings_from_source(
                owner,
                id,
                generation,
                &self.source,
                &self.user,
                candidate,
            )
            .map_err(error)?;
        verify(&fresh, false)?;
        require(
            registry
                .source_view_settings_current(owner, id, generation, &self.source, &fresh)
                .map_err(error)?,
        )?;
        require(registry.state(owner, id).map_err(error)?.lifecycle == Lifecycle::Running)?;
        self.pending = Some(
            registry
                .begin_view_settings(owner, id, generation)
                .map_err(error)?
                .read(&self.store)
                .await
                .map_err(error)?,
        );
        self.published = fresh;
        self.generation = generation;
        self.page_nonce = page_nonce;
        self.theme
            .refreshed(registry, owner, id, generation)
            .await?;
        peer_view::owned(state, window, &self.nonce, &self.page_nonce)?;
        theme_delivery::deliver(state, window, &self.nonce, &self.page_nonce, generation).await?;
        eprintln!("session.native_settings.reload_checked");
        Ok(self)
    }
}
