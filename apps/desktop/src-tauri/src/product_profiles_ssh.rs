//! Upper shared-store public trust operations. No plaintext, Vault unlock or arbitrary paths.
#[path = "product_profiles_route.rs"]
mod route_service;
use super::*;
use pulse_ssh::managed_store::FirstPinApproval;
use pulse_store::ssh_target_source::{SshTargetSource, TemporarySshTarget};
impl ProfileReader {
    pub(in super::super) async fn temporary_ssh_target(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        endpoint: pulse_core::ssh_endpoint::SshEndpoint,
    ) -> Result<TemporarySshTarget> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit)
                .await?;
            // The persistent liveness callback deliberately holds no SQL reservation.
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || gate_reader.check(&gate_registry, &gate_page).is_ok());
            let target = store.temporary_ssh_target(endpoint, gate);
            reader.check(&registry, &page)?;
            Ok(target)
        })
        .await
        .map_err(|_| "workspace.quick_failed")?
    }

    pub(in super::super) async fn save_temporary_host(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        target: TemporarySshTarget,
        label: String,
    ) -> Result<uuid::Uuid> {
        self.check(&registry, &page)?;
        let original = target.endpoint();
        let endpoint = pulse_core::ssh_endpoint::SshEndpoint::new(
            &label,
            original.target().host(),
            original.target().port(),
            original.username(),
        )
        .map_err(|_| "workspace.quick_invalid")?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _actual_sql_credit = &credit;
                target.is_current() && gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let before = store
                .load_ssh_hosts_authorized(gate.clone())
                .await
                .map_err(|_| "workspace.quick_save_failed")?;
            let matches: Vec<_> = before
                .records()
                .iter()
                .filter(|row| {
                    row.endpoint().target() == endpoint.target()
                        && row.endpoint().username() == endpoint.username()
                })
                .collect();
            let id = match matches.as_slice() {
                [existing] => existing.id(),
                [] => {
                    let next = store
                        .edit_ssh_hosts(
                            before.clone(),
                            pulse_store::ssh_hosts::HostEdit::Create(endpoint),
                            gate.clone(),
                        )
                        .await
                        .map_err(|_| "workspace.quick_save_failed")?;
                    let mut created = next.records().iter().filter(|row| {
                        !before
                            .records()
                            .iter()
                            .any(|previous| previous.id() == row.id())
                    });
                    let id = created.next().ok_or("workspace.quick_save_failed")?.id();
                    if created.next().is_some() {
                        return Err("workspace.quick_save_failed");
                    }
                    id
                }
                _ => return Err("workspace.quick_ambiguous"),
            };
            // This path holds a native Root-registration proof, never a page-reported state.
            if gate() && store.record_recent_ssh_host(id).await.is_err() {
                eprintln!("workspace.recent_host_record_failed");
            }
            reader.check(&registry, &page)?;
            Ok(id)
        })
        .await
        .map_err(|_| "workspace.quick_save_failed")?
    }

    pub(in super::super) async fn ssh_trust_source(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        native: Arc<dyn Fn() -> bool + Send + Sync>,
        temporary: Option<TemporarySshTarget>,
    ) -> Result<(StateStore, SshTargetSource)> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            if !native() {
                return Err("workspace.denied");
            }
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _actual_sql_credit = &credit;
                native() && gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let source = match temporary {
                Some(target) => store.load_temporary_ssh_target_source(target, gate).await,
                None => store.load_ssh_target_source(gate).await,
            }
            .map_err(|_| "workspace.ssh_source_failed")?;
            reader.check(&registry, &page)?;
            Ok((store, source))
        })
        .await
        .map_err(|_| "workspace.ssh_source_failed")?
    }
    pub(in super::super) async fn ssh_key_source(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        native: Arc<dyn Fn() -> bool + Send + Sync>,
        temporary: Option<TemporarySshTarget>,
    ) -> Result<(
        StateStore,
        pulse_store::ssh_key_target_source::SshKeyTargetSource,
    )> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            if !native() {
                return Err("workspace.denied");
            }
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _actual_sql_credit = &credit;
                native() && gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let source = match temporary {
                Some(target) => {
                    store
                        .load_temporary_ssh_key_target_source(target, gate)
                        .await
                }
                None => store.load_ssh_key_target_source(gate).await,
            }
            .map_err(|_| "workspace.ssh_source_failed")?;
            reader.check(&registry, &page)?;
            Ok((store, source))
        })
        .await
        .map_err(|_| "workspace.ssh_source_failed")?
    }
    pub(in super::super) async fn ssh_pin_confirm(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        prompt: Box<FirstPinApproval>,
        native: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<u64> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            if !native() {
                return Err("workspace.denied");
            }
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _actual_sql_credit = &credit;
                native() && gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let revision = (*prompt)
                .confirm_with_guard(&store, true, gate)
                .await
                .map_err(|_| "workspace.ssh_approval_failed")?;
            reader.check(&registry, &page)?;
            Ok(revision)
        })
        .await
        .map_err(|_| "workspace.ssh_approval_failed")?
    }
}
