use super::*;
use pkcs8::{LineEnding, der::asn1::OctetStringRef};
use russh::keys::Algorithm;
use zeroize::Zeroizing;
#[path = "ssh_private_key_algorithm_tests.rs"]
mod algorithm_tests;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn fixture() -> Result<SecretDocument> {
    let mut bytes = Zeroizing::new(vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ]);
    bytes.extend_from_slice(&[37; 32]);
    Ok(SecretDocument::try_from(bytes.as_slice())?)
}

fn parameters(encryption: pbes2::EncryptionScheme) -> Result<pbes2::Parameters> {
    Ok(pbes2::Parameters {
        kdf: pbes2::Kdf::Pbkdf2(pbes2::Pbkdf2Params {
            salt: pbes2::Salt::new([17; 16])?,
            iteration_count: 2048,
            key_length: None,
            prf: pbes2::Pbkdf2Prf::HmacWithSha256,
        }),
        encryption,
    })
}

fn encrypted_pem(params: pbes2::Parameters, ciphertext: &[u8]) -> Result<Zeroizing<String>> {
    let encrypted = pkcs8::EncryptedPrivateKeyInfoRef {
        encryption_algorithm: params.into(),
        encrypted_data: OctetStringRef::new(ciphertext)?,
    };
    Ok(SecretDocument::try_from(encrypted)?.to_pem("ENCRYPTED PRIVATE KEY", LineEnding::LF)?)
}

#[test]
fn plaintext_pem_and_openssh_resolve_to_the_same_public_identity() -> Result {
    let document = fixture()?;
    let pem = document.to_pem("PRIVATE KEY", LineEnding::LF)?;
    let key = decode_bounded(pem.as_bytes(), None).map_err(|_| "fixture rejected")?;
    let openssh = key.to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
    let same = decode_bounded(openssh.as_bytes(), None).map_err(|_| "OpenSSH fixture rejected")?;
    assert_eq!(key.algorithm(), Algorithm::Ed25519);
    assert_eq!(key.public_key(), same.public_key());
    assert!(decode_bounded(pem.as_bytes(), Some(b"unused")).is_err());
    Ok(())
}

#[test]
fn bounded_encrypted_pem_supports_both_prfs_and_aes256_schemes() -> Result {
    let document = fixture()?;
    let expected = russh::keys::pkcs8::decode_pkcs8(document.as_bytes(), None)?;
    for encryption in [
        pbes2::EncryptionScheme::Aes256Cbc { iv: [23; 16] },
        pbes2::EncryptionScheme::Aes256Gcm { nonce: [29; 12] },
    ] {
        for prf in [
            pbes2::Pbkdf2Prf::HmacWithSha256,
            pbes2::Pbkdf2Prf::HmacWithSha512,
        ] {
            let mut params = parameters(encryption)?;
            let pbes2::Kdf::Pbkdf2(kdf) = &mut params.kdf else {
                return Err("wrong fixture KDF".into());
            };
            kdf.prf = prf;
            let ciphertext = params.encrypt(b"fixture passphrase", document.as_bytes())?;
            let pem = encrypted_pem(params, &ciphertext)?;
            let key = decode_bounded(pem.as_bytes(), Some(b"fixture passphrase"))
                .map_err(|_| "encrypted fixture rejected")?;
            assert_eq!(key.public_key(), expected.public_key());
            assert!(decode_bounded(pem.as_bytes(), None).is_err());
            assert!(decode_bounded(pem.as_bytes(), Some(b"wrong fixture passphrase")).is_err());
        }
    }
    Ok(())
}

#[test]
fn excessive_rounds_short_salt_key_length_and_legacy_prf_are_rejected() -> Result {
    for case in 0..5 {
        let mut params = parameters(pbes2::EncryptionScheme::Aes256Cbc { iv: [23; 16] })?;
        let pbes2::Kdf::Pbkdf2(kdf) = &mut params.kdf else {
            return Err("wrong fixture KDF".into());
        };
        match case {
            0 => kdf.iteration_count = PBKDF2_ROUNDS_LIMIT + 1,
            1 => kdf.iteration_count = 0,
            2 => kdf.salt = pbes2::Salt::new([17; 7])?,
            3 => kdf.key_length = Some(64),
            _ => kdf.prf = pbes2::Pbkdf2Prf::HmacWithSha1,
        }
        assert!(!bounded_pkcs8(&params));
        let pem = encrypted_pem(params, &[0; 16])?;
        assert!(decode_bounded(pem.as_bytes(), Some(b"fixture passphrase")).is_err());
    }
    let params = parameters(pbes2::EncryptionScheme::Aes128Cbc { iv: [23; 16] })?;
    assert!(!bounded_pkcs8(&params));
    Ok(())
}

#[test]
fn multiple_blocks_preamble_wrong_labels_and_oversized_inputs_are_rejected() -> Result {
    let document = fixture()?;
    let pem = document.to_pem("PRIVATE KEY", LineEnding::LF)?;
    let mut doubled = Zeroizing::new(String::from(pem.as_str()));
    doubled.push_str(pem.as_str());
    let mut prefixed = Zeroizing::new(String::from("not a key\n"));
    prefixed.push_str(pem.as_str());
    let mislabeled = document.to_pem("ENCRYPTED PRIVATE KEY", LineEnding::LF)?;
    assert!(decode_bounded(doubled.as_bytes(), None).is_err());
    assert!(decode_bounded(prefixed.as_bytes(), None).is_err());
    assert!(decode_bounded(mislabeled.as_bytes(), Some(b"fixture passphrase")).is_err());
    assert!(decode_bounded(&[0xff], None).is_err());
    assert!(decode_bounded(&vec![0; KEY_BYTES_LIMIT + 1], None).is_err());
    assert!(decode_bounded(pem.as_bytes(), Some(&vec![0; PASSPHRASE_BYTES_LIMIT + 1])).is_err());
    Ok(())
}
