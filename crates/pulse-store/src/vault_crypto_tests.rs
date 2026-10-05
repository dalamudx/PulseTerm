use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn context(kind: &str) -> Result<SecretContext, CryptoError> {
    SecretContext::new(Uuid::from_bytes([1; 16]), kind)
}
#[test]
fn bytes_round_trip_including_unicode_zero_and_empty() -> TestResult {
    let cipher = VaultCipher::generate()?;
    let context = context("password")?;
    for input in [b"".as_slice(), b"\0\xff", "fixture中文🔐".as_bytes()] {
        let sealed = cipher.seal(&context, input)?;
        assert_eq!(sealed.ciphertext.len(), input.len() + 16);
        assert_eq!(cipher.open(&context, &sealed)?.expose(), input);
    }
    Ok(())
}
#[test]
fn nonce_is_fresh_and_different_key_is_not_accepted() -> TestResult {
    let cipher = VaultCipher::generate()?;
    let context = context("password")?;
    let first = cipher.seal(&context, b"fixture")?;
    let second = cipher.seal(&context, b"fixture")?;
    assert_ne!(first.nonce, second.nonce);
    assert_ne!(first.ciphertext, second.ciphertext);
    assert!(matches!(
        VaultCipher::generate()?.open(&context, &first),
        Err(CryptoError::Authentication)
    ));
    Ok(())
}
#[test]
fn metadata_binding_is_unambiguous_and_blocks_cross_row_or_kind() -> TestResult {
    let cipher = VaultCipher::generate()?;
    let context = context("password")?;
    let mut expected = vec![1; 16];
    expected.extend_from_slice(b"password");
    assert_eq!(context.aad(), expected);
    let sealed = cipher.seal(&context, b"fixture")?;
    for wrong in [
        SecretContext::new(Uuid::from_bytes([2; 16]), "password")?,
        SecretContext::new(Uuid::from_bytes([1; 16]), "private_key")?,
    ] {
        assert!(matches!(
            cipher.open(&wrong, &sealed),
            Err(CryptoError::Authentication)
        ));
    }
    Ok(())
}
#[test]
fn ciphertext_nonce_tag_and_truncation_fail_without_plaintext() -> TestResult {
    let cipher = VaultCipher::generate()?;
    let context = context("password")?;
    let sealed = cipher.seal(&context, b"fixture")?;
    let mut modified = sealed.clone();
    modified.nonce[0] ^= 1;
    assert!(matches!(
        cipher.open(&context, &modified),
        Err(CryptoError::Authentication)
    ));
    for index in [0, sealed.ciphertext.len() - 1] {
        let mut modified = sealed.clone();
        modified.ciphertext[index] ^= 1;
        assert!(matches!(
            cipher.open(&context, &modified),
            Err(CryptoError::Authentication)
        ));
    }
    for len in [0, 1, 15, sealed.ciphertext.len() - 1] {
        let mut modified = sealed.clone();
        modified.ciphertext.truncate(len);
        assert!(matches!(
            cipher.open(&context, &modified),
            Err(CryptoError::Authentication)
        ));
    }
    Ok(())
}
#[test]
fn allocation_budget_is_checked_at_both_boundaries() -> TestResult {
    let cipher = VaultCipher::generate()?;
    let context = context("password")?;
    let input = vec![7; SECRET_BYTES_LIMIT];
    let sealed = cipher.seal(&context, &input)?;
    assert_eq!(cipher.open(&context, &sealed)?.expose(), input);
    assert!(matches!(
        cipher.seal(&context, &vec![0; SECRET_BYTES_LIMIT + 1]),
        Err(CryptoError::Size)
    ));
    let too_large = SealedSecret {
        nonce: [0; 24],
        ciphertext: vec![0; SECRET_BYTES_LIMIT + 17],
    };
    assert!(matches!(
        cipher.open(&context, &too_large),
        Err(CryptoError::Size)
    ));
    Ok(())
}
#[test]
fn lock_is_idempotent_and_plaintext_can_clear_early() -> TestResult {
    let mut cipher = VaultCipher::from_key_material(Zeroizing::new([7; 32]))?;
    let context = context("password")?;
    let sealed = cipher.seal(&context, b"fixture")?;
    let mut plaintext = cipher.open(&context, &sealed)?;
    plaintext.clear();
    assert!(plaintext.expose().is_empty());
    cipher.lock();
    cipher.lock();
    assert!(cipher.is_locked());
    assert!(matches!(
        cipher.seal(&context, b"fixture"),
        Err(CryptoError::Locked)
    ));
    assert!(matches!(
        cipher.open(&context, &sealed),
        Err(CryptoError::Locked)
    ));
    Ok(())
}
#[test]
fn imported_key_matches_published_xchacha_known_answer() -> TestResult {
    // draft-irtf-cfrg-xchacha appendix A.1; public vector, not real key material.
    let key = std::array::from_fn(|index| 0x80 + index as u8);
    let nonce = std::array::from_fn(|index| 0x40 + index as u8);
    let aad = [
        0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    ];
    let message = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let expected = [
        0xbd, 0x6d, 0x17, 0x9d, 0x3e, 0x83, 0xd4, 0x3b, 0x95, 0x76, 0x57, 0x94, 0x93, 0xc0, 0xe9,
        0x39, 0x57, 0x2a, 0x17, 0x00, 0x25, 0x2b, 0xfa, 0xcc, 0xbe, 0xd2, 0x90, 0x2c, 0x21, 0x39,
        0x6c, 0xbb, 0x73, 0x1c, 0x7f, 0x1b, 0x0b, 0x4a, 0xa6, 0x44, 0x0b, 0xf3, 0xa8, 0x2f, 0x4e,
        0xda, 0x7e, 0x39, 0xae, 0x64, 0xc6, 0x70, 0x8c, 0x54, 0xc2, 0x16, 0xcb, 0x96, 0xb7, 0x2e,
        0x12, 0x13, 0xb4, 0x52, 0x2f, 0x8c, 0x9b, 0xa4, 0x0d, 0xb5, 0xd9, 0x45, 0xb1, 0x1b, 0x69,
        0xb9, 0x82, 0xc1, 0xbb, 0x9e, 0x3f, 0x3f, 0xac, 0x2b, 0xc3, 0x69, 0x48, 0x8f, 0x76, 0xb2,
        0x38, 0x35, 0x65, 0xd3, 0xff, 0xf9, 0x21, 0xf9, 0x66, 0x4c, 0x97, 0x63, 0x7d, 0xa9, 0x76,
        0x88, 0x12, 0xf6, 0x15, 0xc6, 0x8b, 0x13, 0xb5, 0x2e, 0xc0, 0x87, 0x59, 0x24, 0xc1, 0xc7,
        0x98, 0x79, 0x47, 0xde, 0xaf, 0xd8, 0x78, 0x0a, 0xcf, 0x49,
    ];
    let holder = VaultCipher::from_key_material(Zeroizing::new(key))?;
    let cipher = holder.cipher.as_ref().ok_or(CryptoError::Locked)?;
    assert_eq!(
        cipher.encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: message,
                aad: &aad
            }
        )?,
        expected
    );
    Ok(())
}
#[test]
fn malformed_context_does_not_reach_crypto() {
    assert!(matches!(
        SecretContext::new(Uuid::nil(), "password"),
        Err(CryptoError::Context)
    ));
    for kind in ["", "PASSWORD", "a b", "a\0b", "中文", "_kind"] {
        assert!(matches!(context(kind), Err(CryptoError::Context)));
    }
    assert!(matches!(
        context(&"a".repeat(33)),
        Err(CryptoError::Context)
    ));
}
