//! Bounded, read-only OpenSSH known_hosts text parsing. No file access, home lookup or logging.
use crate::{HostKeyError, HostPin, HostTarget};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::PublicKey;
use sha1::Sha1;
const MAX_TEXT: usize = 256 * 1024;
const MAX_LINE: usize = 8192;
const MAX_MATCHES: usize = 128;

fn hashed_match(name: &str, entry: &str) -> Result<bool, HostKeyError> {
    let Some(rest) = entry.strip_prefix("|1|") else {
        return Err(HostKeyError::StoreUnavailable);
    };
    let (salt, digest) = rest.split_once('|').ok_or(HostKeyError::StoreUnavailable)?;
    if digest.contains('|') {
        return Err(HostKeyError::StoreUnavailable);
    }
    let salt = STANDARD
        .decode(salt)
        .map_err(|_| HostKeyError::StoreUnavailable)?;
    let digest = STANDARD
        .decode(digest)
        .map_err(|_| HostKeyError::StoreUnavailable)?;
    if salt.len() != 20 || digest.len() != 20 {
        return Err(HostKeyError::StoreUnavailable);
    }
    let mut mac =
        Hmac::<Sha1>::new_from_slice(&salt).map_err(|_| HostKeyError::StoreUnavailable)?;
    mac.update(name.as_bytes());
    Ok(mac.verify_slice(&digest).is_ok())
}

fn match_names(name: &str, entries: &str) -> Result<bool, HostKeyError> {
    if entries.is_empty() {
        return Err(HostKeyError::StoreUnavailable);
    }
    if entries.starts_with('|') {
        return hashed_match(name, entries);
    }
    // OpenSSH wildcard/negation/other hashed formats cannot silently become Unknown.
    if entries
        .bytes()
        .any(|b| matches!(b, b'*' | b'?' | b'!' | b'|' | b'\\'))
    {
        return Err(HostKeyError::StoreUnavailable);
    }
    Ok(entries.split(',').any(|item| item == name))
}

/// Parse an already-read, bounded system known_hosts snapshot for a final dial target.
/// Supports exact/comma-separated names, nonstandard `[host]:port`, OpenSSH `|1|` hashes,
/// and `@revoked`. Unsupported patterns/markers fail closed instead of falling through to TOFU.
/// This function performs no filesystem IO. A failed read must be passed as `None` to
/// `verify_host_key`, NEVER as an empty string or an empty list.
pub fn parse_system_known_hosts(
    target: &HostTarget,
    text: &str,
) -> Result<Vec<HostPin>, HostKeyError> {
    if text.len() > MAX_TEXT {
        return Err(HostKeyError::StoreUnavailable);
    }
    let name = if target.port() == 22 {
        target.host().to_owned()
    } else {
        format!("[{}]:{}", target.host(), target.port())
    };
    let mut found = Vec::new();
    for raw in text.lines() {
        if raw.len() > MAX_LINE || raw.contains('\0') {
            return Err(HostKeyError::StoreUnavailable);
        }
        let trimmed = raw.trim_ascii();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut words = trimmed.split_ascii_whitespace();
        let first = words.next().ok_or(HostKeyError::StoreUnavailable)?;
        let (marker, hosts) = if first.starts_with('@') {
            (
                Some(first),
                words.next().ok_or(HostKeyError::StoreUnavailable)?,
            )
        } else {
            (None, first)
        };
        let matched = match_names(&name, hosts)?;
        if !matched {
            continue;
        }
        if marker == Some("@revoked") {
            return Err(HostKeyError::Changed);
        }
        if marker.is_some() {
            return Err(HostKeyError::StoreUnavailable);
        }
        let kind = words.next().ok_or(HostKeyError::StoreUnavailable)?;
        let encoded = words.next().ok_or(HostKeyError::StoreUnavailable)?;
        if found.len() == MAX_MATCHES {
            return Err(HostKeyError::StoreUnavailable);
        }
        let key = PublicKey::from_openssh(&format!("{kind} {encoded}"))
            .map_err(|_| HostKeyError::StoreUnavailable)?;
        found.push(HostPin::new(target.clone(), key));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, PrivateKey};
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[test]
    fn exact_port_hashed_and_revoked_are_not_first_contact() -> TestResult {
        let target = HostTarget::new("host.example", 2222)?;
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?
            .public_key()
            .clone();
        let encoded = key.to_openssh()?.to_string();
        let line = format!("[host.example]:2222 {encoded} comment\n");
        let own: [HostPin; 0] = [];
        let loaded = parse_system_known_hosts(&target, &line)?;
        assert_eq!(
            crate::verify_host_key(&target, &key, Some(&own), Some(&loaded)),
            Ok(())
        );
        assert!(parse_system_known_hosts(&HostTarget::new("host.example", 22)?, &line)?.is_empty());
        let salt = [42u8; 20];
        let mut mac = Hmac::<Sha1>::new_from_slice(&salt)?;
        mac.update(b"[host.example]:2222");
        let hash = mac.finalize().into_bytes();
        let hashed = format!(
            "|1|{}|{} {encoded}\n",
            STANDARD.encode(salt),
            STANDARD.encode(hash)
        );
        let loaded = parse_system_known_hosts(&target, &hashed)?;
        assert_eq!(
            crate::verify_host_key(&target, &key, Some(&own), Some(&loaded)),
            Ok(())
        );
        assert!(
            parse_system_known_hosts(&HostTarget::new("other.example", 2222)?, &hashed)?.is_empty()
        );
        assert_eq!(
            parse_system_known_hosts(
                &target,
                &format!("{line}@revoked [host.example]:2222 {encoded}\n")
            )
            .err(),
            Some(HostKeyError::Changed)
        );
        Ok(())
    }
    #[test]
    fn openssh_generated_hashed_nonstandard_port_vector() -> TestResult {
        // Generated independently with `ssh-keygen -H -f` from the public test key.
        let host = HostTarget::new("host.example", 2222)?;
        let key = PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti",
        )?;
        let vector = "|1|JaNlcaAQ8MWFIKv4kWpbaiL3Yuc=|TDxwrLlZfB0vk3Z0uPYYaVvdRwk= ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti\n";
        let pins = parse_system_known_hosts(&host, vector)?;
        assert_eq!(
            crate::verify_host_key(&host, &key, Some(&[]), Some(&pins)),
            Ok(())
        );
        let wrong_port = HostTarget::new("host.example", 22)?;
        let no_pins = parse_system_known_hosts(&wrong_port, vector)?;
        assert_eq!(
            crate::verify_host_key(&wrong_port, &key, Some(&[]), Some(&no_pins)),
            Err(HostKeyError::Unknown)
        );
        Ok(())
    }
    #[test]
    fn corrupt_matching_lines_unsupported_syntax_and_size_fail_closed() -> TestResult {
        let target = HostTarget::new("host.example", 22)?;
        for text in [
            "host.example ssh-ed25519 !!!!",
            "host.example ssh-rsa AAAA",
            "host.example",
            "@cert-authority host.example ssh-ed25519 bad",
            "*.example ssh-ed25519 bad",
            "!host.example ssh-ed25519 bad",
            "|1|!!!!|! ssh-ed25519 bad",
            "host.example ssh-ed25519 bad\0",
        ] {
            assert_eq!(
                parse_system_known_hosts(&target, text).err(),
                Some(HostKeyError::StoreUnavailable)
            );
        }
        assert!(parse_system_known_hosts(&target, "# ignore\n")?.is_empty());
        assert_eq!(
            parse_system_known_hosts(&target, &"x".repeat(MAX_TEXT + 1)).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        assert_eq!(
            parse_system_known_hosts(&target, &format!("#{}", "x".repeat(MAX_LINE))).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        Ok(())
    }
}
