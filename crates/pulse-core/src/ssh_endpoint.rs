//! Bounded I/O-free SSH metadata for the first product host editor. No credential value.
use crate::ssh_target::SshTarget;
/// Static error code; does not contain addresses, labels or login names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("ssh.endpoint.invalid")]
pub struct InvalidSshEndpoint;
/// Metadata only. Constructor is neither host-key approval nor permission to connect.
/// IDs, identity references, grouping and revisions belong to the backend catalogue.
#[derive(Clone, PartialEq, Eq)]
pub struct SshEndpoint {
    label: String,
    target: SshTarget,
    username: String,
}
impl SshEndpoint {
    /// Check all field budgets before cloning. Callers may not bypass via Deserialize.
    /// Host normalization is shared with SSH decisions and application pins.
    pub fn new(
        label: &str,
        host: &str,
        port: u16,
        username: &str,
    ) -> Result<Self, InvalidSshEndpoint> {
        if !crate::ssh_metadata::valid(label, username) {
            return Err(InvalidSshEndpoint);
        }
        let target = SshTarget::new(host, port).map_err(|_| InvalidSshEndpoint)?;
        Ok(Self {
            label: label.to_owned(),
            target,
            username: username.to_owned(),
        })
    }
    /// Display label only, never used to derive filenames or execution commands.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Final normalized host/port, never supplied by a remote SSH server message.
    pub fn target(&self) -> &SshTarget {
        &self.target
    }
    /// A protocol string only. Do not interpolate it into a shell or include it in logs.
    pub fn username(&self) -> &str {
        &self.username
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_display_and_protocol_user_are_not_shell_commands() -> Result<(), InvalidSshEndpoint>
    {
        let e = SshEndpoint::new("开发主机", "EXAMPLE.COM", 22, "user@example.com")?;
        assert_eq!(e.label(), "开发主机");
        assert_eq!(e.target().host(), "example.com");
        assert_eq!(e.target().port(), 22);
        assert_eq!(e.username(), "user@example.com");
        assert!(
            SshEndpoint::new("其它显示名", "example.com", 22, "user@example.com")?.target()
                == e.target()
        );
        Ok(())
    }
    #[test]
    fn byte_budgets_and_invalid_fields_reject_before_clone() -> Result<(), InvalidSshEndpoint> {
        SshEndpoint::new(&"x".repeat(256), "host", 65535, &"u".repeat(255))?;
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
            assert!(SshEndpoint::new(l, "host", 22, u).is_err());
        }
        assert!(SshEndpoint::new(&"x".repeat(257), "host", 22, "user").is_err());
        assert!(SshEndpoint::new("name", "host", 22, &"u".repeat(256)).is_err());
        assert!(SshEndpoint::new("name", "[::1]", 22, "user").is_err());
        assert!(SshEndpoint::new("name", "host", 0, "user").is_err());
        Ok(())
    }
}
