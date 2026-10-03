//! Attachment v1: Begin -> History* -> Restore+ -> Commit -> (Data | Checkpoint | Marker)* -> End.
//! All messages use ONE ordered binary stream and the same sending credit window.
//! A new generation starts a new stream; sequence gaps are fatal, not recoverable
//! by accepting a later fragment. A caller must serialize control and PTY output
//! at the recorded boundary; this codec alone does not provide that integration.
#[cfg(test)]
#[path = "attach_tests.rs"]
mod tests;

use crate::{
    Frame, FrameError, FrameKind,
    flow::{CreditWindow, FlowError, Released},
};
/// Header inside an Attachment frame: version, part, reserved, generation, offset.
pub const ENVELOPE_LEN: usize = 20;
/// Largest fragment body, excluding its envelope.
pub const MAX_FRAGMENT: usize = 64 * 1024;
/// Largest recent-history replay retained by one reconnect transaction.
pub const MAX_HISTORY: usize = 4 * 1024 * 1024;
/// Largest complete restore in one transaction.
pub const MAX_RESTORE: usize = 1024 * 1024;
/// Wire parts, not standalone terminal frame kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Part {
    /// Fixed metadata; starts the transaction.
    Begin = 1,
    /// Recent history bytes at a section-local byte offset.
    History = 2,
    /// Restore bytes at a section-local byte offset.
    Restore = 3,
    /// Empty commit after exactly the declared bytes have arrived.
    Commit = 4,
    /// Live VT bytes starting at parsed_bytes from Begin.
    Data = 5,
    /// Ordered state checkpoint at the current live byte cursor (u64 revision).
    Checkpoint = 6,
    /// Genuine parser EOF; finish UTF-8 and render before ACK. No further frames.
    End = 7,
    /// Pure boundary marker: preserve parser, decoder and synchronized updates.
    Marker = 8,
    /// Atomic screen-only repair metadata at current live offset.
    RepairBegin = 9,
    /// Section-local offset, never original PTY bytes.
    RepairBytes = 10,
    /// Empty commit after the declared exact restore bytes.
    RepairCommit = 11,
}
/// Control/output boundary captured by the terminal owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundary {
    /// Validated character columns.
    pub columns: u16,
    /// Validated character rows.
    pub rows: u16,
    /// Backend state/control revision; never narrowed to a JS number.
    pub revision: u64,
    /// Original output byte count, NOT an ACK or re-encoded UTF-8 length.
    pub parsed_bytes: u64,
}
/// Invalid transaction, credits, or wire data. No partial replay is accepted.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AttachError {
    /// Malformed envelope, bad dimensions or invalid allocation/fragment limits.
    #[error("invalid attachment payload or limits")]
    Invalid,
    /// Wrong phase; live data cannot be sent before Commit.
    #[error("attachment transaction has not committed")]
    Phase,
    /// ACK belongs to another attachment.
    #[error("attachment generation mismatch")]
    Generation,
    /// Counter overflow requires starting a fresh attachment.
    #[error("attachment byte counter exhausted")]
    Overflow,
    /// Existing bounded sending-window rule.
    #[error(transparent)]
    Flow(#[from] FlowError),
    /// Existing binary frame rule.
    #[error(transparent)]
    Frame(#[from] FrameError),
}
/// Borrowed attachment fragment, whose boundary is supplied by the outer frame.
#[derive(Debug, PartialEq, Eq)]
pub struct Fragment<'a> {
    /// Wire part.
    pub part: Part,
    /// Nonzero attachment generation.
    pub generation: u64,
    /// Section byte offset; for live Data this is the backend parsed-byte cursor.
    pub offset: u64,
    /// Metadata, VT bytes or empty Commit.
    pub body: &'a [u8],
}
impl Fragment<'_> {
    /// Encode the envelope; the outer Frame adds ordering/credit sequence.
    pub fn encode(&self) -> Result<Vec<u8>, AttachError> {
        if self.generation == 0 || self.body.len() > MAX_FRAGMENT {
            return Err(AttachError::Invalid);
        }
        let mut out = vec![1, self.part as u8, 0, 0];
        out.extend_from_slice(&self.generation.to_le_bytes());
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.extend_from_slice(self.body);
        // Validate the same structural requirements as receivers.
        decode_fragment(&out)?;
        Ok(out)
    }
}
/// Decode without allocating and reject unknown versions/flags before using bytes.
pub fn decode_fragment(bytes: &[u8]) -> Result<Fragment<'_>, AttachError> {
    if bytes.len() < ENVELOPE_LEN
        || bytes.len() > ENVELOPE_LEN + MAX_FRAGMENT
        || bytes[0] != 1
        || bytes[2..4] != [0, 0]
    {
        return Err(AttachError::Invalid);
    }
    let part = match bytes[1] {
        1 => Part::Begin,
        2 => Part::History,
        3 => Part::Restore,
        4 => Part::Commit,
        5 => Part::Data,
        6 => Part::Checkpoint,
        7 => Part::End,
        8 => Part::Marker,
        9 => Part::RepairBegin,
        10 => Part::RepairBytes,
        11 => Part::RepairCommit,
        _ => return Err(AttachError::Invalid),
    };
    let read = |at| -> Result<u64, AttachError> {
        let data = bytes.get(at..at + 8).ok_or(AttachError::Invalid)?;
        Ok(u64::from_le_bytes(
            data.try_into().map_err(|_| AttachError::Invalid)?,
        ))
    };
    let generation = read(4)?;
    let offset = read(12)?;
    let body = &bytes[ENVELOPE_LEN..];
    if generation == 0
        || (matches!(part, Part::Begin | Part::Commit) && offset != 0)
        || (part == Part::Commit && !body.is_empty())
        || (matches!(part, Part::Checkpoint | Part::End | Part::Marker) && body.len() != 8)
        || (matches!(
            part,
            Part::History | Part::Restore | Part::Data | Part::RepairBytes
        ) && body.is_empty())
        || (part == Part::RepairBegin && body.len() != 12)
        || (part == Part::RepairCommit && !body.is_empty())
    {
        return Err(AttachError::Invalid);
    }
    if part == Part::Begin {
        decode_begin(body)?;
    }
    Ok(Fragment {
        part,
        generation,
        offset,
        body,
    })
}
/// Decode fixed 28-byte Begin metadata, validating totals before allocating.
pub fn decode_begin(body: &[u8]) -> Result<(Boundary, usize, usize), AttachError> {
    if body.len() != 28 {
        return Err(AttachError::Invalid);
    }
    let u16_at = |i| u16::from_le_bytes([body[i], body[i + 1]]);
    let u64_at = |i| {
        u64::from_le_bytes([
            body[i],
            body[i + 1],
            body[i + 2],
            body[i + 3],
            body[i + 4],
            body[i + 5],
            body[i + 6],
            body[i + 7],
        ])
    };
    let u32_at = |i| u32::from_le_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]) as usize;
    let boundary = Boundary {
        columns: u16_at(0),
        rows: u16_at(2),
        revision: u64_at(4),
        parsed_bytes: u64_at(12),
    };
    let history = u32_at(20);
    let restore = u32_at(24);
    if !(1..=512).contains(&boundary.columns)
        || !(1..=256).contains(&boundary.rows)
        || history > MAX_HISTORY
        || restore == 0
        || restore > MAX_RESTORE
    {
        return Err(AttachError::Invalid);
    }
    Ok((boundary, history, restore))
}
/// Ordered, credit-gated transaction generator. Owns at most 5 MiB of replay.
/// A NoCredit error leaves phase and offsets unchanged. A generated frame must
/// be delivered or the whole generation cancelled; never skip a reserved frame.
pub struct AttachmentSender {
    generation: u64,
    boundary: Boundary,
    history: Vec<u8>,
    restore: Vec<u8>,
    phase: Part,
    offset: usize,
    live_offset: u64,
    fragment: usize,
    window: CreditWindow,
    repair: Option<(Vec<u8>, usize, Part)>,
}
impl AttachmentSender {
    /// Freeze a transaction from an actor boundary; validate before emitting Begin.
    pub fn new(
        generation: u64,
        boundary: Boundary,
        history: Vec<u8>,
        restore: Vec<u8>,
        fragment: usize,
        max_frames: usize,
        max_bytes: usize,
    ) -> Result<Self, AttachError> {
        if generation == 0
            || history.len() > MAX_HISTORY
            || restore.is_empty()
            || restore.len() > MAX_RESTORE
            || fragment == 0
            || fragment > MAX_FRAGMENT
            || max_bytes < ENVELOPE_LEN + fragment.max(28)
        {
            return Err(AttachError::Invalid);
        }
        let value = Self {
            generation,
            boundary,
            history,
            restore,
            phase: Part::Begin,
            offset: 0,
            live_offset: boundary.parsed_bytes,
            fragment,
            window: CreditWindow::new(max_frames, max_bytes)?,
            repair: None,
        };
        decode_begin(&value.metadata())?;
        Ok(value)
    }
    fn metadata(&self) -> Vec<u8> {
        let mut body = Vec::with_capacity(28);
        body.extend_from_slice(&self.boundary.columns.to_le_bytes());
        body.extend_from_slice(&self.boundary.rows.to_le_bytes());
        body.extend_from_slice(&self.boundary.revision.to_le_bytes());
        body.extend_from_slice(&self.boundary.parsed_bytes.to_le_bytes());
        body.extend_from_slice(&(self.history.len() as u32).to_le_bytes());
        body.extend_from_slice(&(self.restore.len() as u32).to_le_bytes());
        body
    }
    /// Next bootstrap frame, or None once Commit has been emitted.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, AttachError> {
        if matches!(self.phase, Part::Data | Part::End) {
            return Ok(None);
        }
        let metadata = self.metadata();
        let body = match self.phase {
            Part::Begin => metadata.as_slice(),
            Part::History => {
                &self.history[self.offset..(self.offset + self.fragment).min(self.history.len())]
            }
            Part::Restore => {
                &self.restore[self.offset..(self.offset + self.fragment).min(self.restore.len())]
            }
            Part::Commit => &[],
            Part::Data | Part::End => return Ok(None),
            Part::Checkpoint
            | Part::Marker
            | Part::RepairBegin
            | Part::RepairBytes
            | Part::RepairCommit => return Err(AttachError::Phase),
        };
        let payload = Fragment {
            part: self.phase,
            generation: self.generation,
            offset: if self.phase == Part::Begin || self.phase == Part::Commit {
                0
            } else {
                self.offset as u64
            },
            body,
        }
        .encode()?;
        let sequence = self.window.record_send(payload.len())?;
        let frame = Frame {
            kind: FrameKind::Attachment,
            sequence,
            payload: &payload,
        }
        .encode()?;
        self.offset += body.len();
        match self.phase {
            Part::Begin => {
                self.phase = if self.history.is_empty() {
                    Part::Restore
                } else {
                    Part::History
                };
                self.offset = 0;
            }
            Part::History if self.offset == self.history.len() => {
                self.phase = Part::Restore;
                self.offset = 0;
                self.history = Vec::new();
            }
            Part::Restore if self.offset == self.restore.len() => {
                self.phase = Part::Commit;
                self.offset = 0;
                self.restore = Vec::new();
            }
            Part::Commit => {
                self.phase = Part::Data;
                self.offset = 0;
            }
            _ => {}
        }
        Ok(Some(frame))
    }
    /// Send post-boundary VT bytes only after Commit on this same ordered stream.
    pub fn data(&mut self, bytes: &[u8]) -> Result<Vec<u8>, AttachError> {
        if self.phase != Part::Data || self.repair.is_some() {
            return Err(AttachError::Phase);
        }
        if bytes.is_empty() || bytes.len() > self.fragment {
            return Err(AttachError::Invalid);
        }
        let next = self
            .live_offset
            .checked_add(bytes.len() as u64)
            .ok_or(AttachError::Overflow)?;
        let payload = Fragment {
            part: Part::Data,
            generation: self.generation,
            offset: self.live_offset,
            body: bytes,
        }
        .encode()?;
        let sequence = self.window.record_send(payload.len())?;
        let out = Frame {
            kind: FrameKind::Attachment,
            sequence,
            payload: &payload,
        }
        .encode()?;
        self.live_offset = next;
        Ok(out)
    }
    /// Start exact repair at this cursor, keeping the same credit sequence and byte offset.
    pub fn start_repair(&mut self, boundary: Boundary, bytes: Vec<u8>) -> Result<(), AttachError> {
        if self.phase != Part::Data || self.repair.is_some() {
            return Err(AttachError::Phase);
        }
        if boundary.columns != self.boundary.columns
            || boundary.rows != self.boundary.rows
            || boundary.parsed_bytes != self.live_offset
            || boundary.revision < self.boundary.revision
            || bytes.is_empty()
            || bytes.len() > MAX_RESTORE
        {
            return Err(AttachError::Invalid);
        }
        self.boundary.revision = boundary.revision;
        self.repair = Some((bytes, 0, Part::RepairBegin));
        Ok(())
    }
    /// Generate one credit-bound repair fragment; NoCredit leaves the transaction intact.
    pub fn next_repair(&mut self) -> Result<Option<Vec<u8>>, AttachError> {
        let Some((bytes, at, part)) = self.repair.as_ref() else {
            return Ok(None);
        };
        let mut metadata = Vec::new();
        let (offset, body) = match part {
            Part::RepairBegin => {
                metadata.extend_from_slice(&self.boundary.revision.to_le_bytes());
                metadata.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                (self.live_offset, metadata.as_slice())
            }
            Part::RepairBytes => (
                *at as u64,
                &bytes[*at..(*at + self.fragment).min(bytes.len())],
            ),
            Part::RepairCommit => (self.live_offset, &[][..]),
            _ => return Err(AttachError::Phase),
        };
        let payload = Fragment {
            part: *part,
            generation: self.generation,
            offset,
            body,
        }
        .encode()?;
        let sequence = self.window.record_send(payload.len())?;
        let wire = Frame {
            kind: FrameKind::Attachment,
            sequence,
            payload: &payload,
        }
        .encode()?;
        match part {
            Part::RepairBegin => {
                if let Some(r) = self.repair.as_mut() {
                    r.2 = Part::RepairBytes;
                }
            }
            Part::RepairBytes => {
                let len = body.len();
                if let Some(r) = self.repair.as_mut() {
                    r.1 += len;
                    if r.1 == r.0.len() {
                        r.2 = Part::RepairCommit;
                    }
                }
            }
            Part::RepairCommit => self.repair = None,
            _ => return Err(AttachError::Phase),
        }
        Ok(Some(wire))
    }
    /// State advanced without new raw bytes (e.g. synchronized-update expiry).
    /// Same stream/credits as Data. Wrong cursor or older revision is rejected;
    /// a NoCredit error leaves all state unchanged. Receiver must process before ACK.
    pub fn checkpoint(&mut self, boundary: Boundary) -> Result<Vec<u8>, AttachError> {
        self.state_frame(boundary, Part::Checkpoint)
    }
    /// A read-only boundary at the current cursor; no VT or sync-flush semantics.
    pub fn marker(&mut self, boundary: Boundary) -> Result<Vec<u8>, AttachError> {
        self.state_frame(boundary, Part::Marker)
    }
    /// End at the exact final byte cursor. NoCredit leaves phase/state unchanged.
    pub fn finish(&mut self, boundary: Boundary) -> Result<Vec<u8>, AttachError> {
        let frame = self.state_frame(boundary, Part::End)?;
        self.phase = Part::End;
        Ok(frame)
    }
    fn state_frame(&mut self, boundary: Boundary, part: Part) -> Result<Vec<u8>, AttachError> {
        if self.phase != Part::Data || self.repair.is_some() {
            return Err(AttachError::Phase);
        }
        if boundary.columns != self.boundary.columns
            || boundary.rows != self.boundary.rows
            || boundary.parsed_bytes != self.live_offset
            || boundary.revision < self.boundary.revision
        {
            return Err(AttachError::Invalid);
        }
        let body = boundary.revision.to_le_bytes();
        let payload = Fragment {
            part,
            generation: self.generation,
            offset: self.live_offset,
            body: &body,
        }
        .encode()?;
        let sequence = self.window.record_send(payload.len())?;
        let frame = Frame {
            kind: FrameKind::Attachment,
            sequence,
            payload: &payload,
        }
        .encode()?;
        self.boundary.revision = boundary.revision;
        Ok(frame)
    }
    /// Release credit only for this generation and an outstanding sequence.
    pub fn acknowledge(&mut self, generation: u64, sequence: u32) -> Result<Released, AttachError> {
        if generation != self.generation {
            return Err(AttachError::Generation);
        }
        Ok(self.window.acknowledge(sequence)?)
    }
    /// Envelope + body bytes currently in flight; outer six-byte frame header excluded.
    pub fn outstanding_bytes(&self) -> usize {
        self.window.outstanding_bytes()
    }
}
