//! Bounded OSC 133 hints from an untrusted terminal stream.
//! Hints cannot authenticate a shell, grant input, clear recovery risk, or execute code.
//! Only exact A/B/C and D[;0..255] forms terminated by BEL or full ST are accepted.
use serde::Serialize;
use std::collections::VecDeque;
/// Maximum undrained hints per terminal. Excess hints are counted and discarded.
pub const MAX_SHELL_MARKERS: usize = 128;
/// An advisory boundary, not proof of command execution or completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ShellMarker {
    /// Start of prompt text (OSC 133 A).
    PromptStart,
    /// End of prompt text/start of user input (OSC 133 B).
    PromptEnd,
    /// Start of command output (OSC 133 C).
    CommandStart,
    /// Advisory command end; a missing exit status remains unknown (OSC 133 D).
    CommandEnd {
        /// Untrusted shell exit status; never used for authorization.
        exit_status: Option<u8>,
    },
}
/// Ordered hints consumed since the previous drain, with explicit lossy overflow.
/// No line coordinates, byte boundary, revision, or parser synchronization is implied.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ShellMarkerBatch {
    /// At most MAX_SHELL_MARKERS hints, in stream order.
    pub markers: Vec<ShellMarker>,
    /// Discarded excess hints since the previous drain, saturating at u32::MAX.
    pub dropped: u32,
}
#[derive(Default)]
pub(crate) struct MarkerQueue {
    pending_st: Option<ShellMarker>,
    queue: VecDeque<ShellMarker>,
    dropped: u32,
}
impl MarkerQueue {
    pub fn observe(&mut self, byte: u8, body: Option<&[u8]>) {
        if let Some(marker) = self.pending_st.take()
            && byte == b'\\'
        {
            self.push(marker);
        }
        if let Some(marker) = body.and_then(parse) {
            if byte == 0x07 {
                self.push(marker);
            } else if byte == 0x1b {
                self.pending_st = Some(marker);
            }
        }
    }
    fn push(&mut self, marker: ShellMarker) {
        if self.queue.len() < MAX_SHELL_MARKERS {
            self.queue.push_back(marker);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }
    /// Restore a failed actor delivery before any further feed can interleave.
    pub(crate) fn restore(&mut self, batch: ShellMarkerBatch) {
        for marker in batch.markers {
            self.push(marker);
        }
        self.dropped = self.dropped.saturating_add(batch.dropped);
    }
    pub fn take(&mut self) -> ShellMarkerBatch {
        ShellMarkerBatch {
            markers: self.queue.drain(..).collect(),
            dropped: std::mem::take(&mut self.dropped),
        }
    }
}
fn parse(body: &[u8]) -> Option<ShellMarker> {
    match body {
        b"133;A" => Some(ShellMarker::PromptStart),
        b"133;B" => Some(ShellMarker::PromptEnd),
        b"133;C" => Some(ShellMarker::CommandStart),
        b"133;D" => Some(ShellMarker::CommandEnd { exit_status: None }),
        _ => {
            let status = body.strip_prefix(b"133;D;")?;
            if status.is_empty() || status.len() > 3 || !status.iter().all(u8::is_ascii_digit) {
                return None;
            }
            let value = status
                .iter()
                .fold(0u16, |value, byte| value * 10 + u16::from(byte - b'0'));
            Some(ShellMarker::CommandEnd {
                exit_status: Some(u8::try_from(value).ok()?),
            })
        }
    }
}
#[cfg(test)]
#[path = "shell_marker_tests.rs"]
mod tests;
