//! Algorithm admission shared by import and actual SSH use.
use pkcs8::{PrivateKeyInfoRef, der::Decode};
use pulse_app::vault_owner::VaultOwnerError;
use russh::keys::{Algorithm, EcdsaCurve, PrivateKey};

type Result<T> = std::result::Result<T, VaultOwnerError>;

pub(super) fn check_key(key: &PrivateKey) -> Result<()> {
    match key.algorithm() {
        Algorithm::Ed25519
        | Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256 | EcdsaCurve::NistP384 | EcdsaCurve::NistP521,
        } => Ok(()),
        _ => Err(VaultOwnerError::Denied),
    }
}

/// Inspect algorithm and named curve before any algorithm-specific key constructor.
/// Software RSA remains rejected while the backend dependency has an unpatched advisory.
pub(super) fn check_pkcs8(bytes: &[u8]) -> Result<()> {
    let info = PrivateKeyInfoRef::from_der(bytes).map_err(|_| VaultOwnerError::Denied)?;
    match info.algorithm.oid.to_string().as_str() {
        "1.3.101.112" => {
            if info.private_key.as_bytes().len() > 64 {
                return Err(VaultOwnerError::Denied);
            }
        }
        "1.2.840.10045.2.1" => {
            let curve = info
                .algorithm
                .parameters_oid()
                .map_err(|_| VaultOwnerError::Denied)?;
            if !matches!(
                curve.to_string().as_str(),
                "1.2.840.10045.3.1.7" | "1.3.132.0.34" | "1.3.132.0.35"
            ) || info.private_key.as_bytes().len() > 512
            {
                return Err(VaultOwnerError::Denied);
            }
        }
        _ => return Err(VaultOwnerError::Denied),
    }
    Ok(())
}
