use crate::vault_crypto::SecretContext;
use crate::vault_wrap::{
    ValidationWrappedDek, WrapError, create_validation_vault, recover_validation_dek,
};
use uuid::Uuid;
#[test]
fn envelope_reopen_recovers_same_key_and_credential_not_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = crate::vault_kdf::TEST_SCENARIOS
        .lock()
        .map_err(|_| "fixture.lock")?;
    let id = Uuid::new_v4();
    let password = zeroize::Zeroizing::new(b"probe-only-password".to_vec());
    let (envelope, mut original) = create_validation_vault(&password, id)?;
    let context = SecretContext::new(Uuid::new_v4(), "ssh_password")?;
    let credential = original.seal(&context, b"probe-only-secret")?;
    let bytes = envelope.encode_ciphertext()?;
    assert_eq!(bytes.len(), 110);
    original.lock();
    let stored = ValidationWrappedDek::decode_ciphertext(&bytes, id)?;
    let restored = recover_validation_dek(&password, id, &stored)?.into_cipher();
    assert_eq!(
        restored.open(&context, &credential)?.expose(),
        b"probe-only-secret"
    );
    assert!(recover_validation_dek(b"wrong-probe-password", id, &stored).is_err());
    let mut tampered = bytes.clone();
    tampered[62] ^= 1;
    let bad = ValidationWrappedDek::decode_ciphertext(&tampered, id)?;
    assert!(recover_validation_dek(&password, id, &bad).is_err());
    Ok(())
}
#[test]
fn bounded_envelope_rejects_all_lengths_versions_profiles_and_identity_before_kdf() {
    let id = Uuid::new_v4();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    assert!(ValidationWrappedDek::decode_ciphertext(&bytes, id).is_ok());
    for len in 0..110 {
        assert!(matches!(
            ValidationWrappedDek::decode_ciphertext(&bytes[..len], id),
            Err(WrapError::Envelope)
        ));
    }
    for pos in 0..6 {
        let mut malformed = bytes.clone();
        malformed[pos] ^= 1;
        assert!(ValidationWrappedDek::decode_ciphertext(&malformed, id).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(ValidationWrappedDek::decode_ciphertext(&extra, id).is_err());
    assert!(ValidationWrappedDek::decode_ciphertext(&bytes, Uuid::nil()).is_err());
    assert!(ValidationWrappedDek::decode_ciphertext(&bytes, Uuid::new_v4()).is_err());
    assert!(ValidationWrappedDek::decode_ciphertext(&vec![0; 1_048_576], id).is_err());
}
