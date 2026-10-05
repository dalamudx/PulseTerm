//! Internal backend primitive. It neither grants credential access nor reads keyrings.
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};
/// Pre-crypto plaintext and ciphertext allocation budget (excluding the 16-byte tag).
pub const SECRET_BYTES_LIMIT: usize = 65_536;
/// Stable errors never include keys, plaintext, ciphertext or OS diagnostic data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    /// Caller metadata is not a non-nil UUID and bounded canonical ASCII kind.
    #[error("vault.invalid_context")]
    Context,
    /// Key has been dropped; this instance cannot encrypt or resolve credentials.
    #[error("vault.locked")]
    Locked,
    /// Input would exceed the fixed allocation budget.
    #[error("vault.secret_too_large")]
    Size,
    /// OS entropy is unavailable; never substitute predictable bytes.
    #[error("vault.entropy_unavailable")]
    Entropy,
    /// Authentication failed; deliberately does not distinguish tampering causes.
    #[error("vault.authentication_failed")]
    Authentication,
    /// Cipher construction or encryption failed.
    #[error("vault.crypto_failed")]
    Cipher,
}
/// Trusted caller's expected row metadata, not metadata supplied by encrypted input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretContext {
    id: Uuid,
    kind: String,
}
impl SecretContext {
    /// Kind labels are canonical lower ASCII identifiers, not a credential taxonomy.
    pub fn new(id: Uuid, kind: &str) -> Result<Self, CryptoError> {
        if id.is_nil()
            || kind.is_empty()
            || kind.len() > 32
            || !kind.as_bytes()[0].is_ascii_lowercase()
            || !kind
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(CryptoError::Context);
        }
        Ok(Self {
            id,
            kind: kind.into(),
        })
    }
    fn aad(&self) -> Vec<u8> {
        let mut aad = Vec::with_capacity(16 + self.kind.len());
        aad.extend_from_slice(self.id.as_bytes());
        aad.extend_from_slice(self.kind.as_bytes());
        aad
    }
}
/// Unauthenticated transport/storage bytes; no durable envelope format is defined yet.
#[derive(Clone)]
pub struct SealedSecret {
    /// Exactly 192 nonce bits; generated independently for every seal operation.
    pub nonce: [u8; 24],
    /// Ciphertext followed by a 16-byte authentication tag.
    pub ciphertext: Vec<u8>,
}
/// Plaintext is only explicitly exposed to trusted Rust consumers and clears on drop.
/// It cannot accidentally be logged or serialized by a DTO.
/// ```compile_fail
/// fn log(value: pulse_store::vault_crypto::SecretBytes) { println!("{value:?}"); }
/// ```
/// ```compile_fail
/// fn send(value: pulse_store::vault_crypto::SecretBytes) { let _ = serde_json::to_vec(&value); }
/// ```
pub struct SecretBytes(Zeroizing<Vec<u8>>);
impl SecretBytes {
    /// Borrow for an authorized transport operation; never register this as frontend IPC.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
    /// Explicit early clear, in addition to zeroizing on drop.
    pub fn clear(&mut self) {
        self.0.zeroize();
    }
}
/// Memory-only DEK/cipher holder; no Debug, Clone, serialization or implicit unlock.
/// Cipher `zeroize` feature is mandatory; locking drops its owned key material.
/// ```compile_fail
/// fn log(value: pulse_store::vault_crypto::VaultCipher) { println!("{value:?}"); }
/// ```
pub struct VaultCipher {
    cipher: Option<XChaCha20Poly1305>,
}
impl VaultCipher {
    /// Generate a 256-bit DEK through a fallible OS CSPRNG, never a fallback seed.
    pub fn generate() -> Result<Self, CryptoError> {
        let mut material = Zeroizing::new([0; 32]);
        getrandom::fill(&mut material[..]).map_err(|_| CryptoError::Entropy)?;
        Self::from_key_material(material)
    }
    /// Internal adapter import boundary; the caller must have already authorized key use.
    /// Material's owned source buffer is zeroized after cipher construction.
    pub fn from_key_material(material: Zeroizing<[u8; 32]>) -> Result<Self, CryptoError> {
        let cipher =
            XChaCha20Poly1305::new_from_slice(&material[..]).map_err(|_| CryptoError::Cipher)?;
        Ok(Self {
            cipher: Some(cipher),
        })
    }
    /// Drop the key and reject all future operations. Existing transports are untouched.
    pub fn lock(&mut self) {
        self.cipher = None;
    }
    /// This is key availability, not an authorization decision or successful OS lock.
    pub fn is_locked(&self) -> bool {
        self.cipher.is_none()
    }
    /// Encrypt a bounded value with fresh OS random nonce and id/kind AAD.
    pub fn seal(
        &self,
        context: &SecretContext,
        plaintext: &[u8],
    ) -> Result<SealedSecret, CryptoError> {
        let cipher = self.cipher.as_ref().ok_or(CryptoError::Locked)?;
        if plaintext.len() > SECRET_BYTES_LIMIT {
            return Err(CryptoError::Size);
        }
        let mut nonce = [0; 24];
        getrandom::fill(&mut nonce).map_err(|_| CryptoError::Entropy)?;
        let aad = context.aad();
        let ciphertext = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Cipher)?;
        Ok(SealedSecret { nonce, ciphertext })
    }
    /// Authenticate against the expected caller context before returning zeroizing bytes.
    /// This is a trusted backend operation, never a frontend plaintext-read command.
    pub fn open(
        &self,
        context: &SecretContext,
        sealed: &SealedSecret,
    ) -> Result<SecretBytes, CryptoError> {
        let cipher = self.cipher.as_ref().ok_or(CryptoError::Locked)?;
        if sealed.ciphertext.len() > SECRET_BYTES_LIMIT + 16 {
            return Err(CryptoError::Size);
        }
        let aad = context.aad();
        let plaintext = cipher
            .decrypt(
                &XNonce::from(sealed.nonce),
                Payload {
                    msg: &sealed.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Authentication)?;
        Ok(SecretBytes(Zeroizing::new(plaintext)))
    }
}
#[cfg(test)]
#[path = "vault_crypto_tests.rs"]
mod vault_crypto_tests;
