//! Staged original-click route reader; the protocol driver and GUI execution remain pending.
use super::*;
use pulse_app::vault_owner::{RoutedSessionGesture, SavedSessionGesture};
use pulse_store::ssh_route_source::SshRouteIdentity;
use uuid::Uuid;
#[cfg(test)]
#[path = "product_profiles_route_tests.rs"]
mod tests;

impl ProfileReader {
    // Explicitly staged backend API; no Tauri RPC or dial path is registered before driver acceptance.
    #[allow(
        dead_code,
        reason = "The production routed-session driver is not yet connected"
    )]
    pub(in super::super::super) async fn ssh_route(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        gesture: SavedSessionGesture,
        destination: Uuid,
        identity: SshRouteIdentity,
    ) -> Result<(StateStore, RoutedSessionGesture)> {
        self.check(&registry, &page)?;
        if !gesture.permits_route_read(&page) {
            return Err("workspace.denied");
        }
        let credit = self.reserve()?;
        let reader = self.clone();
        tokio::spawn(async move {
            reader.check(&registry, &page)?;
            if !gesture.permits_route_read(&page) {
                return Err("workspace.denied");
            }
            let store = reader
                .prepare_store(&registry, &page, directory, credit.clone())
                .await?;
            let gate_reader = reader.clone();
            let gate_registry = registry.clone();
            let gate_page = page.clone();
            let gate = Arc::new(move || {
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let route = gesture
                .read_route(&page, &store, destination, identity, gate)
                .await
                .map_err(|_| "workspace.ssh_route_failed")?;
            reader.check(&registry, &page)?;
            Ok((store, route))
        })
        .await
        .map_err(|_| "workspace.ssh_route_failed")?
    }
}
