//! Supervised saved-identity password producer/submit prototype; no frontend command until acceptance.
use super::*;
use pulse_app::{
    vault_owner::{PageVaultCancellation, PageVaultOwner},
    vault_password_handoff::{PasswordWriteBank, PasswordWritePermit},
};
use pulse_store::vault_crypto::SecretBytes;
use uuid::Uuid;
#[cfg(test)]
#[path = "product_profiles_password_tests.rs"]
mod tests;
struct WaitCancel {
    cancel: PageVaultCancellation,
    armed: bool,
}
impl Drop for WaitCancel {
    fn drop(&mut self) {
        if self.armed {
            self.cancel.cancel();
        }
    }
}
impl ProfileReader {
    pub(in super::super) async fn password_prepare(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        owner: PageVaultOwner,
        bank: PasswordWriteBank,
        identity: Uuid,
    ) -> Result<Uuid> {
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let prepare_credit = credit.clone();
        let reader = self.clone();
        let gate_reader = reader.clone();
        let gate_registry = registry.clone();
        let gate_page = page.clone();
        let expected = directory.clone();
        let gate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            let _credit = &credit;
            gate_reader.check(&gate_registry, &gate_page).is_ok()
                && gate_reader
                    .store
                    .get()
                    .is_none_or(|s| s.directory == expected)
        });
        let operation = owner
            .begin_authorized(page.clone(), gate.clone())
            .map_err(|_| "workspace.vault_denied")?;
        let mut cancel = WaitCancel {
            cancel: operation.cancellation(),
            armed: true,
        };
        let work = tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, prepare_credit)
                .await?;
            let source = store
                .load_identity_vault_source(gate)
                .await
                .map_err(|_| "workspace.identities_failed")?;
            reader.check(&registry, &page)?;
            let intent = operation
                .prepare_password_identity_write(source, identity)
                .map_err(|_| "workspace.vault_denied")?;
            bank.publish(page, intent)
                .map_err(|_| "workspace.vault_denied")
        });
        let id = tokio::time::timeout(std::time::Duration::from_secs(5), work)
            .await
            .map_err(|_| "workspace.vault_timeout")?
            .map_err(|_| "workspace.vault_failed")??;
        cancel.armed = false;
        Ok(id)
    }
    pub(in super::super) async fn password_submit(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        permit: PasswordWritePermit,
        plain: SecretBytes,
    ) -> Result<()> {
        self.check(&registry, &page)?;
        // Preparation retains actual I/O quota through the intent. Reserving again would self-deadlock.
        let store = self
            .store
            .get()
            .filter(|s| s.directory == directory)
            .ok_or("workspace.vault_denied")?
            .store
            .clone();
        let mut cancel = WaitCancel {
            cancel: permit.cancellation(),
            armed: true,
        };
        let reader = self.clone();
        let work = tokio::spawn(async move {
            reader.check(&registry, &page)?;
            permit
                .write(&store, plain)
                .await
                .map_err(|_| "workspace.vault_denied")?;
            reader.check(&registry, &page)?;
            Ok::<(), &'static str>(())
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), work)
            .await
            .map_err(|_| "workspace.vault_timeout")?
            .map_err(|_| "workspace.vault_failed")??;
        cancel.armed = false;
        Ok(())
    }
}
