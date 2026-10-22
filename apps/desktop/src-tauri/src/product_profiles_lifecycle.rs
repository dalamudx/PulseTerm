use super::*;
use pulse_store::{
    profile_default::{DefaultProfileSnapshot, ProfileLifecycleEdit},
    profiles::AutomaticShell,
};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(super) struct LifecycleTicket {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    store: StateStore,
    snapshot: DefaultProfileSnapshot,
    edit: ProfileLifecycleEdit,
    deadline: Instant,
    active: Arc<AtomicBool>,
}
impl Drop for LifecycleTicket {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
pub(super) struct LifecycleControl {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    active: Arc<AtomicBool>,
}
impl Drop for LifecycleControl {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LifecycleOffer {
    ticket: Uuid,
    shell: AutomaticShell,
    action: &'static str,
    is_default: bool,
}
fn shell_identity(kind: pulse_pty::profiles::ShellKind) -> AutomaticShell {
    use pulse_pty::profiles::ShellKind;
    match kind {
        ShellKind::Bash => AutomaticShell::Bash,
        ShellKind::Zsh => AutomaticShell::Zsh,
        ShellKind::Fish => AutomaticShell::Fish,
        ShellKind::Sh => AutomaticShell::Sh,
        ShellKind::Pwsh => AutomaticShell::Pwsh,
        ShellKind::PowerShell => AutomaticShell::PowerShell,
        ShellKind::Cmd => AutomaticShell::Cmd,
        ShellKind::Wsl => AutomaticShell::Wsl,
    }
}
impl ProfileReader {
    pub(in super::super) async fn lifecycle_deleted(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        source: (PathBuf, OsString, Option<PathBuf>),
    ) -> Result<Vec<AutomaticShell>> {
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
                .load_default_profile_retaining(credit)
                .await
                .map_err(|_| "workspace.unavailable")?;
            reader.check(&registry, &page)?;
            Ok(snapshot.preferences().deleted().collect())
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    pub(in super::super) async fn lifecycle_prepare(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        source: (PathBuf, OsString, Option<PathBuf>),
        edit: ProfileLifecycleEdit,
    ) -> Result<LifecycleOffer> {
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
                .load_default_profile_retaining(credit.clone())
                .await
                .map_err(|_| "workspace.unavailable")?;
            let (shell, action) = match edit {
                ProfileLifecycleEdit::Delete(shell)
                    if !snapshot.preferences().deleted().any(|item| item == shell)
                        && candidates
                            .iter()
                            .any(|candidate| shell_identity(candidate.kind()) == shell) =>
                {
                    (shell, "delete")
                }
                ProfileLifecycleEdit::Restore(shell)
                    if snapshot.preferences().deleted().any(|item| item == shell) =>
                {
                    (shell, "restore")
                }
                _ => return Err("workspace.denied"),
            };
            reader.check(&registry, &page)?;
            let id = Uuid::new_v4();
            let is_default = snapshot.shell() == Some(shell);
            let ticket = LifecycleTicket {
                id,
                page: page.clone(),
                store,
                snapshot,
                edit,
                deadline: Instant::now() + Duration::from_secs(60),
                active: Arc::new(AtomicBool::new(true)),
            };
            let mut bank = reader
                .lifecycle_ticket
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            reader.check(&registry, &page)?;
            *bank = Some(ticket);
            drop(credit);
            Ok(LifecycleOffer {
                ticket: id,
                shell,
                action,
                is_default,
            })
        })
        .await
        .map_err(|_| "workspace.unavailable")?
    }
    pub(in super::super) fn lifecycle_cancel(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
    ) -> Result<()> {
        self.check(registry, page)?;
        let mut bank = self
            .lifecycle_ticket
            .lock()
            .map_err(|_| "workspace.unavailable")?;
        if bank
            .as_ref()
            .is_some_and(|ticket| ticket.id == id && Arc::ptr_eq(&ticket.page, page))
        {
            bank.take();
        }
        let mut active = self
            .lifecycle_active
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
    pub(in super::super) async fn lifecycle_commit(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        id: Uuid,
    ) -> Result<()> {
        self.check(&registry, &page)?;
        let ticket = {
            let mut bank = self
                .lifecycle_ticket
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            let ticket = bank.as_ref().ok_or("workspace.denied")?;
            if ticket.id != id
                || !Arc::ptr_eq(&ticket.page, &page)
                || Instant::now() >= ticket.deadline
            {
                return Err("workspace.denied");
            }
            let mut active = self
                .lifecycle_active
                .lock()
                .map_err(|_| "workspace.unavailable")?;
            *active = Some(LifecycleControl {
                id,
                page: page.clone(),
                active: ticket.active.clone(),
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
                .change_profile_lifecycle(ticket.snapshot.clone(), ticket.edit, move || {
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

#[cfg(all(test, unix))]
#[path = "product_profiles_lifecycle_tests.rs"]
mod tests;
