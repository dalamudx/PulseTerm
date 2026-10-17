//! Bounded ciphertext record. Parsing never authenticates bytes or enables a key.
use crate::vault_crypto::{
    CryptoError, SECRET_BYTES_LIMIT, SealedSecret, SecretBytes, SecretContext, VaultCipher,
};
use uuid::Uuid;
/// Immutable encrypted bytes plus caller-bound metadata; no Debug/serde/clear-key fields.
/// ```compile_fail
/// fn log(record: pulse_store::vault_record::CredentialRecord) { println!("{record:?}"); }
/// ```
/// ```compile_fail
/// fn send(record: pulse_store::vault_record::CredentialRecord) { let _ = serde_json::to_vec(&record); }
/// ```
#[derive(Clone)]
pub struct CredentialRecord {
    vault_id: Uuid,
    context: SecretContext,
    sealed: SealedSecret,
}
/// Static shape/identity errors, never secret bytes or untrusted diagnostic text.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// Unsupported shape/version/profile, budget overflow or expected identity mismatch.
    #[error("vault.invalid_ciphertext_record")]
    Invalid,
    /// Fixed cryptographic failure; no plaintext or input is included in the error.
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}
/// Maximum fixed header, 32-byte kind and bounded AEAD ciphertext/tag.
pub const RECORD_BYTES_LIMIT: usize = 99 + SECRET_BYTES_LIMIT + 16;
impl CredentialRecord {
    /// Native ciphertext construction under an exact available epoch, without exporting the DEK.
    /// Caller must independently authorize the saved reference and edit; borrowed plaintext stays owned by caller.
    pub fn seal_in_epoch(
        scope: &crate::vault_lifecycle::VaultKeyLifecycle,
        epoch: &crate::vault_lifecycle::AvailableKeyEpoch,
        id: Uuid,
        kind: &str,
        plaintext: &[u8],
    ) -> Result<Self, crate::vault_lifecycle::KeyUseError> {
        let vault_id = scope.expected_id()?;
        let context = SecretContext::bound_to_vault(vault_id, id, kind)?;
        let sealed = scope.seal_in_epoch(epoch, &context, plaintext)?;
        Ok(Self {
            vault_id,
            context,
            sealed,
        })
    }
    /// Rust-only trusted adapter authentication; never expose as plaintext IPC.
    /// Exact stored identity/reference and verified peer/target-use permission remain independent requirements.
    pub fn open_for_adapter_in_epoch(
        &self,
        scope: &crate::vault_lifecycle::VaultKeyLifecycle,
        epoch: &crate::vault_lifecycle::AvailableKeyEpoch,
        expected_id: Uuid,
        expected_kind: &str,
    ) -> Result<SecretBytes, crate::vault_lifecycle::KeyUseError> {
        let vault_id = scope.expected_id()?;
        let expected = SecretContext::bound_to_vault(vault_id, expected_id, expected_kind)?;
        if self.vault_id != vault_id || self.context != expected {
            return Err(CryptoError::Context.into());
        }
        scope.open_for_adapter_in_epoch(epoch, &expected, &self.sealed)
    }

    /// Trusted backend encryption; borrowed plaintext ownership/zeroization belongs to caller.
    pub fn seal(
        vault_id: Uuid,
        id: Uuid,
        kind: &str,
        cipher: &VaultCipher,
        plaintext: &[u8],
    ) -> Result<Self, RecordError> {
        if vault_id.is_nil() {
            return Err(RecordError::Invalid);
        }
        let context = SecretContext::bound_to_vault(vault_id, id, kind)?;
        let sealed = cipher.seal(&context, plaintext)?;
        Ok(Self {
            vault_id,
            context,
            sealed,
        })
    }
    /// Metadata is not permission to resolve/use a credential.
    pub fn id(&self) -> Uuid {
        self.context.id()
    }
    pub(crate) fn kind(&self) -> &str {
        self.context.kind()
    }
    pub(crate) fn storage_parts(&self) -> (&[u8; 24], &[u8]) {
        (&self.sealed.nonce, &self.sealed.ciphertext)
    }
    pub(crate) fn from_storage(
        vault_id: Uuid,
        id: Uuid,
        kind: &str,
        nonce: [u8; 24],
        ciphertext: Vec<u8>,
    ) -> Result<Self, RecordError> {
        if !(16..=SECRET_BYTES_LIMIT + 16).contains(&ciphertext.len()) {
            return Err(RecordError::Invalid);
        }
        let context = SecretContext::bound_to_vault(vault_id, id, kind)?;
        Ok(Self {
            vault_id,
            context,
            sealed: SealedSecret { nonce, ciphertext },
        })
    }
    /// Fixed version/profile plus vault/credential/kind, nonce and exact ciphertext length.
    pub fn encode_ciphertext(&self) -> Result<Vec<u8>, RecordError> {
        let kind = self.context.kind().as_bytes();
        let len = self.sealed.ciphertext.len();
        if self.vault_id.is_nil() || !(16..=SECRET_BYTES_LIMIT + 16).contains(&len) {
            return Err(RecordError::Invalid);
        }
        let mut bytes = Vec::with_capacity(67 + kind.len() + len);
        bytes.extend_from_slice(b"PVCR\x01\x01");
        bytes.extend_from_slice(self.vault_id.as_bytes());
        bytes.extend_from_slice(self.id().as_bytes());
        bytes.push(kind.len() as u8);
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(&self.sealed.nonce);
        bytes.extend_from_slice(&(len as u32).to_be_bytes());
        bytes.extend_from_slice(&self.sealed.ciphertext);
        Ok(bytes)
    }
    /// Bound all lengths and canonical kind before ciphertext allocation. No crypto work here.
    pub fn decode_ciphertext(bytes: &[u8], expected_vault: Uuid) -> Result<Self, RecordError> {
        if !(84..=RECORD_BYTES_LIMIT).contains(&bytes.len())
            || &bytes[..6] != b"PVCR\x01\x01"
            || expected_vault.is_nil()
        {
            return Err(RecordError::Invalid);
        }
        let vault_id = Uuid::from_slice(&bytes[6..22]).map_err(|_| RecordError::Invalid)?;
        if vault_id != expected_vault {
            return Err(RecordError::Invalid);
        }
        let id = Uuid::from_slice(&bytes[22..38]).map_err(|_| RecordError::Invalid)?;
        let n = bytes[38] as usize;
        if !(1..=32).contains(&n) || bytes.len() < 67 + n + 16 {
            return Err(RecordError::Invalid);
        }
        let kind = std::str::from_utf8(&bytes[39..39 + n]).map_err(|_| RecordError::Invalid)?;
        let context =
            SecretContext::bound_to_vault(vault_id, id, kind).map_err(|_| RecordError::Invalid)?;
        let p = 39 + n;
        let nonce = bytes[p..p + 24]
            .try_into()
            .map_err(|_| RecordError::Invalid)?;
        let raw: [u8; 4] = bytes[p + 24..p + 28]
            .try_into()
            .map_err(|_| RecordError::Invalid)?;
        let len = u32::from_be_bytes(raw) as usize;
        if !(16..=SECRET_BYTES_LIMIT + 16).contains(&len) || bytes.len() != p + 28 + len {
            return Err(RecordError::Invalid);
        }
        Ok(Self {
            vault_id,
            context,
            sealed: SealedSecret {
                nonce,
                ciphertext: bytes[p + 28..].to_vec(),
            },
        })
    }
    /// Authenticate against trusted expected metadata, never self-supplied row context.
    /// Authorization/epoch and protocol-use approval remain caller responsibilities.
    pub fn open(
        &self,
        expected_vault: Uuid,
        expected_id: Uuid,
        expected_kind: &str,
        cipher: &VaultCipher,
    ) -> Result<SecretBytes, RecordError> {
        let expected = SecretContext::bound_to_vault(expected_vault, expected_id, expected_kind)?;
        if expected_vault.is_nil() || self.vault_id != expected_vault || self.context != expected {
            return Err(RecordError::Invalid);
        }
        Ok(cipher.open(&expected, &self.sealed)?)
    }
}
