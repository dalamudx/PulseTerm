use crate::{vault_crypto::VaultCipher, vault_record::CredentialRecord};
use uuid::Uuid;
#[test]
fn rewriting_untrusted_header_cannot_change_vault_id_credential_or_kind_even_with_same_key()
-> Result<(), Box<dyn std::error::Error>> {
    let vault = Uuid::new_v4();
    let id = Uuid::new_v4();
    let key = VaultCipher::generate()?;
    let original = CredentialRecord::seal(vault, id, "ssh_password", &key, b"fixed-secret")?
        .encode_ciphertext()?;
    let another_vault = Uuid::new_v4();
    let mut swapped = original.clone();
    swapped[6..22].copy_from_slice(another_vault.as_bytes());
    let record = CredentialRecord::decode_ciphertext(&swapped, another_vault)?;
    assert!(
        record
            .open(another_vault, id, "ssh_password", &key)
            .is_err()
    );
    let another_id = Uuid::new_v4();
    let mut swapped = original.clone();
    swapped[22..38].copy_from_slice(another_id.as_bytes());
    let record = CredentialRecord::decode_ciphertext(&swapped, vault)?;
    assert!(
        record
            .open(vault, another_id, "ssh_password", &key)
            .is_err()
    );
    let mut swapped = original;
    swapped[39] = b't';
    let record = CredentialRecord::decode_ciphertext(&swapped, vault)?;
    assert!(record.open(vault, id, "tsh_password", &key).is_err());
    Ok(())
}
