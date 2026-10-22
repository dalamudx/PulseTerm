//! Transport guard for the fixed native browser operation; canonical URL parsing belongs to callers.
/// Fixed system browser handoff errors, without URLs or native diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WebLinkError {
    /// The input is outside the bounded HTTP(S)-only transport contract.
    #[error("platform.web_link.invalid")]
    Invalid,
    /// The original application authorization was revoked before native dispatch.
    #[error("platform.web_link.denied")]
    Denied,
    /// System runtime initialization or handler dispatch failed.
    #[error("platform.web_link.failed")]
    Failed,
}

pub(crate) fn validate(url: &str) -> Result<(), WebLinkError> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or(WebLinkError::Invalid)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if url.len() > 4096
        || authority.is_empty()
        || authority.contains('@')
        || url.chars().any(|c| {
            c.is_control()
                || c.is_whitespace()
                || c == '\\'
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return Err(WebLinkError::Invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_transport_rejects_programs_files_protocols_and_embedded_arguments() {
        for url in [
            "",
            "cmd.exe",
            "--help",
            "file:///C:/a",
            "shell:AppsFolder",
            "javascript:alert(1)",
            "ftp://example.test/",
            "http:///file",
            "https://?q=x",
            "https://user@example.test/",
            "https://example.test/\0x",
            "https://example.test/\n",
            "https://example.test/ a",
            "https://example.test/\u{202e}a",
            "https://example.test\\a",
        ] {
            assert_eq!(validate(url), Err(WebLinkError::Invalid));
        }
        assert_eq!(
            validate(&format!("https://example.test/{}", "a".repeat(4096))),
            Err(WebLinkError::Invalid)
        );
    }
    #[test]
    fn native_transport_keeps_literal_http_targets_with_shell_metacharacters() {
        for url in [
            "https://example.test/",
            "http://[::1]:8080/a?q=x%20y#section",
            "https://xn--fsqu00a.test/%E4%B8%AD",
            "https://example.test/?a=1&b=$x;echo%20x",
        ] {
            assert_eq!(validate(url), Ok(()));
        }
        // These variants are produced only by the Windows native adapter.
        assert_ne!(WebLinkError::Denied, WebLinkError::Failed);
    }
}
