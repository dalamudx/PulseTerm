//! Receive-only owned private-key bytes. Neither a format validator nor credential-use authority.
use crate::vault_crypto::{CryptoError, SecretBytes};
use serde::de::{Deserializer, Error, SeqAccess, Visitor};
use zeroize::Zeroizing;

/// Shared receive budget; protocol adapters still independently validate supported formats and KDFs.
pub const PRIVATE_KEY_BYTES_LIMIT: usize = 65_536;

/// No plaintext getter, Debug or serialization; partial and rejected owned buffers are zeroized.
/// The JSON transport and upstream parser may retain copies outside this object's ownership.
/// ```compile_fail
/// fn dump(input: pulse_store::vault_private_key_ingress::PrivateKeyIngress) { println!("{input:?}"); }
/// ```
/// ```compile_fail
/// fn export(input: pulse_store::vault_private_key_ingress::PrivateKeyIngress) { let _ = serde_json::to_string(&input); }
/// ```
pub struct PrivateKeyIngress(Zeroizing<Vec<u8>>);
impl PrivateKeyIngress {
    /// Transfer the exact UTF-8 bytes into the native credential job without another plaintext copy.
    /// Whitespace is retained; supported-key validation and page/source/epoch admission remain separate.
    pub fn into_credential(self) -> Result<SecretBytes, CryptoError> {
        std::str::from_utf8(&self.0).map_err(|_| CryptoError::Context)?;
        SecretBytes::from_zeroizing(self.0)
    }
}
impl<'de> serde::Deserialize<'de> for PrivateKeyIngress {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Input;
        impl<'de> Visitor<'de> for Input {
            type Value = PrivateKeyIngress;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("vault.private_key_invalid")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut bytes = Zeroizing::new(Vec::new());
                // Reserve once before input arrives, so growth cannot release unwiped secret copies.
                bytes
                    .try_reserve_exact(PRIVATE_KEY_BYTES_LIMIT)
                    .map_err(|_| A::Error::custom("vault.private_key_invalid"))?;
                while let Some(byte) = sequence
                    .next_element::<u8>()
                    .map_err(|_| A::Error::custom("vault.private_key_invalid"))?
                {
                    if bytes.len() == PRIVATE_KEY_BYTES_LIMIT {
                        return Err(A::Error::custom("vault.private_key_invalid"));
                    }
                    bytes.push(byte);
                }
                if bytes.is_empty() {
                    return Err(A::Error::custom("vault.private_key_invalid"));
                }
                Ok(PrivateKeyIngress(bytes))
            }
        }
        deserializer
            .deserialize_seq(Input)
            .map_err(|_| D::Error::custom("vault.private_key_invalid"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[test]
    fn private_key_ingress_moves_exact_bytes_without_copy_or_normalization() -> TestResult {
        let text = b"  -----BEGIN OPENSSH PRIVATE KEY-----\nfixture\n";
        let input: PrivateKeyIngress = serde_json::from_value(serde_json::json!(text.as_slice()))?;
        let ptr = input.0.as_ptr();
        let secret = input.into_credential()?;
        assert_eq!(secret.expose().as_ptr(), ptr);
        assert_eq!(secret.expose(), text);
        Ok(())
    }
    #[test]
    fn private_key_ingress_has_its_own_exact_budget_without_admitting_format_or_compute()
    -> TestResult {
        for length in [1, 4097, PRIVATE_KEY_BYTES_LIMIT] {
            let input: PrivateKeyIngress =
                serde_json::from_value(serde_json::json!(vec![b'a'; length]))?;
            let secret = input.into_credential()?;
            assert_eq!(secret.expose().len(), length);
        }
        let input: PrivateKeyIngress = serde_json::from_value(serde_json::json!([255]))?;
        assert!(input.into_credential().is_err());
        Ok(())
    }
    #[test]
    fn private_key_ingress_rejection_does_not_echo_malformed_material() -> TestResult {
        for value in [
            serde_json::json!([]),
            serde_json::json!(vec![1u8; PRIVATE_KEY_BYTES_LIMIT + 1]),
            serde_json::json!([42, "private-fixture-marker"]),
            serde_json::json!([42, 256]),
            serde_json::json!([42, -1]),
            serde_json::json!([42, true]),
            serde_json::json!("private-fixture-marker"),
            serde_json::json!({"key":"private-fixture-marker"}),
            serde_json::json!(null),
        ] {
            let error = match serde_json::from_value::<PrivateKeyIngress>(value) {
                Ok(_) => return Err("fixture.unexpected_ingress".into()),
                Err(error) => error,
            };
            assert_eq!(error.to_string(), "vault.private_key_invalid");
        }
        Ok(())
    }
}
