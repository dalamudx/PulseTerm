use super::*;
use pulse_store::{profile_default::DefaultProfileSnapshot, profiles::AutomaticShell};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(super) struct DefaultTicket {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    snapshot: DefaultProfileSnapshot,
    offered: Vec<AutomaticShell>,
    deadline: Instant,
    custom_offered: Vec<Uuid>,
    active: Arc<AtomicBool>,
}
impl Drop for DefaultTicket {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
pub(super) struct DefaultControl {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    active: Arc<AtomicBool>,
}
impl Drop for DefaultControl {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
/// Page metadata only. Neither the ticket nor an available default grants launch permission.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DefaultProfileOffer {
    ticket: Uuid,
    selected: Option<AutomaticShell>,
    available: bool,
    offered: Vec<AutomaticShell>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    custom_offered: Vec<CustomDefault>,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_selected: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_name: Option<String>,
}
#[derive(serde::Serialize)]
struct CustomDefault {
    id: Uuid,
    name: String,
}
fn as_shell(kind: pulse_pty::profiles::ShellKind) -> AutomaticShell {
    use pulse_pty::profiles::ShellKind;
    match kind {
        ShellKind::Bash => AutomaticShell::Bash,
        ShellKind::Zsh => AutomaticShell::Zsh,
        ShellKind::Fish => AutomaticShell::Fish,
        ShellKind::Sh => AutomaticShell::Sh,
        ShellKind::Pwsh => AutomaticShell::Pwsh,
    }
}
impl ProfileReader {
    pub(in super::super) async fn default_read(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        path: OsString,
        shell: Option<PathBuf>,
    ) -> Result<DefaultProfileOffer> {
        self.check(&registry, &page)?;
        let credit = self.reserve_list(&registry, &page).await?;
        let reader = self.clone();
        tokio::spawn(async move {
            let (store, candidates) = reader
                .prepare(&registry, &page, directory, path, shell, credit.clone())
                .await?;
            let snapshot = store
                .load_default_profile_retaining(credit.clone())
                .await
                .map_err(|_| "workspace.unavailable")?;
            let catalog =
                pulse_app::profile_state::restore_catalog(snapshot.preferences(), &candidates)
                    .map_err(|_| "workspace.unavailable")?;
            let offered = candidates
                .iter()
                .filter(|candidate| {
                    catalog
                        .profile(candidate.kind())
                        .is_some_and(|profile| !profile.is_hidden() && profile.is_available())
                })
                .map(|candidate| as_shell(candidate.kind()))
                .collect::<Vec<_>>();
            let selected = snapshot.shell();
            let available =
                pulse_app::profile_state::resolve_default_profile(&snapshot, &candidates)
                    .ok()
                    .flatten()
                    .is_some();
            reader.check(&registry, &page)?;
            let custom_selected = match snapshot.selection() {
                Some(pulse_store::profile_default::DefaultProfileSelection::Custom(id)) => Some(id),
                _ => None,
            };
            let custom_name = custom_selected.and_then(|id| {
                snapshot
                    .custom_profiles()
                    .iter()
                    .find(|profile| profile.id() == id)
                    .map(|profile| profile.draft().name.clone())
            });
            let custom_candidates = snapshot
                .custom_profiles()
                .iter()
                .filter(|profile| !profile.is_deleted() && !profile.draft().hidden)
                .map(|profile| {
                    (
                        profile.id(),
                        profile.draft().name.clone(),
                        PathBuf::from(&profile.draft().executable),
                    )
                })
                .collect::<Vec<_>>();
            let retained = credit.clone();
            let custom_offered = tokio::task::spawn_blocking(move || {
                let _retain_until_native_exit = retained;
                custom_candidates
                    .into_iter()
                    .filter_map(|(id, name, program)| {
                        let metadata = std::fs::metadata(program).ok()?;
                        if !metadata.is_file() {
                            return None;
                        }
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if metadata.permissions().mode() & 0o111 == 0 {
                                return None;
                            }
                        }
                        Some(CustomDefault { id, name })
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(|_| "workspace.unavailable")?;
            let available = custom_selected.map_or(available, |id| {
                custom_offered.iter().any(|profile| profile.id == id)
            });
            let id = Uuid::new_v4();
            let ticket = DefaultTicket {
                id,
                page: page.clone(),
                snapshot,
                offered: offered.clone(),
                custom_offered: custom_offered.iter().map(|profile| profile.id).collect(),
                deadline: Instant::now() + Duration::from_secs(60),
                active: Arc::new(AtomicBool::new(true)),
            };
            let mut bank = reader
                .default_ticket
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            reader.check(&registry, &page)?;
            *bank = Some(ticket);
            drop(credit);
            Ok(DefaultProfileOffer {
                ticket: id,
                selected,
                available,
                offered,
                custom_offered,
                custom_selected,
                custom_name,
            })
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    pub(in super::super) fn default_cancel(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
    ) -> Result<()> {
        self.check(registry, page)?;
        let mut bank = self
            .default_ticket
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if bank
            .as_ref()
            .is_some_and(|ticket| ticket.id == id && Arc::ptr_eq(&ticket.page, page))
        {
            bank.take();
        }
        let mut active = self
            .default_active
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if active
            .as_ref()
            .is_some_and(|control| control.id == id && Arc::ptr_eq(&control.page, page))
        {
            active.take();
        }
        Ok(())
    }

    #[cfg(test)]
    pub(in super::super) async fn default_save(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        source: (PathBuf, OsString, Option<PathBuf>),
        id: Uuid,
        selected: Option<AutomaticShell>,
    ) -> Result<()> {
        self.default_save_choice(
            registry,
            page,
            source,
            id,
            selected.map(pulse_store::profile_default::DefaultProfileSelection::Automatic),
        )
        .await
    }
    pub(in super::super) async fn default_save_choice(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        source: (PathBuf, OsString, Option<PathBuf>),
        id: Uuid,
        selected: Option<pulse_store::profile_default::DefaultProfileSelection>,
    ) -> Result<()> {
        self.check(&registry, &page)?;
        let ticket = {
            let mut bank = self
                .default_ticket
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            let ticket = bank.as_ref().ok_or("workspace.denied")?;
            if ticket.id != id
                || !Arc::ptr_eq(&ticket.page, &page)
                || Instant::now() >= ticket.deadline
                || selected.is_some_and(|selection| match selection {
                    pulse_store::profile_default::DefaultProfileSelection::Automatic(shell) => {
                        !ticket.offered.contains(&shell)
                    }
                    pulse_store::profile_default::DefaultProfileSelection::Custom(id) => {
                        !ticket.custom_offered.contains(&id)
                    }
                })
            {
                return Err("workspace.denied");
            }
            let mut active = self
                .default_active
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            *active = Some(DefaultControl {
                id,
                page: page.clone(),
                active: ticket.active.clone(),
            });
            bank.take().ok_or("workspace.denied")?
        };
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
            if selected.is_some_and(|selection| match selection {
                pulse_store::profile_default::DefaultProfileSelection::Automatic(shell) => {
                    !candidates
                        .iter()
                        .any(|candidate| as_shell(candidate.kind()) == shell)
                }
                pulse_store::profile_default::DefaultProfileSelection::Custom(id) => {
                    !ticket.custom_offered.contains(&id)
                }
            }) {
                return Err("workspace.denied");
            }
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let deadline = ticket.deadline;
            let active = ticket.active.clone();
            store
                .save_default_profile_selection(ticket.snapshot.clone(), selected, move || {
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
            drop(ticket);
            Ok(())
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
}

#[cfg(test)]
#[path = "product_profiles_default_tests.rs"]
mod tests;
