//! One current-page metadata edit capture, no secret, trust or connection authority.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use pulse_app::product_document::ProductDocuments;
    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
    #[tokio::test]
    async fn current_single_use_replacement_foreign_scope_navigation_and_expiry() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = pulse_store::StateStore::open(dir.join("state.db")).await?;
        let snapshot = store.load_ssh_hosts().await?;
        let registry = SessionRegistry::new();
        let docs = ProductDocuments::new(registry.clone());
        let owner = registry.open_window()?;
        docs.install(owner)?;
        docs.started(owner)?;
        let nonce = docs.finished(owner)?;
        docs.confirm(owner, nonce)?;
        let page = docs.current(owner, nonce)?;
        let bank = HostCaptures::new();
        let token = bank.publish(&registry, page.clone(), snapshot.clone())?;
        assert!(bank.take(&registry, &page, Uuid::new_v4()).is_err());
        assert_eq!(bank.take(&registry, &page, token)?.revision(), 0);
        assert!(bank.take(&registry, &page, token).is_err());
        let old = bank.publish(&registry, page.clone(), snapshot.clone())?;
        let new = bank.publish(&registry, page.clone(), snapshot.clone())?;
        assert!(bank.take(&registry, &page, old).is_err());
        let foreign = SessionRegistry::new();
        assert!(bank.take(&foreign, &page, new).is_err());
        assert_eq!(bank.take(&registry, &page, new)?.revision(), 0);
        let retired = bank.publish(&registry, page.clone(), snapshot.clone())?;
        docs.started(owner)?;
        bank.retire();
        assert!(bank.take(&registry, &page, retired).is_err());
        assert!(bank.publish(&registry, page, snapshot.clone()).is_err());
        let nonce = docs.finished(owner)?;
        docs.confirm(owner, nonce)?;
        let page = docs.current(owner, nonce)?;
        let token = bank.publish(&registry, page.clone(), snapshot)?;
        bank.expire_for_test()?;
        assert!(bank.take(&registry, &page, token).is_err());
        assert!(store.load_ssh_hosts().await?.records().is_empty());
        drop(store);
        worker.join().await?;
        registry.shutdown()?;
        foreign.shutdown()?;
        Ok(())
    }
}

use super::super::metadata_captures::MetadataCaptures;
use super::*;
use pulse_app::registry::PageLaunchScope;
use pulse_store::ssh_hosts::HostSnapshot;
pub(in super::super) struct HostCaptures {
    inner: MetadataCaptures<HostSnapshot>,
}
impl HostCaptures {
    pub(in super::super) fn new() -> Self {
        Self {
            inner: MetadataCaptures::new("workspace.hosts_failed"),
        }
    }
    pub(in super::super) fn retire(&self) {
        self.inner.retire();
    }
    pub(super) fn publish(
        &self,
        registry: &SessionRegistry,
        page: Arc<PageLaunchScope>,
        snapshot: HostSnapshot,
    ) -> Result<Uuid> {
        self.inner.publish(registry, page, snapshot)
    }
    pub(super) fn take(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        token: Uuid,
    ) -> Result<HostSnapshot> {
        self.inner.take(registry, page, token)
    }
    #[cfg(test)]
    fn expire_for_test(&self) -> Result<()> {
        self.inner.expire_for_test()
    }
}
