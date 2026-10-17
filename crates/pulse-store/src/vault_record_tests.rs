use crate::vault_crypto::{SECRET_BYTES_LIMIT, VaultCipher};
use crate::vault_record::{CredentialRecord, RECORD_BYTES_LIMIT};
use uuid::Uuid;
#[test]
fn cipher_record_roundtrip_authenticates_trusted_identity_and_rejects_tampering()
-> Result<(), Box<dyn std::error::Error>> {
    let vault = Uuid::new_v4();
    let id = Uuid::new_v4();
    let mut cipher = VaultCipher::generate()?;
    let record = CredentialRecord::seal(vault, id, "ssh_password", &cipher, b"probe-only-secret")?;
    let bytes = record.encode_ciphertext()?;
    let decoded = CredentialRecord::decode_ciphertext(&bytes, vault)?;
    assert_eq!(
        decoded.open(vault, id, "ssh_password", &cipher)?.expose(),
        b"probe-only-secret"
    );
    assert!(
        decoded
            .open(Uuid::new_v4(), id, "ssh_password", &cipher)
            .is_err()
    );
    assert!(
        decoded
            .open(vault, Uuid::new_v4(), "ssh_password", &cipher)
            .is_err()
    );
    assert!(decoded.open(vault, id, "ssh_key", &cipher).is_err());
    let mut tampered = bytes.clone();
    *tampered.last_mut().ok_or("fixture.bytes")? ^= 1;
    let bad = CredentialRecord::decode_ciphertext(&tampered, vault)?;
    assert!(bad.open(vault, id, "ssh_password", &cipher).is_err());
    cipher.lock();
    assert!(decoded.open(vault, id, "ssh_password", &cipher).is_err());
    Ok(())
}
#[test]
fn record_parser_rejects_truncation_unknown_profile_kind_and_length_budgets()
-> Result<(), Box<dyn std::error::Error>> {
    let vault = Uuid::new_v4();
    let cipher = VaultCipher::generate()?;
    let bytes = CredentialRecord::seal(vault, Uuid::new_v4(), "ssh_key", &cipher, b"sample")?
        .encode_ciphertext()?;
    for len in 0..bytes.len() {
        assert!(CredentialRecord::decode_ciphertext(&bytes[..len], vault).is_err());
    }
    for pos in 0..6 {
        let mut bad = bytes.clone();
        bad[pos] ^= 1;
        assert!(CredentialRecord::decode_ciphertext(&bad, vault).is_err());
    }
    for n in [0, 33, 255] {
        let mut bad = bytes.clone();
        bad[38] = n;
        assert!(CredentialRecord::decode_ciphertext(&bad, vault).is_err());
    }
    let mut kind = bytes.clone();
    kind[39] = b'S';
    assert!(CredentialRecord::decode_ciphertext(&kind, vault).is_err());
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(CredentialRecord::decode_ciphertext(&extra, vault).is_err());
    assert!(CredentialRecord::decode_ciphertext(&bytes, Uuid::new_v4()).is_err());
    assert!(CredentialRecord::decode_ciphertext(&vec![0; RECORD_BYTES_LIMIT + 1], vault).is_err());
    let largest = CredentialRecord::seal(
        vault,
        Uuid::new_v4(),
        &"a".repeat(32),
        &cipher,
        &vec![1; SECRET_BYTES_LIMIT],
    )?
    .encode_ciphertext()?;
    assert_eq!(largest.len(), RECORD_BYTES_LIMIT);
    assert!(CredentialRecord::decode_ciphertext(&largest, vault).is_ok());
    Ok(())
}
