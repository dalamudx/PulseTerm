//! Reconciliation hints and a versioned visual digest. Hints are conservative,
//! NOT a second parser nor proof of divergence. Digest covers visible cells,
//! cursor and active buffer only; not palette rendering, hidden/parser/history
//! state, hyperlinks or all style flags. It is NOT a cryptographic authenticator.
use crate::{ObservedScreen, TerminalProbe};
/// Tab/wrap semantics may differ.
pub const TAB: u32 = 1;
/// IL/DL may reposition cursor differently.
pub const LINES: u32 = 2;
/// Wide-cell editing semantics may differ.
pub const CELLS: u32 = 4;
/// DECRC saved mode/cursor semantics may differ.
pub const CURSOR: u32 = 8;
/// Private mode, alternate-buffer and origin changes may differ.
pub const MODE: u32 = 16;
/// Resize/reflow semantics may differ.
pub const GEOMETRY: u32 = 32;
/// REP is expanded into input by VTE and requires boundary-level observation.
pub const REP: u32 = 64;
/// Diagnostic algorithm version; both ends serialize exactly the same projection.
pub const DIGEST_VERSION: u8 = 1;
/// FNV-1a64 canonical UTF-8, little-endian u32 lengths/dimensions/coordinates.
/// Non-security checksum; compare only at the same generation/actor boundary.
pub fn screen_digest(screen: &ObservedScreen) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    let mut bytes = |data: &[u8]| {
        for byte in data {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    };
    bytes(&[DIGEST_VERSION]);
    bytes(&(screen.cells.first().map_or(0, Vec::len) as u32).to_le_bytes());
    bytes(&(screen.cells.len() as u32).to_le_bytes());
    bytes(&(screen.cursor.0 as u32).to_le_bytes());
    bytes(&(screen.cursor.1 as u32).to_le_bytes());
    bytes(&[u8::from(screen.alternate)]);
    for row in &screen.cells {
        for cell in row {
            for text in [&cell.text, &cell.foreground, &cell.background] {
                bytes(&(text.len() as u32).to_le_bytes());
                bytes(text.as_bytes());
            }
            bytes(&[
                cell.width,
                u8::from(cell.bold),
                u8::from(cell.italic),
                u8::from(cell.underline),
                u8::from(cell.inverse),
            ]);
        }
    }
    format!("{hash:016x}")
}
impl TerminalProbe {
    /// Pending known-risk categories, not evidence a renderer actually diverged.
    pub fn divergence_risk(&self) -> u32 {
        self.side.risk | if self.ground.rep_seen { REP } else { 0 }
    }
    /// Visible projection digest; allocation/latency must be measured on target UI.
    pub fn screen_digest(&self) -> String {
        screen_digest(&self.screen())
    }
}
