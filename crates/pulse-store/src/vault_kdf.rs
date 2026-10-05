//! Fixed validation profile, not a persisted format, strength policy or OS unlock.
//! CPU work is non-cancellable; a future service must schedule off the GUI thread
//! and reject late results against its own authorization/unlock epoch.
use crate::vault_crypto::VaultCipher;
use argon2::{Algorithm, Argon2, Block, Params, Version};
use std::sync::Mutex;
use zeroize::Zeroizing;
/// Password input bound; intentionally not a production strength policy.
pub const PASSWORD_BYTES_LIMIT: usize = 4096;
/// Fixed validation cost, in KiB. Untrusted headers cannot control this allocation.
pub const VALIDATION_MEMORY_KIB: u32 = 65536;
const ALGORITHM: Algorithm = Algorithm::Argon2id;
const VERSION: Version = Version::V0x13;
static JOB: Mutex<()> = Mutex::new(());
/// Static errors contain neither password nor derived material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KdfError {
    /// Empty or oversized input is refused before admission.
    #[error("vault.kdf_invalid_input")]
    Input,
    /// Only one in-process job; poisoned admission also fails closed.
    #[error("vault.kdf_busy")]
    Busy,
    /// Explicit allocation failed before expensive computation.
    #[error("vault.kdf_memory")]
    Memory,
    /// The system entropy source did not supply a fresh salt.
    #[error("vault.kdf_entropy")]
    Entropy,
    /// Fixed algorithm construction or execution failed.
    #[error("vault.kdf_derivation")]
    Derivation,
    /// The zeroizing key could not be imported into the internal cipher.
    #[error("vault.kdf_cipher")]
    Cipher,
}
/// Fresh 128-bit salt. Persisting it and a versioned envelope is not implemented.
pub fn fresh_salt() -> Result<[u8; 16], KdfError> {
    let mut salt = [0; 16];
    getrandom::fill(&mut salt).map_err(|_| KdfError::Entropy)?;
    Ok(salt)
}
/// Derive a memory-only KEK: Argon2id v1.3, 64MiB, t=3, p=1, 32-byte output.
/// This synchronous validation profile has not passed production latency review.
/// Borrowed password lifetime/zeroization belongs to the authorized Rust caller.
/// Returned cipher is not an authorization grant or a successful vault unlock.
pub fn derive_validation_kek(password: &[u8], salt: &[u8; 16]) -> Result<VaultCipher, KdfError> {
    if password.is_empty() || password.len() > PASSWORD_BYTES_LIMIT {
        return Err(KdfError::Input);
    }
    let _job = JOB.try_lock().map_err(|_| KdfError::Busy)?;
    let params =
        Params::new(VALIDATION_MEMORY_KIB, 3, 1, Some(32)).map_err(|_| KdfError::Derivation)?;
    let count = params.block_count();
    let mut memory = Zeroizing::new(Vec::<Block>::new());
    memory
        .try_reserve_exact(count)
        .map_err(|_| KdfError::Memory)?;
    memory.resize(count, Block::default());
    let argon = Argon2::new(ALGORITHM, VERSION, params);
    let mut key = Zeroizing::new([0; 32]);
    argon
        .hash_password_into_with_memory(password, salt, &mut key[..], &mut memory[..])
        .map_err(|_| KdfError::Derivation)?;
    VaultCipher::from_key_material(key).map_err(|_| KdfError::Cipher)
}
#[cfg(test)]
#[path = "vault_kdf_tests.rs"]
mod tests;
