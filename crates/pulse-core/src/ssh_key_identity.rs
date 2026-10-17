//! Private-key identity metadata. Contains no key bytes, filesystem path or SSH permission.
use crate::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::InvalidSshIdentity,
};

/// Opaque same-Vault private-key and optional decryption-passphrase references.
/// Presence is derived from a captured Vault source and cannot authorize use.
/// ```compile_fail
/// fn log(identity: pulse_core::ssh_key_identity::SshPrivateKeyIdentity) {
///     println!("{identity:?}");
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct SshPrivateKeyIdentity {
    label: String,
    username: String,
    private_key: SecretRef,
    passphrase: Option<SecretRef>,
}
impl SshPrivateKeyIdentity {
    /// Validate metadata and exact credential roles before cloning any text.
    /// Both references must belong to the same Vault and use distinct credential IDs.
    /// An absent key or passphrase remains displayable; authentication must check its source.
    pub fn new(
        label: &str,
        username: &str,
        private_key: SecretRef,
        passphrase: Option<SecretRef>,
    ) -> Result<Self, InvalidSshIdentity> {
        if !crate::ssh_metadata::valid(label, username)
            || private_key.kind() != SecretKind::SshPrivateKey
            || passphrase.is_some_and(|value| {
                value.kind() != SecretKind::SshKeyPassphrase
                    || value.vault_id() != private_key.vault_id()
                    || value.id() == private_key.id()
            })
        {
            return Err(InvalidSshIdentity);
        }
        Ok(Self {
            label: label.to_owned(),
            username: username.to_owned(),
            private_key,
            passphrase,
        })
    }
    /// Bounded display metadata, never a credential identity or target authority.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Bounded SSH login metadata; never treat it as a shell command argument.
    pub fn username(&self) -> &str {
        &self.username
    }
    /// Metadata only; use requires exact source, original epoch and verified peer.
    pub fn private_key(&self) -> SecretRef {
        self.private_key
    }
    /// Metadata only; this reference can decrypt the selected key, never authenticate as a password.
    pub fn passphrase(&self) -> Option<SecretRef> {
        self.passphrase
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    type Result = std::result::Result<(), Box<dyn std::error::Error>>;
    fn reference(
        vault: u8,
        id: u8,
        kind: SecretKind,
        present: bool,
    ) -> std::result::Result<SecretRef, crate::secret_ref::InvalidSecretRef> {
        SecretRef::new(
            Uuid::from_bytes([vault; 16]),
            Uuid::from_bytes([id; 16]),
            kind,
            present,
        )
    }
    #[test]
    fn presence_is_display_metadata_with_separate_same_vault_roles() -> Result {
        for key_present in [false, true] {
            let key = reference(1, 2, SecretKind::SshPrivateKey, key_present)?;
            for passphrase in [
                None,
                Some(reference(1, 3, SecretKind::SshKeyPassphrase, false)?),
                Some(reference(1, 3, SecretKind::SshKeyPassphrase, true)?),
            ] {
                let identity =
                    SshPrivateKeyIdentity::new("开发密钥", "user@example.com", key, passphrase)?;
                assert_eq!(identity.label(), "开发密钥");
                assert_eq!(identity.username(), "user@example.com");
                assert_eq!(identity.private_key().has_value(), key_present);
                assert!(identity.passphrase() == passphrase);
            }
        }
        Ok(())
    }
    #[test]
    fn wrong_roles_foreign_vault_and_aliased_ids_are_rejected() -> Result {
        let key = reference(1, 2, SecretKind::SshPrivateKey, true)?;
        for invalid in [
            reference(1, 3, SecretKind::SshPassword, true)?,
            reference(1, 3, SecretKind::SshPrivateKey, true)?,
            reference(2, 3, SecretKind::SshKeyPassphrase, true)?,
            reference(1, 2, SecretKind::SshKeyPassphrase, false)?,
        ] {
            assert!(SshPrivateKeyIdentity::new("label", "user", key, Some(invalid)).is_err());
        }
        for kind in [SecretKind::SshPassword, SecretKind::SshKeyPassphrase] {
            assert!(
                SshPrivateKeyIdentity::new("label", "user", reference(1, 2, kind, true)?, None)
                    .is_err()
            );
        }
        assert!(crate::ssh_identity::SshPasswordIdentity::new("label", "user", key).is_err());
        Ok(())
    }
    #[test]
    fn login_and_display_limits_match_password_identity() -> Result {
        let key = reference(1, 2, SecretKind::SshPrivateKey, false)?;
        SshPrivateKeyIdentity::new(&"x".repeat(256), &"u".repeat(255), key, None)?;
        for (label, user) in [
            ("", "user"),
            ("name", ""),
            (" name", "user"),
            ("name ", "user"),
            ("a\nname", "user"),
            ("name", "-option"),
            ("name", "u name"),
            ("name", "u\n"),
            ("a\u{202e}b", "user"),
            ("name", "u\u{2066}ser"),
        ] {
            assert!(SshPrivateKeyIdentity::new(label, user, key, None).is_err());
        }
        assert!(SshPrivateKeyIdentity::new(&"x".repeat(257), "user", key, None).is_err());
        assert!(SshPrivateKeyIdentity::new("name", &"u".repeat(256), key, None).is_err());
        Ok(())
    }
}
