//! Original-page, original-epoch private-key ciphertext ingress. No plaintext read or SSH use authority.
use super::*;
use pulse_core::secret_ref::SecretKind;
use pulse_store::{
    key_identity_ciphertext::KeyIdentityCiphertextEdit,
    key_identity_vault_source::KeyIdentityVaultSource, vault_crypto::SecretBytes,
    vault_lifecycle::AvailableKeyEpoch, vault_record::CredentialRecord,
};
/// Single-use saved private-key write intent; holding it never grants peer or credential-use permission.
/// Trusted upper adapter must validate the supported key format; no parser or KDF runs in SQL.
/// Caller must bound UI ticket expiry and independently authorize its original target and page.
/// ```compile_fail
/// fn log(intent: pulse_app::vault_owner::PrivateKeyIdentityWriteIntent) { println!("{intent:?}"); }
/// ```
/// ```compile_fail
/// fn send(intent: pulse_app::vault_owner::PrivateKeyIdentityWriteIntent) { let _ = serde_json::to_string(&intent); }
/// ```
pub struct PrivateKeyIdentityWriteIntent {
    operation: PageVaultOperation,
    source: KeyIdentityVaultSource,
    identity: Uuid,
    epoch: AvailableKeyEpoch,
}
impl PageVaultOperation {
    /// Freeze the saved references and this available epoch before any asynchronous key parsing.
    /// No plaintext is read, and a later unlock can never upgrade this original intent.
    pub fn prepare_private_key_identity_write(
        self,
        source: KeyIdentityVaultSource,
        identity: Uuid,
    ) -> Result<PrivateKeyIdentityWriteIntent> {
        self.current()?;
        let row = source
            .identities()
            .records()
            .find(|row| row.id() == identity)
            .ok_or(VaultOwnerError::Denied)?;
        let key = row.identity().private_key();
        if key.kind() != SecretKind::SshPrivateKey
            || Some(key.vault_id()) != source.vault().vault_id()
            || row.identity().passphrase().is_some_and(|pass| {
                pass.kind() != SecretKind::SshKeyPassphrase
                    || pass.vault_id() != key.vault_id()
                    || pass.id() == key.id()
            })
        {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .bind(source.vault().clone())?
            .ok_or(VaultOwnerError::Denied)?;
        let epoch = scope
            .available_epoch()
            .map_err(|_| VaultOwnerError::Denied)?;
        Ok(PrivateKeyIdentityWriteIntent {
            operation: self,
            source,
            identity,
            epoch,
        })
    }
}
impl PrivateKeyIdentityWriteIntent {
    pub(crate) fn cancellation(&self) -> PageVaultCancellation {
        self.operation.cancellation()
    }
    pub(crate) fn bound_to(
        &self,
        controller: &VaultControllerSource,
        page: &Arc<PageLaunchScope>,
    ) -> bool {
        if !controller.matches_cancel(&self.operation.cancellation())
            || !Arc::ptr_eq(&self.operation.token.page, page)
        {
            return false;
        }
        let Ok(owner) = self.operation.current() else {
            return false;
        };
        let Ok(state) = owner.guard() else {
            return false;
        };
        state
            .scope
            .as_ref()
            .is_some_and(|scope| self.epoch.is_current_for(scope).unwrap_or(false))
    }
    /// Seal both roles and drop owned plaintext before one native SQLite transaction.
    /// Failure/uncertain response is never retry permission; cancellation retains actual-operation credit.
    /// Optional passphrase must exactly match the original saved reference; raw paths/roles are not accepted.
    pub async fn write(
        self,
        store: &StateStore,
        key: SecretBytes,
        pass: Option<SecretBytes>,
        dispatch_gate: Authorize,
    ) -> Result<VaultSnapshot> {
        struct Waiter {
            cancel: PageVaultCancellation,
            complete: bool,
        }
        impl Drop for Waiter {
            fn drop(&mut self) {
                if !self.complete {
                    self.cancel.cancel();
                }
            }
        }
        let mut waiter = Waiter {
            cancel: self.operation.cancellation(),
            complete: false,
        };
        self.operation.current()?;
        let row = self
            .source
            .identities()
            .records()
            .find(|row| row.id() == self.identity)
            .ok_or(VaultOwnerError::Denied)?;
        let reference = row.identity().private_key();
        let pass_reference = row.identity().passphrase();
        if !(1..=65_536).contains(&key.expose().len())
            || pass_reference.is_some() != pass.is_some()
            || pass
                .as_ref()
                .is_some_and(|plain| !(1..=4096).contains(&plain.expose().len()))
            || !dispatch_gate()
        {
            return Err(VaultOwnerError::Denied);
        }
        let scope = self
            .operation
            .bind(self.source.vault().clone())?
            .ok_or(VaultOwnerError::Denied)?;
        if !self.epoch.is_current_for(&scope).unwrap_or(false) {
            return Err(VaultOwnerError::Denied);
        }
        let key_record = CredentialRecord::seal_in_epoch(
            &scope,
            &self.epoch,
            reference.id(),
            SecretKind::SshPrivateKey.as_str(),
            key.expose(),
        )
        .map_err(|_| VaultOwnerError::Denied)?;
        let pass_record = match (pass_reference, pass.as_ref()) {
            (Some(reference), Some(plain)) => Some(
                CredentialRecord::seal_in_epoch(
                    &scope,
                    &self.epoch,
                    reference.id(),
                    SecretKind::SshKeyPassphrase.as_str(),
                    plain.expose(),
                )
                .map_err(|_| VaultOwnerError::Denied)?,
            ),
            (None, None) => None,
            _ => return Err(VaultOwnerError::Denied),
        };
        drop(key);
        drop(pass);
        let edit =
            KeyIdentityCiphertextEdit::new(self.source, self.identity, key_record, pass_record)
                .map_err(|_| VaultOwnerError::Denied)?;
        let original = self.operation.authorization();
        let native_gate = dispatch_gate.clone();
        let authorize: Authorize = Arc::new(move || original() && native_gate());
        let saved = store
            .edit_key_identity_ciphertext(edit, scope.clone(), self.epoch.clone(), authorize)
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        self.operation.current()?;
        if !self.epoch.is_current_for(&scope).unwrap_or(false) || !dispatch_gate() {
            return Err(VaultOwnerError::Denied);
        }
        waiter.complete = true;
        Ok(saved)
    }
}
