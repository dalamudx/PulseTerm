//! Agent metadata and discovery retain the shared native I/O credit until actual SQL/socket exit.
use super::super::agent_identities::KeyDto;
use super::*;
use pulse_store::ssh_agent_identities::{AgentIdentityEdit, AgentIdentitySnapshot};
fn error(e: pulse_store::StoreError) -> &'static str {
    match e {
        pulse_store::StoreError::IdentityAuthorization => "workspace.denied",
        pulse_store::StoreError::IdentityConflict => "workspace.agent_identities_conflict",
        pulse_store::StoreError::IdentityExhausted => "workspace.agent_identities_limit",
        pulse_store::StoreError::Busy => "workspace.busy",
        _ => "workspace.agent_identities_failed",
    }
}
impl ProfileReader {
    pub(in super::super) async fn agent_identities(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        discover: bool,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<(AgentIdentitySnapshot, Vec<KeyDto>)> {
        self.check(&registry, &page)?;
        if !authorize() {
            return Err("workspace.denied");
        }
        let credit = self.reserve_list(&registry, &page).await?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let r = reader.clone();
            let p = page.clone();
            let reg = registry.clone();
            let gate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
                let _retain = &credit;
                authorize() && r.check(&reg, &p).is_ok()
            });
            let snapshot = store
                .load_ssh_agent_identities(gate.clone())
                .await
                .map_err(error)?;
            #[cfg(feature = "workspace-ssh")]
            let keys = if discover {
                let conn = pulse_ssh::agent::AgentConnection::discover(gate.clone())
                    .await
                    .map_err(|_| "workspace.agent_unavailable")?;
                conn.keys()
                    .iter()
                    .map(|k| KeyDto {
                        algorithm: k.algorithm().to_owned(),
                        fingerprint: k.fingerprint().to_owned(),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            #[cfg(not(feature = "workspace-ssh"))]
            let keys = {
                if discover {
                    return Err("workspace.agent_unavailable");
                }
                Vec::new()
            };
            if !gate() {
                return Err("workspace.denied");
            }
            Ok((snapshot, keys))
        })
        .await
        .map_err(|_| "workspace.agent_identities_failed")?
    }
    pub(in super::super) async fn edit_agent_identity(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        expected: AgentIdentitySnapshot,
        change: AgentIdentityEdit,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<AgentIdentitySnapshot> {
        self.check(&registry, &page)?;
        if !authorize() {
            return Err("workspace.denied");
        }
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let r = reader.clone();
            let p = page.clone();
            let reg = registry.clone();
            let gate = Arc::new(move || {
                let _retain = &credit;
                authorize() && r.check(&reg, &p).is_ok()
            });
            let snapshot = store
                .edit_ssh_agent_identity(expected, change, gate.clone())
                .await
                .map_err(error)?;
            if !gate() {
                return Err("workspace.denied");
            }
            Ok(snapshot)
        })
        .await
        .map_err(|_| "workspace.agent_identities_failed")?
    }
}
