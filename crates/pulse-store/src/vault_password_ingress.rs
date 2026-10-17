//! Receive-only owned input, not a credential read/serialization/authorization surface.
use crate::vault_kdf::{PASSWORD_BYTES_LIMIT, validate_password};
use crate::vault_unlock::PasswordInput;
use serde::de::{Deserializer, Error, SeqAccess, Visitor};
use zeroize::Zeroizing;
/// Explicit byte-array ingress; partial/rejected buffers are zeroized. Upstream JS/JSON copies remain caller-owned.
/// Not Debug/Clone/Serialize, and not permission to schedule KDF or use a secret.
/// ```compile_fail
/// fn dump(p:pulse_store::vault_password_ingress::PasswordIngress){println!("{p:?}");}
/// ```
/// ```compile_fail
/// fn export(p:pulse_store::vault_password_ingress::PasswordIngress){let _=serde_json::to_string(&p);}
/// ```
pub struct PasswordIngress(PasswordInput);
impl PasswordIngress {
    /// Development master-password creation floor: twelve Unicode scalar values, no trim/normalization.
    /// Unlocking existing envelopes uses exact bytes and does not apply this new-password rule.
    pub fn validate_creation_floor(&self) -> Result<(), crate::vault_kdf::KdfError> {
        if std::str::from_utf8(&self.0.bytes).is_ok_and(|value| value.chars().count() >= 12) {
            Ok(())
        } else {
            Err(crate::vault_kdf::KdfError::Input)
        }
    }
    /// Move receive-only UTF-8 credential input into owned zeroizing bytes; no master creation floor.
    /// This does not authorize writing, reading, unlock or SSH credential use.
    pub fn into_credential(
        self,
    ) -> Result<crate::vault_crypto::SecretBytes, crate::vault_crypto::CryptoError> {
        std::str::from_utf8(&self.0.bytes)
            .map_err(|_| crate::vault_crypto::CryptoError::Context)?;
        crate::vault_crypto::SecretBytes::from_zeroizing(self.0.bytes)
    }
    /// Move into native-owned job input without any plaintext getter or copy.
    pub fn into_password(self) -> PasswordInput {
        self.0
    }
}
impl<'de> serde::Deserialize<'de> for PasswordIngress {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Input;
        impl<'de> Visitor<'de> for Input {
            type Value = PasswordIngress;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("vault.password_invalid")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut bytes = Zeroizing::new(Vec::new());
                // Allocate once before secrets arrive; growth must not free unwiped copies.
                bytes
                    .try_reserve_exact(PASSWORD_BYTES_LIMIT)
                    .map_err(|_| A::Error::custom("vault.password_invalid"))?;
                while let Some(byte) = sequence
                    .next_element::<u8>()
                    .map_err(|_| A::Error::custom("vault.password_invalid"))?
                {
                    if bytes.len() == PASSWORD_BYTES_LIMIT {
                        return Err(A::Error::custom("vault.password_invalid"));
                    }
                    bytes.push(byte);
                }
                validate_password(&bytes)
                    .map_err(|_| A::Error::custom("vault.password_invalid"))?;
                Ok(PasswordIngress(PasswordInput { bytes }))
            }
        }
        deserializer
            .deserialize_any(Input)
            .map_err(|_| D::Error::custom("vault.password_invalid"))
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn credential_ingress_moves_original_allocation_without_master_floor_or_utf8_fallback()
    -> Result<(), Box<dyn std::error::Error>> {
        let input: PasswordIngress = serde_json::from_value(serde_json::json!("短".as_bytes()))?;
        let ptr = input.0.bytes.as_ptr();
        let owned = input.into_credential()?;
        assert_eq!(ptr, owned.expose().as_ptr());
        assert_eq!(owned.expose(), "短".as_bytes());
        let input: PasswordIngress = serde_json::from_value(serde_json::json!([255]))?;
        assert!(input.into_credential().is_err());
        assert!(serde_json::from_value::<PasswordIngress>(serde_json::json!([])).is_err());
        Ok(())
    }

    use super::*;
    #[test]
    fn creation_floor_is_exact_unicode_without_mutating_existing_unlock_input()
    -> Result<(), Box<dyn std::error::Error>> {
        for text in ["abcdefghijk", "short"] {
            let input: PasswordIngress =
                serde_json::from_value(serde_json::json!(text.as_bytes()))?;
            assert!(input.validate_creation_floor().is_err());
            assert_eq!(&*input.into_password().bytes, text.as_bytes());
        }
        for text in [
            "abcdefghijkl",
            "一二三四五六七八九十甲乙",
            "  exact padding  ",
        ] {
            let input: PasswordIngress =
                serde_json::from_value(serde_json::json!(text.as_bytes()))?;
            input.validate_creation_floor()?;
            assert_eq!(&*input.into_password().bytes, text.as_bytes());
        }
        let input: PasswordIngress = serde_json::from_value(serde_json::json!(vec![255u8; 12]))?;
        assert!(input.validate_creation_floor().is_err());
        Ok(())
    }

    #[test]
    fn exact_owned_budget_parse_does_not_admit_compute_or_echo_malformed_material()
    -> Result<(), Box<dyn std::error::Error>> {
        for length in [1, 12, PASSWORD_BYTES_LIMIT] {
            let input: PasswordIngress =
                serde_json::from_value(serde_json::json!(vec![42u8; length]))?;
            let owned = input.into_password();
            assert_eq!(owned.bytes.len(), length);
            assert!(owned.bytes.iter().all(|value| *value == 42));
        }
        for value in [
            serde_json::json!([]),
            serde_json::json!(vec![1u8; PASSWORD_BYTES_LIMIT + 1]),
            serde_json::json!([42, "private-validation-marker"]),
            serde_json::json!([42, 256]),
            serde_json::json!([42, -1]),
            serde_json::json!([42, true]),
            serde_json::json!("private-validation-marker"),
            serde_json::json!({"password":"private-validation-marker"}),
            serde_json::json!(null),
        ] {
            let result = serde_json::from_value::<PasswordIngress>(value);
            let error = match result {
                Ok(_) => return Err("fixture.unexpected_password".into()),
                Err(error) => error,
            };
            assert_eq!(error.to_string(), "vault.password_invalid");
        }
        Ok(())
    }
}
