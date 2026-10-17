//! Opaque credential references; no secret bytes or authorization.
use uuid::Uuid;
/// Reviewed initial SSH credential roles; future roles require explicit support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// Password for SSH user authentication, not a proxy/login-shell password.
    SshPassword,
    /// Private-key bytes stored only as Vault ciphertext, not a file path or use permission.
    SshPrivateKey,
    /// Passphrase used only to decrypt a backend-selected SSH private key.
    SshKeyPassphrase,
}
impl SecretKind {
    /// Exact AEAD kind spelling. Never infer a kind from a display label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SshPassword => "ssh_password",
            Self::SshPrivateKey => "ssh_private_key",
            Self::SshKeyPassphrase => "ssh_key_passphrase",
        }
    }
    /// Reject unsupported names rather than treating them as an SSH password.
    pub fn parse(value: &str) -> Result<Self, InvalidSecretRef> {
        match value {
            "ssh_password" => Ok(Self::SshPassword),
            "ssh_private_key" => Ok(Self::SshPrivateKey),
            "ssh_key_passphrase" => Ok(Self::SshKeyPassphrase),
            _ => Err(InvalidSecretRef),
        }
    }
}
/// Static error; no ID, label, ciphertext or plaintext is formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("secret.reference.invalid")]
pub struct InvalidSecretRef;
/// Display metadata only. Presence does not authenticate ciphertext or grant use.
/// No Debug/serde, so exposure needs a deliberate bounded DTO projection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SecretRef {
    vault_id: Uuid,
    id: Uuid,
    kind: SecretKind,
    has_value: bool,
}
impl SecretRef {
    /// Validate opaque IDs; the presence flag is only display data, not authority.
    pub fn new(
        vault_id: Uuid,
        id: Uuid,
        kind: SecretKind,
        has_value: bool,
    ) -> Result<Self, InvalidSecretRef> {
        if vault_id.is_nil() || id.is_nil() {
            return Err(InvalidSecretRef);
        }
        Ok(Self {
            vault_id,
            id,
            kind,
            has_value,
        })
    }
    /// Backend expected Vault identity; not a window or unlock ticket.
    pub fn vault_id(self) -> Uuid {
        self.vault_id
    }
    /// Stable opaque credential reference, not a path or permission.
    pub fn id(self) -> Uuid {
        self.id
    }
    /// Required role must still be checked by the target authentication adapter.
    pub fn kind(self) -> SecretKind {
        self.kind
    }
    /// A row exists in the metadata source; not proof of tag/password validity.
    pub fn has_value(self) -> bool {
        self.has_value
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_roles_and_non_nil_references() -> Result<(), InvalidSecretRef> {
        for kind in [
            SecretKind::SshPassword,
            SecretKind::SshPrivateKey,
            SecretKind::SshKeyPassphrase,
        ] {
            assert_eq!(SecretKind::parse(kind.as_str())?, kind);
            for has in [false, true] {
                let r = SecretRef::new(
                    Uuid::from_bytes([1; 16]),
                    Uuid::from_bytes([2; 16]),
                    kind,
                    has,
                )?;
                assert_eq!(r.has_value(), has);
                assert!(r.vault_id() != r.id());
            }
        }
        for name in [
            "password",
            "ssh_password ",
            "SSH_PASSWORD",
            "http_proxy_password",
            "",
        ] {
            assert!(SecretKind::parse(name).is_err());
        }
        assert!(
            SecretRef::new(
                Uuid::nil(),
                Uuid::from_bytes([1; 16]),
                SecretKind::SshPassword,
                true
            )
            .is_err()
        );
        assert!(
            SecretRef::new(
                Uuid::from_bytes([1; 16]),
                Uuid::nil(),
                SecretKind::SshPassword,
                false
            )
            .is_err()
        );
        Ok(())
    }
}
