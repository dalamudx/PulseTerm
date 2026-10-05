//! Validate KEK->DEK authentication in memory. No disk format or unlock service.
use crate::vault_crypto::{CryptoError, SealedSecret, SecretContext, VaultCipher};
use crate::vault_kdf::{
    KdfError, KdfJob, admit_validation_job, derive_admitted, derive_validation_kek, fresh_salt,
    validate_password,
};
use uuid::Uuid;
use zeroize::Zeroizing;
const KIND: &str = "wrapped_dek";
const SEALED_KEY_LENGTH: usize = 32 + 16;
/// Cloneable encrypted metadata, intentionally without a persistence serializer.
/// Cost/profile selection is fixed by the validation API, never this metadata.
#[derive(Clone)]
pub struct ValidationWrappedDek {
    vault_id: Uuid,
    salt: [u8; 16],
    sealed: SealedSecret,
}
/// An authenticated, identity-bound DEK, not a KEK or random key.
/// Fields are private; only successful tag verification constructs this result.
/// No Clone/Debug/serde. This remains an internal Rust adapter boundary.
/// ```compile_fail
/// fn expose(key: pulse_store::vault_wrap::AuthenticatedDek) { println!("{key:?}"); }
/// ```
pub struct AuthenticatedDek {
    vault_id: Uuid,
    cipher: VaultCipher,
}
impl AuthenticatedDek {
    /// Trusted backend import identity, never an authorization grant.
    pub(crate) fn vault_id(&self) -> Uuid {
        self.vault_id
    }
    /// Consume for a trusted adapter; app publication still needs its epoch gate.
    pub fn into_cipher(self) -> VaultCipher {
        self.cipher
    }
    #[cfg(test)]
    pub(crate) fn fixture(id: Uuid) -> Result<Self, CryptoError> {
        Ok(Self {
            vault_id: id,
            cipher: VaultCipher::generate()?,
        })
    }
}
/// Static errors never contain a password, key, ciphertext or source metadata.
#[derive(Debug, thiserror::Error)]
pub enum WrapError {
    /// A mismatching expected identity or wrong encrypted-key length.
    #[error("vault.invalid_validation_envelope")]
    Envelope,
    /// Entropy failed; there is no fallback DEK.
    #[error("vault.wrap_entropy")]
    Entropy,
    /// Fixed-profile KDF admission or calculation failed.
    #[error(transparent)]
    Kdf(#[from] KdfError),
    /// Auth failure intentionally does not distinguish password from corruption.
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}
/// New validation vault only. Never use this to replace an existing missing key.
/// The authorized caller owns password zeroization and must not run on the GUI.
/// Returns encrypted metadata and an opaque cipher, never the clear DEK.
pub fn create_validation_vault(
    password: &[u8],
    vault_id: Uuid,
) -> Result<(ValidationWrappedDek, VaultCipher), WrapError> {
    let context = SecretContext::new(vault_id, KIND)?;
    let salt = fresh_salt()?;
    let kek = derive_validation_kek(password, &salt)?;
    let mut material = Zeroizing::new([0; 32]);
    getrandom::fill(&mut material[..]).map_err(|_| WrapError::Entropy)?;
    let sealed = kek.seal(&context, &material[..])?;
    let cipher = VaultCipher::from_key_material(material)?;
    Ok((
        ValidationWrappedDek {
            vault_id,
            salt,
            sealed,
        },
        cipher,
    ))
}
/// Authenticate against a trusted expected vault identity before importing the DEK.
/// KDF success alone is not accepted. This does not publish an unlocked app state:
/// authorization, unlock epoch and late-result confirmation belong to the service.
pub fn recover_validation_dek(
    password: &[u8],
    expected_vault_id: Uuid,
    envelope: &ValidationWrappedDek,
) -> Result<AuthenticatedDek, WrapError> {
    envelope.validate_for(expected_vault_id)?;
    validate_password(password)?;
    let job = admit_validation_job()?;
    recover_admitted(password, expected_vault_id, envelope, &job)
}
impl ValidationWrappedDek {
    #[cfg(test)]
    pub(crate) fn malformed_fixture(id: Uuid) -> Self {
        Self {
            vault_id: id,
            salt: [0; 16],
            sealed: SealedSecret {
                nonce: [0; 24],
                ciphertext: vec![0; SEALED_KEY_LENGTH],
            },
        }
    }
    pub(crate) fn validate_for(&self, expected: Uuid) -> Result<(), WrapError> {
        if self.vault_id != expected
            || self.sealed.ciphertext.len() != SEALED_KEY_LENGTH
            || expected.is_nil()
        {
            return Err(WrapError::Envelope);
        }
        Ok(())
    }
}
pub(crate) fn recover_admitted(
    password: &[u8],
    expected_vault_id: Uuid,
    envelope: &ValidationWrappedDek,
    job: &KdfJob,
) -> Result<AuthenticatedDek, WrapError> {
    envelope.validate_for(expected_vault_id)?;
    let context = SecretContext::new(expected_vault_id, KIND)?;
    let kek = derive_admitted(password, &envelope.salt, job)?;
    let plaintext = kek.open(&context, &envelope.sealed)?;
    if plaintext.expose().len() != 32 {
        return Err(WrapError::Envelope);
    }
    let mut material = Zeroizing::new([0; 32]);
    material.copy_from_slice(plaintext.expose());
    Ok(AuthenticatedDek {
        vault_id: expected_vault_id,
        cipher: VaultCipher::from_key_material(material)?,
    })
}
#[cfg(test)]
#[path = "vault_wrap_tests.rs"]
mod tests;
