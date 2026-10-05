//! Pure parsing for successful `wsl.exe --list --quiet` stdout snapshots.
//! Native process exit/status/timeout and Windows installation discovery remain separate.
use crate::{PtyCommand, PtyError};
use std::collections::BTreeSet;
use std::ffi::OsString;
/// Maximum bytes accepted from one finite WSL list process.
pub const MAX_WSL_LIST_BYTES: usize = 64 * 1024;
/// Maximum installed distribution records accepted in one snapshot.
pub const MAX_WSL_DISTRIBUTIONS: usize = 64;
/// Maximum UTF-16 code units in one literal distribution name.
pub const MAX_WSL_NAME_UNITS: usize = 256;
/// One exact name supplied by a trusted local WSL discovery adapter.
/// Name case and spaces are preserved; this is not a stable registry GUID.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WslDistribution {
    name: String,
}
impl std::fmt::Debug for WslDistribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WslDistribution")
            .field("name", &"<redacted>")
            .finish_non_exhaustive()
    }
}
impl WslDistribution {
    /// Exact discovery name, not a command or a rendered/escaped display label.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Construct literal `--distribution` argv for a separately authorized backend.
    /// No shell string/WSL process is executed here. Installation path is caller-owned.
    pub fn command(&self, program: impl Into<OsString>) -> Result<PtyCommand, PtyError> {
        let mut command = PtyCommand::new(program);
        command.args = vec!["--distribution".into(), self.name.clone().into()];
        command.validate()?;
        Ok(command)
    }
}
/// Non-sensitive parser errors: do not include discovery stdout or distribution names.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WslListError {
    /// Output, name or distribution count exceeded a finite bound.
    #[error("wsl.discovery_limit")]
    Limit,
    /// Odd bytes, wrong BOM, or unpaired UTF-16 surrogate.
    #[error("wsl.invalid_utf16le")]
    Encoding,
    /// Nonempty record contained control characters or was only whitespace.
    #[error("wsl.invalid_name")]
    InvalidName,
    /// Exact duplicate names do not constitute two installed distributions.
    #[error("wsl.duplicate_name")]
    Duplicate,
}
/// Decode only UTF-16LE, with optional leading LE BOM, LF/CRLF and empty separator lines.
/// Requires a successful native command receipt before calling. UTF-8/error text must
/// not be used as a fallback; this pure function cannot authenticate process output.
pub fn decode_wsl_list(bytes: &[u8]) -> Result<Vec<WslDistribution>, WslListError> {
    if bytes.len() > MAX_WSL_LIST_BYTES {
        return Err(WslListError::Limit);
    }
    if !bytes.len().is_multiple_of(2) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err(WslListError::Encoding);
    }
    let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    let text = String::from_utf16(&units).map_err(|_| WslListError::Encoding)?;
    let mut names = BTreeSet::new();
    let mut distributions = Vec::new();
    for line in text.split('\n') {
        let name = line.strip_suffix('\r').unwrap_or(line);
        if name.is_empty() {
            continue;
        }
        if distributions.len() >= MAX_WSL_DISTRIBUTIONS
            || name.encode_utf16().count() > MAX_WSL_NAME_UNITS
        {
            return Err(WslListError::Limit);
        }
        if name.chars().all(char::is_whitespace)
            || name.chars().any(|c| c.is_control() || c == '\u{feff}')
        {
            return Err(WslListError::InvalidName);
        }
        if !names.insert(name) {
            return Err(WslListError::Duplicate);
        }
        distributions.push(WslDistribution {
            name: name.to_owned(),
        });
    }
    Ok(distributions)
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }
    #[test]
    fn utf16le_bom_unicode_and_every_distribution_are_preserved() {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(le("Ubuntu\r\n自定义 中文🌍\r\nopenSUSE Tumbleweed\r\n"));
        let list = decode_wsl_list(&bytes).unwrap();
        assert_eq!(
            list.iter().map(WslDistribution::name).collect::<Vec<_>>(),
            ["Ubuntu", "自定义 中文🌍", "openSUSE Tumbleweed"]
        );
        assert_eq!(decode_wsl_list(&le("Ubuntu\nDebian")).unwrap().len(), 2);
        assert!(decode_wsl_list(&[]).unwrap().is_empty());
        assert!(decode_wsl_list(&[0xff, 0xfe]).unwrap().is_empty());
        assert!(decode_wsl_list(&le("\r\n\n")).unwrap().is_empty());
    }
    #[test]
    fn bad_bom_truncated_bytes_and_surrogates_are_rejected() {
        for bytes in [
            vec![0xff, 0xfe, 0x41],
            vec![0xfe, 0xff, 0, 0x41],
            vec![0, 0xd8],
            vec![0, 0xdc],
            vec![0, 0xd8, b'x', 0],
        ] {
            assert_eq!(decode_wsl_list(&bytes), Err(WslListError::Encoding));
        }
        let mut bad = le("Ubuntu\n");
        bad.extend([0, 0xd8]);
        assert_eq!(decode_wsl_list(&bad), Err(WslListError::Encoding));
    }
    #[test]
    fn invalid_or_duplicate_record_rejects_whole_snapshot() {
        for text in [
            "Ubuntu\nsecret\0name",
            "Ubuntu\n\tname",
            "Ubuntu\nname\rvalue",
            "Ubuntu\n \t ",
            "Ubuntu\n\u{feff}name",
        ] {
            let error = decode_wsl_list(&le(text)).unwrap_err();
            assert_eq!(error, WslListError::InvalidName);
            assert!(!error.to_string().contains("secret"));
        }
        assert_eq!(
            decode_wsl_list(&le("Ubuntu\nUbuntu")),
            Err(WslListError::Duplicate)
        );
        let names = decode_wsl_list(&le("Ubuntu\nubuntu\n name with spaces ")).unwrap();
        assert_eq!(names[2].name(), " name with spaces ");
    }
    #[test]
    fn literal_option_like_names_and_debug_do_not_interpolate_or_expose_names() {
        let list = decode_wsl_list(&le("--exec\nspace ; $(not-a-shell) 中文🌍")).unwrap();
        for distro in list {
            let command = distro.command("wsl.exe").unwrap();
            assert_eq!(command.program, "wsl.exe");
            assert_eq!(command.args[0], "--distribution");
            assert_eq!(command.args[1], distro.name());
            assert_eq!(command.args.len(), 2);
            assert!(!format!("{distro:?}").contains(distro.name()));
            assert!(!format!("{command:?}").contains(distro.name()));
            assert!(distro.command("invalid\0program").is_err());
        }
    }
    #[test]
    fn seeded_arbitrary_utf16_snapshots_never_bypass_record_limits_or_literal_argv() {
        let mut state = 0x57_534c_2026_u64;
        for case in 0..2000 {
            let mut units = Vec::new();
            for _ in 0..(case % 257) {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                units.push((state >> 32) as u16);
            }
            let bytes: Vec<u8> = units.into_iter().flat_map(u16::to_le_bytes).collect();
            if let Ok(list) = decode_wsl_list(&bytes) {
                assert!(list.len() <= MAX_WSL_DISTRIBUTIONS);
                let mut names = BTreeSet::new();
                for distro in list {
                    assert!(!distro.name().is_empty());
                    assert!(distro.name().encode_utf16().count() <= MAX_WSL_NAME_UNITS);
                    assert!(names.insert(distro.name().to_owned()));
                    let command = distro.command("wsl.exe").unwrap();
                    assert_eq!(
                        command.args,
                        vec![
                            OsString::from("--distribution"),
                            OsString::from(distro.name())
                        ]
                    );
                    assert!(command.validate().is_ok());
                }
            }
        }
    }

    #[test]
    fn names_count_and_utf16_budgets_have_exact_edges() {
        assert_eq!(
            decode_wsl_list(&vec![0; MAX_WSL_LIST_BYTES + 2]),
            Err(WslListError::Limit)
        );
        assert!(decode_wsl_list(&le(&"x".repeat(MAX_WSL_NAME_UNITS))).is_ok());
        assert_eq!(
            decode_wsl_list(&le(&"x".repeat(MAX_WSL_NAME_UNITS + 1))),
            Err(WslListError::Limit)
        );
        assert!(decode_wsl_list(&le(&"🌍".repeat(MAX_WSL_NAME_UNITS / 2))).is_ok());
        assert_eq!(
            decode_wsl_list(&le(&"🌍".repeat(MAX_WSL_NAME_UNITS / 2 + 1))),
            Err(WslListError::Limit)
        );
        let names = (0..MAX_WSL_DISTRIBUTIONS)
            .map(|n| format!("Distro{n}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            decode_wsl_list(&le(&names)).unwrap().len(),
            MAX_WSL_DISTRIBUTIONS
        );
        assert_eq!(
            decode_wsl_list(&le(&(names + "\nOverflow"))),
            Err(WslListError::Limit)
        );
    }
}
