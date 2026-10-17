//! Current-page supervised private-key preparation and validated ciphertext submission.
#[cfg(all(test, unix))]
#[path = "product_profiles_key_write_tests.rs"]
mod tests;
use super::*;
use pulse_app::{
    vault_key_write_handoff::{PrivateKeyWriteBank, PrivateKeyWritePermit},
    vault_owner::{PageVaultCancellation, PageVaultOwner},
};
use pulse_store::vault_crypto::SecretBytes;
use uuid::Uuid;
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
    pub(in super::super) async fn key_prepare(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        owner: PageVaultOwner,
        bank: PrivateKeyWriteBank,
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
                .load_key_identity_vault_source(gate)
                .await
                .map_err(|_| "workspace.identities_failed")?;
            reader.check(&registry, &page)?;
            let intent = operation
                .prepare_private_key_identity_write(source, identity)
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
    pub(in super::super) async fn key_submit(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        permit: PrivateKeyWritePermit,
        key: SecretBytes,
        pass: Option<SecretBytes>,
    ) -> Result<()> {
        self.check(&registry, &page)?;
        // The original intent owns the I/O reservation; another reservation would self-deadlock.
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
            // The blocking validator owns the original ticket and native credit through actual exit.
            let ValidatedIngress { permit, key, pass } = validate_ingress(permit, key, pass)
                .await
                .map_err(|_| "workspace.private_key_denied")?;
            reader.check(&registry, &page)?;
            permit
                .write(&store, key, pass)
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

struct ValidatedIngress {
    permit: PrivateKeyWritePermit,
    key: SecretBytes,
    pass: Option<SecretBytes>,
}
type ValidationResult<T> = std::result::Result<T, pulse_app::vault_owner::VaultOwnerError>;
fn decode_ingress(key: &SecretBytes, pass: Option<&SecretBytes>) -> ValidationResult<()> {
    let decoded =
        crate::ssh_private_key_decode::decode_bounded(key.expose(), pass.map(SecretBytes::expose))?;
    drop(decoded);
    Ok(())
}
fn spawn_ingress_validation(
    permit: PrivateKeyWritePermit,
    key: SecretBytes,
    pass: Option<SecretBytes>,
    cpu: tokio::sync::OwnedSemaphorePermit,
    decode: impl FnOnce(&SecretBytes, Option<&SecretBytes>) -> ValidationResult<()> + Send + 'static,
) -> tokio::task::JoinHandle<ValidationResult<ValidatedIngress>> {
    use pulse_app::vault_owner::VaultOwnerError;
    tokio::task::spawn_blocking(move || {
        let _cpu = cpu;
        permit
            .check_admission()
            .map_err(|_| VaultOwnerError::Denied)?;
        // Exact input and original ticket remain owned through actual parsing exit.
        decode(&key, pass.as_ref())?;
        permit
            .check_admission()
            .map_err(|_| VaultOwnerError::Denied)?;
        Ok(ValidatedIngress { permit, key, pass })
    })
}
async fn validate_ingress(
    permit: PrivateKeyWritePermit,
    key: SecretBytes,
    pass: Option<SecretBytes>,
) -> ValidationResult<ValidatedIngress> {
    use pulse_app::vault_owner::VaultOwnerError;
    permit
        .check_admission()
        .map_err(|_| VaultOwnerError::Denied)?;
    let cpu = crate::ssh_private_key_decode::DECODE_BUDGET
        .clone()
        .try_acquire_owned()
        .map_err(|_| VaultOwnerError::Denied)?;
    spawn_ingress_validation(permit, key, pass, cpu, decode_ingress)
        .await
        .map_err(|_| VaultOwnerError::Failed)?
}
