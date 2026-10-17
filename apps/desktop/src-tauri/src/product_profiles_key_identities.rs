//! Private-key metadata uses the same root private store and native I/O credit as password metadata.
#[cfg(all(test, unix))]
#[path = "product_profiles_key_identity_tests.rs"]
mod tests;
use super::*;
use pulse_store::ssh_key_identities::{KeyIdentityEdit, KeyIdentitySnapshot};
fn key_identity_error(error: pulse_store::StoreError) -> &'static str {
    match error {
        pulse_store::StoreError::IdentityAuthorization => "workspace.denied",
        pulse_store::StoreError::IdentityConflict => "workspace.key_identities_conflict",
        pulse_store::StoreError::IdentityExhausted => "workspace.key_identities_limit",
        pulse_store::StoreError::Busy => "workspace.busy",
        _ => "workspace.key_identities_failed",
    }
}
impl ProfileReader {
    pub(in super::super) async fn key_identities(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
    ) -> Result<KeyIdentitySnapshot> {
        self.check(&registry, &page)?;
        let credit = self.reserve_list(&registry, &page).await?;
        let reader = self.clone();
        tokio::spawn(async move {
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
            let snapshot = store
                .load_ssh_key_identities(gate)
                .await
                .map_err(key_identity_error)?;
            reader.check(&registry, &page)?;
            Ok(snapshot)
        })
        .await
        .map_err(|_| "workspace.key_identities_failed")?
    }
    pub(in super::super) async fn edit_key_identity(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        expected: KeyIdentitySnapshot,
        change: KeyIdentityEdit,
    ) -> Result<KeyIdentitySnapshot> {
        self.check(&registry, &page)?;
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
                let _retain_until_sql_exit = &credit;
                gate_reader.check(&gate_registry, &gate_page).is_ok()
            });
            let snapshot = store
                .edit_ssh_key_identities(expected, change, gate)
                .await
                .map_err(key_identity_error)?;
            reader.check(&registry, &page)?;
            Ok(snapshot)
        })
        .await
        .map_err(|_| "workspace.key_identities_failed")?
    }
}
