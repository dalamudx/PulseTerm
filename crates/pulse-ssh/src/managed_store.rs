//! The only storage-to-SSH-public-key conversion; do not use raw storage rows in a handler.
use crate::{HostKeyError, HostPin, HostTarget};
#[path = "first_pin.rs"]
mod first_pin;
pub use first_pin::{FirstPinApproval, FirstPinError, FirstPinObservation};
use pulse_store::ssh_trust::{TrustEdit, TrustRecord, TrustSnapshot, TrustTarget};
use russh::keys::PublicKey;

/// Construct a backend-only edit after a separate explicit approval decision.
/// This is NOT an approval token and is never exposed to a page or remote peer.
pub fn pin_edit(target: &HostTarget, key: &PublicKey) -> Result<TrustEdit, HostKeyError> {
    let identity = TrustTarget::new(target.host(), target.port())
        .map_err(|_| HostKeyError::StoreUnavailable)?;
    let encoded = key
        .to_openssh()
        .map_err(|_| HostKeyError::StoreUnavailable)?;
    let record = TrustRecord::new(identity, encoded).map_err(|_| HostKeyError::StoreUnavailable)?;
    Ok(TrustEdit::Pin(record))
}
/// Construct an explicit forget request without re-enabling automatic first-contact trust.
pub fn forget_edit(target: &HostTarget) -> Result<TrustEdit, HostKeyError> {
    Ok(TrustEdit::Forget(
        TrustTarget::new(target.host(), target.port())
            .map_err(|_| HostKeyError::StoreUnavailable)?,
    ))
}
/// Parse **every** stored public key before authentication. One corrupted record fails the
/// entire source closed; callers must not substitute an empty list or fall back to system pins.
pub fn validated_pins(snapshot: &TrustSnapshot) -> Result<Vec<HostPin>, HostKeyError> {
    let mut pins = Vec::with_capacity(snapshot.records().len());
    for record in snapshot.records() {
        let target = HostTarget::new(record.target().host(), record.target().port())
            .map_err(|_| HostKeyError::StoreUnavailable)?;
        if target.host() != record.target().host() {
            return Err(HostKeyError::StoreUnavailable);
        }
        let key =
            PublicKey::from_openssh(record.key()).map_err(|_| HostKeyError::StoreUnavailable)?;
        pins.push(HostPin::new(target, key));
    }
    Ok(pins)
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, PrivateKey};
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[tokio::test]
    async fn malformed_stored_key_fails_entire_source_not_system_fallback() -> TestResult {
        let d = tempfile::tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        let (store, task) = pulse_store::StateStore::open(d.path().join("trust.db")).await?;
        let target = HostTarget::new("alpha.example", 22)?;
        let correct = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?
            .public_key()
            .clone();
        let snapshot = store.load_ssh_trust().await?;
        let poisoned = TrustRecord::new(
            TrustTarget::new(target.host(), target.port())?,
            "ssh-ed25519 AAAA".into(),
        )?;
        let raw = store
            .edit_ssh_trust(snapshot, TrustEdit::Pin(poisoned))
            .await?;
        assert!(matches!(
            validated_pins(&raw),
            Err(HostKeyError::StoreUnavailable)
        ));
        // Neither a correct system pin nor an empty own list can override a corrupt own row.
        let system = [HostPin::new(target.clone(), correct.clone())];
        assert_eq!(
            crate::verify_host_key(&target, &correct, None, Some(&system)),
            Err(HostKeyError::StoreUnavailable)
        );
        let replaced = store
            .edit_ssh_trust(raw, pin_edit(&target, &correct)?)
            .await?;
        let parsed = validated_pins(&replaced)?;
        assert_eq!(
            crate::verify_host_key(&target, &correct, Some(&parsed), Some(&system)),
            Ok(())
        );
        task.shutdown().await?;
        Ok(())
    }
    #[test]
    fn store_target_and_final_dial_normalization_agree() -> TestResult {
        for host in ["Alpha.Example", "127.0.0.1", "::1", "fe80::3"] {
            let target = HostTarget::new(host, 2222)?;
            let record = TrustTarget::new(host, 2222)?;
            assert_eq!(target.host(), record.host());
            assert_eq!(target.port(), record.port());
        }
        for bad in ["", "ab..com", "[::1]", "space .example", "a_.example"] {
            assert!(HostTarget::new(bad, 22).is_err());
            assert!(TrustTarget::new(bad, 22).is_err());
        }
        Ok(())
    }
}
