//! Bounded OpenSSH/PKCS#8 parsing and one CPU reservation for import and authentication.
use pkcs8::{EncryptedPrivateKeyInfoRef, SecretDocument, der::Decode, pkcs5::pbes2};
use pulse_app::vault_owner::VaultOwnerError;
use russh::keys::{PrivateKey, ssh_key::Kdf};
#[path = "ssh_private_key_algorithms.rs"]
mod algorithms;
use std::sync::{Arc, LazyLock};
pub(crate) static DECODE_BUDGET: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(1)));
pub(crate) const KEY_BYTES_LIMIT: usize =
    pulse_store::vault_private_key_ingress::PRIVATE_KEY_BYTES_LIMIT;
pub(crate) const PASSPHRASE_BYTES_LIMIT: usize = 4096;
const BCRYPT_ROUNDS_LIMIT: u32 = 64;
const PBKDF2_ROUNDS_LIMIT: u32 = 600_000;

fn bounded_pkcs8(params: &pbes2::Parameters) -> bool {
    let pbes2::Kdf::Pbkdf2(kdf) = &params.kdf else {
        return false;
    };
    (1..=PBKDF2_ROUNDS_LIMIT).contains(&kdf.iteration_count)
        && (8..=64).contains(&kdf.salt.as_ref().len())
        && matches!(kdf.key_length, None | Some(32))
        && matches!(
            kdf.prf,
            pbes2::Pbkdf2Prf::HmacWithSha256 | pbes2::Pbkdf2Prf::HmacWithSha512
        )
        && matches!(
            params.encryption,
            pbes2::EncryptionScheme::Aes256Cbc { .. } | pbes2::EncryptionScheme::Aes256Gcm { .. }
        )
}

fn decode_pem(bytes: &[u8], passphrase: Option<&[u8]>) -> Result<PrivateKey, VaultOwnerError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| VaultOwnerError::Denied)?
        .trim_ascii();
    let label = text
        .strip_prefix("-----BEGIN ")
        .and_then(|rest| rest.split_once("-----"))
        .map(|(label, _)| label)
        .filter(|label| matches!(*label, "PRIVATE KEY" | "ENCRYPTED PRIVATE KEY"))
        .ok_or(VaultOwnerError::Denied)?;
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let body = text
        .strip_prefix(&begin)
        .and_then(|body| body.strip_suffix(&end))
        .ok_or(VaultOwnerError::Denied)?;
    if body.contains('-') {
        return Err(VaultOwnerError::Denied);
    }
    let (parsed_label, document) =
        SecretDocument::from_pem(text).map_err(|_| VaultOwnerError::Denied)?;
    if parsed_label != label {
        return Err(VaultOwnerError::Denied);
    }
    let plaintext = match (label, passphrase) {
        ("PRIVATE KEY", None) => document,
        ("ENCRYPTED PRIVATE KEY", Some(pass)) if !pass.is_empty() => {
            let encrypted = EncryptedPrivateKeyInfoRef::from_der(document.as_bytes())
                .map_err(|_| VaultOwnerError::Denied)?;
            let pkcs8::pkcs5::EncryptionScheme::Pbes2(params) = &encrypted.encryption_algorithm
            else {
                return Err(VaultOwnerError::Denied);
            };
            if !bounded_pkcs8(params) {
                return Err(VaultOwnerError::Denied);
            }
            encrypted
                .decrypt(pass)
                .map_err(|_| VaultOwnerError::Denied)?
        }
        _ => return Err(VaultOwnerError::Denied),
    };
    algorithms::check_pkcs8(plaintext.as_bytes())?;
    let key = russh::keys::pkcs8::decode_pkcs8(plaintext.as_bytes(), None)
        .map_err(|_| VaultOwnerError::Denied)?;
    algorithms::check_key(&key)?;
    Ok(key)
}
fn bounded_kdf(kdf: &Kdf) -> bool {
    match kdf {
        Kdf::None => true,
        Kdf::Bcrypt { salt, rounds } => {
            !salt.is_empty() && salt.len() <= 64 && (1..=BCRYPT_ROUNDS_LIMIT).contains(rounds)
        }
        _ => false,
    }
}
/// Content-free refusal before expensive derivation for unsupported or excessive KDF parameters.
pub(crate) fn decode_bounded(
    key: &[u8],
    passphrase: Option<&[u8]>,
) -> Result<PrivateKey, VaultOwnerError> {
    if key.is_empty()
        || key.len() > KEY_BYTES_LIMIT
        || passphrase.is_some_and(|pass| pass.len() > PASSPHRASE_BYTES_LIMIT)
    {
        return Err(VaultOwnerError::Denied);
    }
    let key = match PrivateKey::from_openssh(key) {
        Ok(key) => key,
        Err(_) => return decode_pem(key, passphrase),
    };
    algorithms::check_key(&key)?;
    if !bounded_kdf(key.kdf()) {
        return Err(VaultOwnerError::Denied);
    }
    let key = match (key.is_encrypted(), key.kdf(), passphrase) {
        (false, Kdf::None, None) => key,
        (true, Kdf::Bcrypt { .. }, Some(pass)) if !pass.is_empty() => {
            key.decrypt(pass).map_err(|_| VaultOwnerError::Denied)?
        }
        _ => return Err(VaultOwnerError::Denied),
    };
    algorithms::check_key(&key)?;
    Ok(key)
}

#[cfg(test)]
#[path = "ssh_private_key_decode_tests.rs"]
mod tests;
