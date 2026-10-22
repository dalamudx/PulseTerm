//! Backend-only SSH host identity admission: no writes or implicit trust on first contact.
/// Local Agent discovery and one independently authorized selected-key authentication.
pub mod agent;
/// Bounded OpenSSH metadata parsing and explicit read-only platform sources; no execution authority.
pub mod config;
mod known_hosts;
#[cfg(unix)]
mod known_hosts_file;
#[cfg(feature = "managed-store")]
/// Optional strict adapter from untrusted storage text to verified SSH host pins.
pub mod managed_store;
/// Connection-bound host-key evidence; never credential or target-use authority.
pub mod peer;
pub use known_hosts::parse_system_known_hosts;
#[cfg(unix)]
pub use known_hosts_file::load_system_known_hosts_directory;
#[cfg(not(unix))]
/// Unsupported system-file read: never substitute an empty trust store.
pub fn load_system_known_hosts_directory(
    _: &std::path::Path,
    _: &HostTarget,
) -> Result<Vec<HostPin>, HostKeyError> {
    Err(HostKeyError::StoreUnavailable)
}
use russh::keys::PublicKey;
const MAX_PINS: usize = 128;

/// Canonical final dial target and TCP port (never a server-provided name).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostTarget {
    host: String,
    port: u16,
}
impl HostTarget {
    /// Reject ambiguous host syntax and invalid ports rather than weakening pin identity.
    pub fn new(host: &str, port: u16) -> Result<Self, HostKeyError> {
        let target = pulse_core::ssh_target::SshTarget::new(host, port)
            .map_err(|_| HostKeyError::InvalidTarget)?;
        Ok(Self {
            host: target.host().to_owned(),
            port: target.port(),
        })
    }
    /// Canonical hostname or unbracketed IP literal.
    pub fn host(&self) -> &str {
        &self.host
    }
    /// TCP port included in the trust identity.
    pub fn port(&self) -> u16 {
        self.port
    }
}

/// Backend-loaded pinned public key, without a Debug path that prints key material.
#[derive(Clone)]
pub struct HostPin {
    target: HostTarget,
    key: PublicKey,
}
impl HostPin {
    /// Construction is not approval. Only trusted storage loaders may populate a pin list.
    pub fn new(target: HostTarget, key: PublicKey) -> Self {
        Self { target, key }
    }
    /// Final dial identity; this is not a hostname asserted by the remote peer.
    pub fn target(&self) -> &HostTarget {
        &self.target
    }
    /// Public host key only; possession does not imply trust or authorization.
    pub fn public_key(&self) -> &PublicKey {
        &self.key
    }
}

/// Stable content-free refusal codes, never including hostname or key.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HostKeyError {
    /// Ambiguous final target.
    #[error("ssh.hostkey_invalid_target")]
    InvalidTarget,
    /// A source was unavailable or its bounded view was exceeded.
    #[error("ssh.hostkey_store_unavailable")]
    StoreUnavailable,
    /// No pin exists: authenticate only after a separate explicit approval flow.
    #[error("ssh.hostkey_unknown")]
    Unknown,
    /// A known target presented an untrusted key (including algorithm changes).
    #[error("ssh.hostkey_changed")]
    Changed,
}
fn check(
    target: &HostTarget,
    presented: &PublicKey,
    pins: &[HostPin],
) -> Result<Option<bool>, HostKeyError> {
    if pins.len() > MAX_PINS {
        return Err(HostKeyError::StoreUnavailable);
    }
    let mut known = false;
    let mut matched = false;
    for pin in pins {
        if pin.target == *target {
            known = true;
            matched |= pin.key == *presented;
        }
    }
    Ok(known.then_some(matched))
}
/// Verify **before** SSH authentication. Own records take precedence over system records.
/// `None` means read failure, not an empty store; a changed key cannot fall back to a lower
/// priority source. File parsing, explicit approval and russh connection wiring are separate.
/// Both slices are bounded, validated snapshots for the final dial target.
pub fn verify_host_key(
    target: &HostTarget,
    presented: &PublicKey,
    own: Option<&[HostPin]>,
    system: Option<&[HostPin]>,
) -> Result<(), HostKeyError> {
    let own = own.ok_or(HostKeyError::StoreUnavailable)?;
    let verdict = match check(target, presented, own)? {
        Some(value) => Some(value),
        None => check(
            target,
            presented,
            system.ok_or(HostKeyError::StoreUnavailable)?,
        )?,
    };
    match verdict {
        Some(true) => Ok(()),
        Some(false) => Err(HostKeyError::Changed),
        None => Err(HostKeyError::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, EcdsaCurve, PrivateKey};
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    fn key(algorithm: Algorithm) -> Result<PublicKey, Box<dyn std::error::Error>> {
        Ok(PrivateKey::random(&mut rand::rng(), algorithm)?
            .public_key()
            .clone())
    }
    #[test]
    fn own_precedence_changed_algorithm_and_explicit_unknown() -> TestResult {
        let target = HostTarget::new("SERVER.Example", 2222)?;
        let good = key(Algorithm::Ed25519)?;
        let bad = key(Algorithm::Ed25519)?;
        let other_algorithm = key(Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        })?;
        let own = [HostPin::new(target.clone(), good.clone())];
        let system = [HostPin::new(target.clone(), bad.clone())];
        assert_eq!(verify_host_key(&target, &good, Some(&own), None), Ok(()));
        for presented in [&bad, &other_algorithm] {
            assert_eq!(
                verify_host_key(&target, presented, Some(&own), Some(&system)),
                Err(HostKeyError::Changed)
            );
        }
        assert_eq!(
            verify_host_key(&target, &bad, Some(&[]), Some(&system)),
            Ok(())
        );
        for different in [
            HostTarget::new("server.example", 22)?,
            HostTarget::new("other.example", 2222)?,
        ] {
            assert_eq!(
                verify_host_key(&different, &good, Some(&own), Some(&system)),
                Err(HostKeyError::Unknown)
            );
        }
        assert_eq!(
            verify_host_key(&target, &good, None, Some(&system)),
            Err(HostKeyError::StoreUnavailable)
        );
        assert_eq!(
            verify_host_key(&target, &good, Some(&[]), None),
            Err(HostKeyError::StoreUnavailable)
        );
        Ok(())
    }
    #[test]
    fn canonical_targets_multi_pin_and_bounded_admission() -> TestResult {
        for host in [
            "",
            "[::1]",
            "-host",
            "host.",
            "u@host",
            "foo/bar",
            "*.example",
            "host\nline",
        ] {
            assert_eq!(HostTarget::new(host, 22), Err(HostKeyError::InvalidTarget));
        }
        assert_eq!(
            HostTarget::new("valid", 0),
            Err(HostKeyError::InvalidTarget)
        );
        assert_eq!(
            HostTarget::new("2001:0DB8::1", 22)?,
            HostTarget::new("2001:db8::1", 22)?
        );
        let target = HostTarget::new("server.example", 22)?;
        let good = key(Algorithm::Ed25519)?;
        let rotated = [
            HostPin::new(target.clone(), key(Algorithm::Ed25519)?),
            HostPin::new(target.clone(), good.clone()),
        ];
        assert_eq!(
            verify_host_key(&target, &good, Some(&rotated), None),
            Ok(())
        );
        let too_many = vec![HostPin::new(target.clone(), good.clone()); MAX_PINS + 1];
        assert_eq!(
            verify_host_key(&target, &good, Some(&too_many), None),
            Err(HostKeyError::StoreUnavailable)
        );
        Ok(())
    }
}
