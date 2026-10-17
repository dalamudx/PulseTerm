//! Selected Agent public-key metadata. No socket path, key bytes, sign or authentication authority.
use crate::ssh_identity::InvalidSshIdentity;
/// Reusable explicit Agent identity; an unavailable key is not an invitation to try another key.
/// ```compile_fail
/// fn log(identity: pulse_core::ssh_agent_identity::SshAgentIdentity) { println!("{identity:?}"); }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct SshAgentIdentity {
    label: String,
    username: String,
    algorithm: String,
    fingerprint: String,
}
impl SshAgentIdentity {
    /// Validate exact supported key algorithm and canonical unpadded SHA256 public-key fingerprint.
    /// Metadata is backend-discovered; this constructor alone grants no signing or socket access.
    pub fn new(
        label: &str,
        username: &str,
        algorithm: &str,
        fingerprint: &str,
    ) -> Result<Self, InvalidSshIdentity> {
        let Some(hash) = fingerprint.strip_prefix("SHA256:") else {
            return Err(InvalidSshIdentity);
        };
        if !crate::ssh_metadata::valid(label, username)
            || !matches!(
                algorithm,
                "ssh-ed25519"
                    | "ssh-rsa"
                    | "ecdsa-sha2-nistp256"
                    | "ecdsa-sha2-nistp384"
                    | "ecdsa-sha2-nistp521"
                    | "sk-ssh-ed25519@openssh.com"
                    | "sk-ecdsa-sha2-nistp256@openssh.com"
            )
            || hash.len() != 43
            || !hash
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
            || !matches!(
                hash.as_bytes().last(),
                Some(
                    b'A' | b'E'
                        | b'I'
                        | b'M'
                        | b'Q'
                        | b'U'
                        | b'Y'
                        | b'c'
                        | b'g'
                        | b'k'
                        | b'o'
                        | b's'
                        | b'w'
                        | b'0'
                        | b'4'
                        | b'8'
                )
            )
        {
            return Err(InvalidSshIdentity);
        }
        Ok(Self {
            label: label.into(),
            username: username.into(),
            algorithm: algorithm.into(),
            fingerprint: fingerprint.into(),
        })
    }
    /// Bounded display label, never interpreted as code.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Exact saved login username.
    pub fn username(&self) -> &str {
        &self.username
    }
    /// Exact public-key algorithm, not a negotiated host-key policy.
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
    /// Canonical public-key SHA256 fingerprint, not permission to sign a payload.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_agent_metadata_rejects_aliases_paths_unknown_algorithms_and_invalid_logins() {
        let fingerprint = format!("SHA256:{}", "A".repeat(43));
        let identity = SshAgentIdentity::new("Selected key", "user", "ssh-ed25519", &fingerprint);
        assert!(identity.is_ok());
        for algorithm in [
            "",
            "SSH-ED25519",
            "ssh-dss",
            "ssh-ed25519-cert-v01@openssh.com",
            "/data/agent.sock",
        ] {
            assert!(
                SshAgentIdentity::new("Selected key", "user", algorithm, &fingerprint).is_err()
            );
        }
        for bad in [
            format!("SHA256:{}B", "A".repeat(42)),
            format!("SHA256:{}=", "A".repeat(43)),
            format!("SHA256:{}", "A".repeat(42)),
            format!("sha256:{}", "A".repeat(43)),
            format!("SHA256:{}\u{202e}", "A".repeat(40)),
        ] {
            assert!(SshAgentIdentity::new("Selected key", "user", "ssh-ed25519", &bad).is_err());
        }
        for username in ["", "-option", "user name", "user\n", "u\u{2066}ser"] {
            assert!(
                SshAgentIdentity::new("Selected key", username, "ssh-ed25519", &fingerprint)
                    .is_err()
            );
        }
    }
}
