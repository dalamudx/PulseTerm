//! Page-bound automatic Profile editing; each read permits one original-source save only.
use super::*;
use pulse_store::profile_editor::{ProfileConfig, ProfileEditorSnapshot};
use pulse_store::profiles::AutomaticShell;
use std::time::{Duration, Instant};
use uuid::Uuid;
pub(super) struct EditorTicket {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    store: StateStore,
    snapshot: ProfileEditorSnapshot,
    catalog: pulse_pty::profiles::AutoProfileCatalog,
    kind: pulse_pty::profiles::ShellKind,
    deadline: Instant,
    active: Arc<AtomicBool>,
}
impl Drop for EditorTicket {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
pub(super) struct EditorControl {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    active: Arc<AtomicBool>,
}
impl Drop for EditorControl {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorOffer {
    ticket: Uuid,
    shell: AutomaticShell,
    config: ProfileConfig,
}
fn kind(shell: AutomaticShell) -> pulse_pty::profiles::ShellKind {
    use pulse_pty::profiles::ShellKind;
    match shell {
        AutomaticShell::Bash => ShellKind::Bash,
        AutomaticShell::Zsh => ShellKind::Zsh,
        AutomaticShell::Fish => ShellKind::Fish,
        AutomaticShell::Sh => ShellKind::Sh,
        AutomaticShell::Pwsh => ShellKind::Pwsh,
    }
}
fn options(config: &ProfileConfig) -> pulse_pty::profiles::AutoProfileLaunch {
    pulse_pty::profiles::AutoProfileLaunch {
        args: config.args.iter().map(Into::into).collect(),
        cwd: config.cwd.as_ref().map(Into::into),
        env: config
            .env
            .iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        env_remove: config.env_remove.iter().map(Into::into).collect(),
        ..Default::default()
    }
}
impl ProfileReader {
    pub(in super::super) async fn editor_read(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        source: (PathBuf, OsString, Option<PathBuf>),
        shell: AutomaticShell,
    ) -> Result<EditorOffer> {
        self.check(&registry, &page)?;
        let credit = self.reserve_list(&registry, &page).await?;
        let reader = self.clone();
        tokio::spawn(async move {
            let (store, candidates) = reader
                .prepare(
                    &registry,
                    &page,
                    source.0,
                    source.1,
                    source.2,
                    credit.clone(),
                )
                .await?;
            let snapshot = store
                .load_profile_editor_retaining(shell, credit.clone())
                .await
                .map_err(|_| "workspace.unavailable")?;
            let catalog = pulse_app::profile_state::restore_catalog(
                snapshot.lifecycle().preferences(),
                &candidates,
            )
            .map_err(|_| "workspace.denied")?;
            let kind = kind(shell);
            if !catalog
                .profile(kind)
                .is_some_and(|profile| profile.is_available())
            {
                return Err("workspace.denied");
            }
            reader.check(&registry, &page)?;
            let config = snapshot.config();
            let id = Uuid::new_v4();
            let ticket = EditorTicket {
                id,
                page: page.clone(),
                store,
                snapshot,
                catalog,
                kind,
                deadline: Instant::now() + Duration::from_secs(60),
                active: Arc::new(AtomicBool::new(true)),
            };
            let mut bank = reader
                .editor_ticket
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            reader.check(&registry, &page)?;
            *bank = Some(ticket);
            Ok(EditorOffer {
                ticket: id,
                shell,
                config,
            })
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    pub(in super::super) fn editor_cancel(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
    ) -> Result<()> {
        self.check(registry, page)?;
        let mut bank = self
            .editor_ticket
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if bank
            .as_ref()
            .is_some_and(|t| t.id == id && Arc::ptr_eq(&t.page, page))
        {
            bank.take();
        }
        let mut active = self
            .editor_active
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if active
            .as_ref()
            .is_some_and(|t| t.id == id && Arc::ptr_eq(&t.page, page))
        {
            active.take();
        }
        Ok(())
    }
    pub(in super::super) async fn editor_save(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        id: Uuid,
        config: ProfileConfig,
    ) -> Result<()> {
        config.validate().map_err(|_| "workspace.profile_invalid")?;
        self.check(&registry, &page)?;
        let ticket = {
            let mut bank = self
                .editor_ticket
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            let t = bank.as_ref().ok_or("workspace.denied")?;
            if t.id != id || !Arc::ptr_eq(&t.page, &page) || Instant::now() >= t.deadline {
                return Err("workspace.denied");
            }
            t.catalog
                .prepare_launch(
                    t.kind,
                    options(&config),
                    &pulse_pty::launch::LocalSessionEnvironment {
                        application_version: env!("CARGO_PKG_VERSION").into(),
                        pane: None,
                    },
                )
                .map_err(|_| "workspace.profile_invalid")?;
            let mut active = self
                .editor_active
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            *active = Some(EditorControl {
                id,
                page: page.clone(),
                active: t.active.clone(),
            });
            bank.take().ok_or("workspace.denied")?
        };
        let credit = self.reserve_list(&registry, &page).await?;
        let reader = self.clone();
        tokio::spawn(async move {
            reader.check(&registry, &page)?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let active = ticket.active.clone();
            let deadline = ticket.deadline;
            ticket
                .store
                .save_profile_editor(ticket.snapshot.clone(), config, move || {
                    let _retain_until_sql_exit = &credit;
                    active.load(Ordering::SeqCst)
                        && Instant::now() < deadline
                        && gate_reader.check(&gate_registry, &gate_page).is_ok()
                })
                .await
                .map_err(|error| match error {
                    pulse_store::StoreError::Conflict => "workspace.profiles_conflict",
                    pulse_store::StoreError::ProfileAuthorization => "workspace.denied",
                    pulse_store::StoreError::Busy => "workspace.busy",
                    _ => "workspace.unavailable",
                })?;
            reader.check(&registry, &page)?;
            Ok(())
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
}
#[cfg(all(test, unix))]
#[path = "product_profiles_editor_tests.rs"]
mod tests;
