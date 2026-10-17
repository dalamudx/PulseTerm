//! Fixed-password, generated-key material used only by the loopback development probe.
use anyhow::Result;
use pkcs8::{
    AlgorithmIdentifierRef, PrivateKeyInfoRef, SecretDocument,
    der::asn1::{AnyRef, ObjectIdentifier, OctetStringRef},
    pkcs5::pbes2,
};
use russh::keys::{PrivateKey, ssh_key::private::EcdsaKeypair};
use zeroize::Zeroizing;

pub(super) fn encrypted(key: &PrivateKey, password: &[u8]) -> Result<Zeroizing<String>> {
    let pair = key
        .key_data()
        .ecdsa()
        .ok_or_else(|| anyhow::anyhow!("fixture.expected_ecdsa"))?;
    let (private, oid) = match pair {
        EcdsaKeypair::NistP256 { private, .. } => (private.as_slice(), "1.2.840.10045.3.1.7"),
        EcdsaKeypair::NistP384 { private, .. } => (private.as_slice(), "1.3.132.0.34"),
        EcdsaKeypair::NistP521 { private, .. } => (private.as_slice(), "1.3.132.0.35"),
    };
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
    let curve = ObjectIdentifier::new(oid)?;
    let info = PrivateKeyInfoRef::new(
        AlgorithmIdentifierRef {
            oid: ObjectIdentifier::new("1.2.840.10045.2.1")?,
            parameters: Some(AnyRef::from(&curve)),
        },
        OctetStringRef::new(&sec1)?,
    );
    let document = SecretDocument::try_from(info)?;
    let params = pbes2::Parameters {
        kdf: pbes2::Kdf::Pbkdf2(pbes2::Pbkdf2Params {
            salt: pbes2::Salt::new([17; 16])?,
            iteration_count: 2048,
            key_length: None,
            prf: pbes2::Pbkdf2Prf::HmacWithSha256,
        }),
        encryption: pbes2::EncryptionScheme::Aes256Cbc { iv: [23; 16] },
    };
    let ciphertext = params.encrypt(password, document.as_bytes())?;
    let info = pkcs8::EncryptedPrivateKeyInfoRef {
        encryption_algorithm: params.into(),
        encrypted_data: OctetStringRef::new(&ciphertext)?,
    };
    Ok(SecretDocument::try_from(info)?.to_pem("ENCRYPTED PRIVATE KEY", pkcs8::LineEnding::LF)?)
}
