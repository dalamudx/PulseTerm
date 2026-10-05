use super::*;
use crate::vault_crypto::{CryptoError, SecretContext};
// RFC 9106 section 5.3: public values, 32KiB/3 passes/4 lanes.
#[test]
fn rfc9106_argon2id_known_answer_uses_same_algorithm_and_version()
-> Result<(), Box<dyn std::error::Error>> {
    let params = argon2::ParamsBuilder::new()
        .m_cost(32)
        .t_cost(3)
        .p_cost(4)
        .data(argon2::AssociatedData::new(&[4; 12])?)
        .build()?;
    let mut work = Zeroizing::new(vec![Block::default(); params.block_count()]);
    let argon = Argon2::new_with_secret(&[3; 8], ALGORITHM, VERSION, params)?;
    let mut out = Zeroizing::new([0; 32]);
    argon.hash_password_into_with_memory(&[1; 32], &[2; 16], &mut out[..], &mut work[..])?;
    assert_eq!(
        *out,
        [
            0x0d, 0x64, 0x0d, 0xf5, 0x8d, 0x78, 0x76, 0x6c, 0x08, 0xc0, 0x37, 0xa3, 0x4a, 0x8b,
            0x53, 0xc9, 0xd0, 0x1e, 0xf0, 0x45, 0x2d, 0x75, 0xb6, 0x5e, 0xb5, 0x25, 0x20, 0xe9,
            0x6b, 0x01, 0xe6, 0x59
        ]
    );
    Ok(())
}
#[test]
fn fixed_profile_repeats_key_and_binds_password_salt_and_busy_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let salt = fresh_salt()?;
    assert_ne!(salt, fresh_salt()?);
    let context = SecretContext::new(uuid::Uuid::from_bytes([1; 16]), "wrapped_dek")?;
    let cipher = derive_validation_kek(b"fixed validation password", &salt)?;
    let sealed = cipher.seal(&context, &[0xab; 32])?;
    let repeated = derive_validation_kek(b"fixed validation password", &salt)?;
    assert_eq!(repeated.open(&context, &sealed)?.expose(), &[0xab; 32]);
    let other = derive_validation_kek(b"wrong validation password", &salt)?;
    assert!(matches!(
        other.open(&context, &sealed),
        Err(CryptoError::Authentication)
    ));
    let changed_salt = fresh_salt()?;
    let other = derive_validation_kek(b"fixed validation password", &changed_salt)?;
    assert!(matches!(
        other.open(&context, &sealed),
        Err(CryptoError::Authentication)
    ));
    let _held = JOB.try_lock().map_err(|_| KdfError::Busy)?;
    assert!(matches!(
        derive_validation_kek(b"valid", &salt),
        Err(KdfError::Busy)
    ));
    Ok(())
}
#[test]
fn bad_input_fails_before_job_admission_and_never_echoes_password() {
    for input in [Vec::new(), vec![b'x'; PASSWORD_BYTES_LIMIT + 1]] {
        let result = derive_validation_kek(&input, &[0; 16]);
        assert!(matches!(result, Err(KdfError::Input)));
    }
    assert_eq!(KdfError::Input.to_string(), "vault.kdf_invalid_input");
}
