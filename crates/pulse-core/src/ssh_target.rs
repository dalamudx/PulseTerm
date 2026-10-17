//! Pure final-dial identity; preserves the existing SSH/store canonicalization policy.
use std::net::IpAddr;
/// Invalid or ambiguous target. Never includes caller-supplied text.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[error("ssh.target.invalid")]
pub struct InvalidSshTarget;
/// Canonical DNS ASCII or unbracketed IP literal with a nonzero TCP port.
/// Does not resolve DNS, authorize connections, import aliases or approve keys.
#[derive(Clone, PartialEq, Eq)]
pub struct SshTarget {
    host: String,
    port: u16,
}
impl SshTarget {
    /// Reject URI/userinfo/wildcard/trailing-dot/zone syntax rather than approximate it.
    pub fn new(host: &str, port: u16) -> Result<Self, InvalidSshTarget> {
        if port == 0 || host.is_empty() || host.len() > 253 || !host.is_ascii() {
            return Err(InvalidSshTarget);
        }
        let host = if let Ok(ip) = host.parse::<IpAddr>() {
            ip.to_string()
        } else {
            if host.contains(':')
                || host.split('.').any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || label.starts_with('-')
                        || label.ends_with('-')
                        || !label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return Err(InvalidSshTarget);
            }
            host.to_ascii_lowercase()
        };
        Ok(Self { host, port })
    }
    /// Canonical host. Do not include identifiers in public diagnostics.
    pub fn host(&self) -> &str {
        &self.host
    }
    /// TCP port is part of trust identity, not a display default.
    pub fn port(&self) -> u16 {
        self.port
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_matches_existing_dns_and_ip_rules() -> Result<(), InvalidSshTarget> {
        for (raw, expected) in [
            ("Example.COM", "example.com"),
            ("localhost", "localhost"),
            ("192.0.2.5", "192.0.2.5"),
            ("2001:0DB8:0000:0000:0000:0000:0000:0001", "2001:db8::1"),
            ("::1", "::1"),
        ] {
            let target = SshTarget::new(raw, 22)?;
            assert_eq!(target.host(), expected);
            assert_eq!(target.port(), 22);
        }
        assert!(SshTarget::new("example.com", 22)? != SshTarget::new("example.com", 2222)?);
        Ok(())
    }
    #[test]
    fn ambiguous_or_external_resource_syntax_fails_without_logging_identifiers() {
        for raw in [
            "",
            " user.example",
            "example.com ",
            "example.com.",
            "a..b",
            "-host",
            "host-",
            "*.example.com",
            "[::1]",
            "fe80::1%eth0",
            "user@host",
            "ssh://example.com",
            "host/path",
            "host:22",
            "主机.example",
            "host\nname",
            "host\0name",
        ] {
            assert!(SshTarget::new(raw, 22).is_err());
        }
        assert!(SshTarget::new("localhost", 0).is_err());
        assert!(SshTarget::new(&"a".repeat(64), 22).is_err());
    }
    #[test]
    fn domain_boundary_exactly_253_ascii_bytes_and_valid_label_sizes()
    -> Result<(), InvalidSshTarget> {
        let valid = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(valid.len(), 253);
        SshTarget::new(&valid, 65535)?;
        assert!(SshTarget::new(&format!("{valid}x"), 22).is_err());
        Ok(())
    }
}
