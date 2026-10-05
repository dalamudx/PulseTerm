use super::*;
use crate::vault_kdf::TEST_SCENARIOS;
#[test]
fn only_authenticated_key_recovers_credential_and_wrong_password_or_tamper_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let _scenario = TEST_SCENARIOS.lock().map_err(|_| KdfError::Busy)?;
    let vault_id = Uuid::from_bytes([1; 16]);
    let secret_id = Uuid::from_bytes([2; 16]);
    let context = SecretContext::new(secret_id, "password")?;
    let (envelope, mut original) = create_validation_vault(b"validation only", vault_id)?;
    assert_eq!(envelope.sealed.ciphertext.len(), SEALED_KEY_LENGTH);
    let credential = original.seal(&context, b"fixed fixture credential")?;
    original.lock();
    assert!(matches!(
        original.open(&context, &credential),
        Err(CryptoError::Locked)
    ));
    let recovered = recover_validation_dek(b"validation only", vault_id, &envelope)?;
    let lifecycle = crate::vault_lifecycle::VaultKeyLifecycle::new(vault_id)?;
    let mut permit = lifecycle.begin()?;
    let mut candidate = permit.worker()?.finish(recovered)?;
    assert_eq!(
        lifecycle.status()?,
        crate::vault_lifecycle::KeyStatus::Pending
    );
    lifecycle.confirm(&mut permit, &mut candidate)?;
    assert_eq!(
        lifecycle.open_for_adapter(&context, &credential)?.expose(),
        b"fixed fixture credential"
    );
    assert!(matches!(
        recover_validation_dek(b"incorrect", vault_id, &envelope),
        Err(WrapError::Crypto(CryptoError::Authentication))
    ));
    let mut damaged = envelope.clone();
    if let Some(last) = damaged.sealed.ciphertext.last_mut() {
        *last ^= 1;
    }
    assert!(matches!(
        recover_validation_dek(b"validation only", vault_id, &damaged),
        Err(WrapError::Crypto(CryptoError::Authentication))
    ));
    // Even changing the metadata to agree with the caller cannot move the blob:
    // the original identity is cryptographically bound into AAD.
    let mut moved = envelope.clone();
    moved.vault_id = secret_id;
    assert!(matches!(
        recover_validation_dek(b"validation only", secret_id, &moved),
        Err(WrapError::Crypto(CryptoError::Authentication))
    ));
    Ok(())
}
#[test]
fn malformed_or_wrong_identity_is_refused_before_kdf_and_does_not_mutate_envelope() {
    let envelope = ValidationWrappedDek {
        vault_id: Uuid::from_bytes([1; 16]),
        salt: [0; 16],
        sealed: SealedSecret {
            nonce: [0; 24],
            ciphertext: vec![0; SEALED_KEY_LENGTH],
        },
    };
    assert!(matches!(
        recover_validation_dek(b"valid", Uuid::from_bytes([2; 16]), &envelope),
        Err(WrapError::Envelope)
    ));
    let mut short = envelope.clone();
    short.sealed.ciphertext.pop();
    assert!(matches!(
        recover_validation_dek(b"valid", envelope.vault_id, &short),
        Err(WrapError::Envelope)
    ));
    assert_eq!(envelope.sealed.ciphertext.len(), SEALED_KEY_LENGTH);
    assert!(create_validation_vault(b"fixed", Uuid::nil()).is_err());
}
