//! Initial password-identity metadata; agent/key modes are not approximated.
use crate::secret_ref::{SecretKind, SecretRef};
/// Static validation failure; cannot include username or credential metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("ssh.identity.invalid")]
pub struct InvalidSshIdentity;
/// Non-secret identity fields. Not a transport, DEK or permission to use its reference.
#[derive(Clone, PartialEq, Eq)]
pub struct SshPasswordIdentity {
    label: String,
    username: String,
    password: SecretRef,
}
impl SshPasswordIdentity {
    /// Apply the same bounded login/display rules as an endpoint before cloning strings.
    /// Missing references can be displayed but cannot authenticate a connection.
    pub fn new(
        label: &str,
        username: &str,
        password: SecretRef,
    ) -> Result<Self, InvalidSshIdentity> {
        if !crate::ssh_metadata::valid(label, username)
            || password.kind() != SecretKind::SshPassword
        {
            return Err(InvalidSshIdentity);
        }
        Ok(Self {
            label: label.to_owned(),
            username: username.to_owned(),
            password,
        })
    }
    /// Display label must not be used as credential identity or target authority.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Login metadata; never log this or use it as a command argument.
    pub fn username(&self) -> &str {
        &self.username
    }
    /// Metadata only; actual use independently validates target, source and epoch.
    pub fn password(&self) -> SecretRef {
        self.password
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    fn reference(
        kind: SecretKind,
        has: bool,
    ) -> Result<SecretRef, crate::secret_ref::InvalidSecretRef> {
        SecretRef::new(
            Uuid::from_bytes([1; 16]),
            Uuid::from_bytes([2; 16]),
            kind,
            has,
        )
    }
    #[test]
    fn password_reference_does_not_grant_presence_or_accept_key_role() -> TestResult {
        for has in [false, true] {
            let r = reference(SecretKind::SshPassword, has)?;
            let i = SshPasswordIdentity::new("开发身份", "user@example.com", r)?;
            assert_eq!(i.label(), "开发身份");
            assert_eq!(i.username(), "user@example.com");
            assert_eq!(i.password().has_value(), has);
        }
        assert!(
            SshPasswordIdentity::new(
                "label",
                "user",
                reference(SecretKind::SshKeyPassphrase, true)?
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn shared_byte_and_invisible_character_limits_remain_exact() -> TestResult {
        let r = reference(SecretKind::SshPassword, false)?;
        SshPasswordIdentity::new(&"x".repeat(256), &"u".repeat(255), r)?;
        for (l, u) in [
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
            assert!(SshPasswordIdentity::new(l, u, r).is_err());
        }
        assert!(SshPasswordIdentity::new(&"x".repeat(257), "user", r).is_err());
        assert!(SshPasswordIdentity::new("name", &"u".repeat(256), r).is_err());
        Ok(())
    }
}
