//! Generated keys exercise wire formats, public identities and real cryptographic signatures.
use super::*;
use pkcs8::{
    AlgorithmIdentifierRef, PrivateKeyInfoRef,
    der::asn1::{AnyRef, ObjectIdentifier},
};
use russh::keys::{
    EcdsaCurve,
    ssh_key::{
        HashAlg,
        private::{EcdsaKeypair, RsaKeypair},
    },
};

fn round_trip(key: &PrivateKey) -> Result {
    let plain = key.to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
    let encrypted = key
        .encrypt(&mut rand::rng(), b"fixture passphrase")?
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
    for (bytes, pass) in [
        (plain.as_bytes(), None),
        (encrypted.as_bytes(), Some(b"fixture passphrase".as_slice())),
    ] {
        let decoded = decode_bounded(bytes, pass).map_err(|_| "OpenSSH key rejected")?;
        assert_eq!(decoded.public_key(), key.public_key());
        let signature = decoded.sign(
            "pulseterm-algorithm-fixture",
            HashAlg::Sha512,
            b"fixed fixture message",
        )?;
        decoded.public_key().verify(
            "pulseterm-algorithm-fixture",
            b"fixed fixture message",
            &signature,
        )?;
    }
    assert!(decode_bounded(encrypted.as_bytes(), Some(b"wrong passphrase")).is_err());
    assert!(decode_bounded(encrypted.as_bytes(), None).is_err());
    Ok(())
}

#[test]
fn all_nist_ecdsa_openssh_keys_preserve_public_identity_and_signatures() -> Result {
    for curve in [
        EcdsaCurve::NistP256,
        EcdsaCurve::NistP384,
        EcdsaCurve::NistP521,
    ] {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ecdsa { curve })?;
        round_trip(&key)?;
    }
    Ok(())
}

fn pem_round_trip(document: &SecretDocument, expected: &PrivateKey) -> Result {
    let plain = document.to_pem("PRIVATE KEY", LineEnding::LF)?;
    let decoded = decode_bounded(plain.as_bytes(), None).map_err(|_| "PKCS8 key rejected")?;
    assert_eq!(decoded.public_key(), expected.public_key());
    let params = parameters(pbes2::EncryptionScheme::Aes256Cbc { iv: [23; 16] })?;
    let ciphertext = params.encrypt(b"fixture passphrase", document.as_bytes())?;
    let encrypted = encrypted_pem(params, &ciphertext)?;
    let decoded = decode_bounded(encrypted.as_bytes(), Some(b"fixture passphrase"))
        .map_err(|_| "encrypted PKCS8 key rejected")?;
    assert_eq!(decoded.public_key(), expected.public_key());
    let signature = decoded.sign(
        "pulseterm-algorithm-fixture",
        HashAlg::Sha512,
        b"fixed fixture message",
    )?;
    decoded.public_key().verify(
        "pulseterm-algorithm-fixture",
        b"fixed fixture message",
        &signature,
    )?;
    assert!(decode_bounded(encrypted.as_bytes(), Some(b"wrong passphrase")).is_err());
    Ok(())
}

#[test]
fn all_nist_ecdsa_pkcs8_plaintext_and_encrypted_keys_keep_the_same_public_identity() -> Result {
    for (curve, oid) in [
        (EcdsaCurve::NistP256, "1.2.840.10045.3.1.7"),
        (EcdsaCurve::NistP384, "1.3.132.0.34"),
        (EcdsaCurve::NistP521, "1.3.132.0.35"),
    ] {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ecdsa { curve })?;
        let private = match key.key_data().ecdsa().ok_or("wrong fixture algorithm")? {
            EcdsaKeypair::NistP256 { private, .. } => private.as_slice(),
            EcdsaKeypair::NistP384 { private, .. } => private.as_slice(),
            EcdsaKeypair::NistP521 { private, .. } => private.as_slice(),
        };
        // SEC1 ECPrivateKey version and scalar, with the named curve carried by PKCS8.
        let mut sec1 = Zeroizing::new(vec![
            0x30,
            (private.len() + 5) as u8,
            0x02,
            0x01,
            0x01,
            0x04,
            private.len() as u8,
        ]);
        sec1.extend_from_slice(private);
        let curve_oid = ObjectIdentifier::new(oid)?;
        let info = PrivateKeyInfoRef::new(
            AlgorithmIdentifierRef {
                oid: ObjectIdentifier::new("1.2.840.10045.2.1")?,
                parameters: Some(AnyRef::from(&curve_oid)),
            },
            OctetStringRef::new(&sec1)?,
        );
        let document = SecretDocument::try_from(info)?;
        pem_round_trip(&document, &key)?;
    }
    Ok(())
}

#[test]
fn rsa_openssh_is_rejected_including_otherwise_acceptable_moduli_and_exponents() -> Result {
    use russh::keys::ssh_key::{
        Mpint,
        private::{KeypairData, RsaPrivateKey},
        public::RsaPublicKey,
    };
    for (bits, exponent) in [
        (2048usize, vec![1, 0, 1]),
        (8192usize, vec![1, 0, 1]),
        (8193usize, vec![1, 0, 1]),
        (2048, vec![1, 0, 0, 0, 1]),
        (2048, vec![2]),
    ] {
        let mut modulus = vec![0; bits.div_ceil(8)];
        modulus[0] = 1 << ((bits - 1) % 8);
        if let Some(last) = modulus.last_mut() {
            *last |= 1;
        }
        let public = RsaPublicKey::new(
            Mpint::from_positive_bytes(&exponent),
            Mpint::from_positive_bytes(&modulus),
        )?;
        let private = RsaPrivateKey::new(
            Mpint::from_positive_bytes(&[1]),
            Mpint::from_positive_bytes(&[1]),
            Mpint::from_positive_bytes(&[1]),
            Mpint::from_positive_bytes(&[1]),
        )?;
        let key = PrivateKey::new(KeypairData::Rsa(RsaKeypair::new(public, private)?), "")?;
        let openssh = key.to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
        assert!(decode_bounded(openssh.as_bytes(), None).is_err());
    }
    Ok(())
}

#[test]
fn rsa_and_unknown_ec_pkcs8_algorithms_are_rejected_before_key_construction() -> Result {
    for (oid, curve) in [
        ("1.2.840.113549.1.1.1", None),
        ("1.2.840.10045.2.1", Some("1.3.132.0.10")),
    ] {
        let curve = curve.map(ObjectIdentifier::new).transpose()?;
        let info = PrivateKeyInfoRef::new(
            AlgorithmIdentifierRef {
                oid: ObjectIdentifier::new(oid)?,
                parameters: curve.as_ref().map(AnyRef::from),
            },
            OctetStringRef::new(&[0x30, 0])?,
        );
        let document = SecretDocument::try_from(info)?;
        assert!(super::super::algorithms::check_pkcs8(document.as_bytes()).is_err());
        let pem = document.to_pem("PRIVATE KEY", LineEnding::LF)?;
        assert!(decode_bounded(pem.as_bytes(), None).is_err());
    }
    Ok(())
}
