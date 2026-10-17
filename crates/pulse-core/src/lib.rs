//! I/O-free domain primitives shared by terminal and application layers.
#![forbid(unsafe_code)]
/// Bounded hybrid timestamp metadata; no clock reads or synchronization authority.
pub mod hlc;
/// Independent Host, Endpoint and SSH Profile metadata; no dial or credential-use authority.
pub mod host_catalogue;
/// Opaque credential metadata only; never a key or use permission.
pub mod secret_ref;
/// Explicit Agent public-key identity metadata; no socket or signing authority.
pub mod ssh_agent_identity;
/// Bounded saved SSH endpoint metadata, never an identity secret or dial permission.
pub mod ssh_endpoint;
/// Bounded SSH password identity metadata; no program/path or credential value.
pub mod ssh_identity;
/// Bounded SSH private-key identity metadata; separate key and optional passphrase references.
pub mod ssh_key_identity;
mod ssh_metadata;
/// I/O-free final SSH dial/trust target domain, not a network or execution permission.
pub mod ssh_target;

/// Validated terminal dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    columns: u16,
    rows: u16,
}

/// Invalid dimensions would permit an empty grid or excessive allocation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("terminal size must be within 1..=512 columns and 1..=256 rows")]
pub struct InvalidTermSize;

impl TermSize {
    /// Check size before a parser or PTY is allocated.
    pub fn new(columns: u16, rows: u16) -> Result<Self, InvalidTermSize> {
        if !(1..=512).contains(&columns) || !(1..=256).contains(&rows) {
            return Err(InvalidTermSize);
        }
        Ok(Self { columns, rows })
    }

    /// Width in cells.
    pub fn columns(self) -> u16 {
        self.columns
    }
    /// Height in cells.
    pub fn rows(self) -> u16 {
        self.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_are_bounded() {
        assert!(TermSize::new(1, 1).is_ok());
        assert!(TermSize::new(512, 256).is_ok());
        for (cols, rows) in [(0, 1), (1, 0), (513, 1), (1, 257)] {
            assert_eq!(TermSize::new(cols, rows), Err(InvalidTermSize));
        }
    }
}
