//! Original-page, bounded custom Profile editor. Metadata saving grants no execution authority.
use super::*;
use pulse_store::custom_profiles::{CustomProfileDraft, CustomProfileEdit, CustomProfileSnapshot};
use std::time::{Duration, Instant};
use uuid::Uuid;
#[derive(Default)]
pub(super) struct CustomEditor {
    bank: Mutex<Bank>,
}
#[derive(Default)]
struct Bank {
    ticket: Option<Ticket>,
    active: Option<Control>,
}
struct Ticket {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    store: StateStore,
    snapshot: CustomProfileSnapshot,
    deadline: Instant,
    active: Arc<AtomicBool>,
}
struct Control {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    active: Arc<AtomicBool>,
}
impl Drop for Ticket {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
impl CustomEditor {
    pub(super) fn revoke(&self) {
        if let Ok(mut bank) = self.bank.lock() {
            bank.ticket.take();
            bank.active.take();
        }
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomOffer {
    ticket: Uuid,
    profiles: Vec<CustomProfileDto>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomProfileDto {
    id: Uuid,
    name: String,
    executable: String,
    hidden: bool,
    deleted: bool,
    config: pulse_store::profile_editor::ProfileConfig,
}
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CustomAction {
    Create { draft: CustomProfileDraft },
    Update { id: Uuid, draft: CustomProfileDraft },
    Delete { id: Uuid },
    Restore { id: Uuid },
}
impl CustomAction {
    fn into_edit(self) -> CustomProfileEdit {
        match self {
            Self::Create { draft } => CustomProfileEdit::Create(draft),
            Self::Update { id, draft } => CustomProfileEdit::Update(id, draft),
            Self::Delete { id } => CustomProfileEdit::Delete(id),
            Self::Restore { id } => CustomProfileEdit::Restore(id),
        }
    }
}
pub(crate) struct CustomStart {
    pub cwd: Option<pulse_app::local_launch::LaunchDirectory>,
    pub source: (PathBuf, OsString, Option<PathBuf>),
    pub id: Uuid,
    pub startup: pulse_store::settings::TerminalStartup,
    pub interrupt: Option<tokio_util::sync::CancellationToken>,
}
impl ProfileReader {
    pub(in super::super) async fn custom_start(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        root: Arc<Mutex<pulse_app::local_session_root::LocalSessionRoot>>,
        source: CustomStart,
    ) -> Result<pulse_app::registry::SessionId> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let (store, _) = reader
                .prepare(
                    &registry,
                    &page,
                    source.source.0,
                    source.source.1,
                    source.source.2,
                    credit.clone(),
                )
                .await?;
            reader.check(&registry, &page)?;
            let context = pulse_pty::launch::LocalSessionEnvironment {
                application_version: env!("CARGO_PKG_VERSION").into(),
                pane: None,
            };
            let spec = pulse_app::local_launch::CustomProfileLaunch {
                cwd: source.cwd.as_ref(),
                id: source.id,
                context: &context,
                size: pulse_core::TermSize::new(80, 24).map_err(|_| "workspace.unavailable")?,
                store_retention: Box::new(credit.clone()),
            };
            let launch = pulse_app::local_session_root::start_custom_page_local_with_preset(
                &registry,
                &page,
                &store,
                &root,
                spec,
                source.startup,
            );
            let interrupt = async move {
                match source.interrupt {
                    Some(token) => token.cancelled().await,
                    None => std::future::pending::<()>().await,
                }
            };
            let id = tokio::select! { biased;
                _ = interrupt => return Err("workspace.denied"),
                result = launch => result.map_err(|_| "workspace.unavailable")?,
            };
            drop(credit);
            Ok(id)
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
}
#[derive(serde::Serialize)]
pub(crate) struct CustomBrief {
    pub(in super::super) id: Uuid,
    pub(in super::super) name: String,
    pub(in super::super) hidden: bool,
}
impl ProfileReader {
    pub(in super::super) async fn custom_list(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        source: (PathBuf, OsString, Option<PathBuf>),
    ) -> Result<Vec<CustomBrief>> {
        self.check(&registry, &page)?;
        let credit = self.reserve_list(&registry, &page).await?;
        let reader = self.clone();
        tokio::spawn(async move {
            let (store, _) = reader
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
                .load_custom_profiles_retaining(credit)
                .await
                .map_err(|_| "workspace.unavailable")?;
            reader.check(&registry, &page)?;
            Ok(snapshot
                .profiles()
                .iter()
                .filter(|profile| !profile.is_deleted())
                .map(|profile| CustomBrief {
                    id: profile.id(),
                    name: profile.draft().name.clone(),
                    hidden: profile.draft().hidden,
                })
                .collect())
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
}
impl ProfileReader {
    pub(in super::super) async fn custom_read(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        source: (PathBuf, OsString, Option<PathBuf>),
    ) -> Result<CustomOffer> {
        self.check(&registry, &page)?;
        let credit = self.reserve_list(&registry, &page).await?;
        let reader = self.clone();
        tokio::spawn(async move {
            let (store, _) = reader
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
                .load_custom_profiles_retaining(credit.clone())
                .await
                .map_err(|_| "workspace.unavailable")?;
            reader.check(&registry, &page)?;
            let profiles = snapshot
                .profiles()
                .iter()
                .map(|profile| CustomProfileDto {
                    id: profile.id(),
                    name: profile.draft().name.clone(),
                    executable: profile.draft().executable.clone(),
                    hidden: profile.draft().hidden,
                    deleted: profile.is_deleted(),
                    config: profile.draft().config.clone(),
                })
                .collect();
            let id = Uuid::new_v4();
            let ticket = Ticket {
                id,
                page: page.clone(),
                store,
                snapshot,
                deadline: Instant::now() + Duration::from_secs(60),
                active: Arc::new(AtomicBool::new(true)),
            };
            let mut bank = reader
                .custom_editor
                .bank
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            reader.check(&registry, &page)?;
            bank.active.take();
            bank.ticket = Some(ticket);
            Ok(CustomOffer {
                ticket: id,
                profiles,
            })
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    pub(in super::super) fn custom_cancel(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
    ) -> Result<()> {
        self.check(registry, page)?;
        let mut bank = self
            .custom_editor
            .bank
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if bank
            .ticket
            .as_ref()
            .is_some_and(|ticket| ticket.id == id && Arc::ptr_eq(&ticket.page, page))
        {
            bank.ticket.take();
        }
        if bank
            .active
            .as_ref()
            .is_some_and(|control| control.id == id && Arc::ptr_eq(&control.page, page))
        {
            bank.active.take();
        }
        Ok(())
    }
    pub(in super::super) async fn custom_save(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        id: Uuid,
        change: CustomAction,
    ) -> Result<()> {
        self.check(&registry, &page)?;
        let ticket = {
            let mut bank = self
                .custom_editor
                .bank
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            let ticket = bank.ticket.as_ref().ok_or("workspace.denied")?;
            if ticket.id != id
                || !Arc::ptr_eq(&ticket.page, &page)
                || Instant::now() >= ticket.deadline
            {
                return Err("workspace.denied");
            }
            let control = Control {
                id,
                page: page.clone(),
                active: ticket.active.clone(),
            };
            bank.active = Some(control);
            bank.ticket.take().ok_or("workspace.denied")?
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
                .edit_custom_profiles(ticket.snapshot.clone(), change.into_edit(), move || {
                    let _retain_until_sql_exit = &credit;
                    active.load(Ordering::SeqCst)
                        && Instant::now() < deadline
                        && gate_reader.check(&gate_registry, &gate_page).is_ok()
                })
                .await
                .map_err(|error| match error {
                    pulse_store::StoreError::Conflict => "workspace.profiles_conflict",
                    pulse_store::StoreError::ProfileAuthorization => "workspace.denied",
                    pulse_store::StoreError::Record => "workspace.profile_invalid",
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
