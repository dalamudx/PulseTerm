//! Explicit v2 transactions: source bytes are never inferred from UTF-8 length.
use crate::attach::{AttachError, MAX_FRAGMENT, MAX_HISTORY, MAX_RESTORE, Part};
use crate::{
    Frame, FrameKind,
    flow::{CreditWindow, Released},
};
/// v2 envelope: version/part/flags, generation, section/display offset, source boundary.
pub const ENVELOPE_LEN: usize = 28;
/// Atomic source/display boundary. Source is transport data consumed, not ACK credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundary {
    /// Character columns.
    pub columns: u16,
    /// Character rows.
    pub rows: u16,
    /// State revision.
    pub revision: u64,
    /// Consumed source bytes, including pending decoder input.
    pub source_bytes: u64,
    /// Normalized bytes submitted to the display parser.
    pub utf8_bytes: u64,
}
/// Borrowed v2 fragment. Offset is section-local or normalized display offset.
#[derive(Debug, PartialEq, Eq)]
pub struct Fragment<'a> {
    /// Same ordered part IDs as v1, different version and metadata layout.
    pub part: Part,
    /// Nonzero binding generation.
    pub generation: u64,
    /// Section offset or UTF-8 offset, never source offset.
    pub offset: u64,
    /// Source boundary at completion of this fragment; no per-byte mapping implied.
    pub source_bytes: u64,
    /// Metadata or display bytes.
    pub body: &'a [u8],
}
fn u64_at(bytes: &[u8], at: usize) -> Result<u64, AttachError> {
    Ok(u64::from_le_bytes(
        bytes
            .get(at..at + 8)
            .ok_or(AttachError::Invalid)?
            .try_into()
            .map_err(|_| AttachError::Invalid)?,
    ))
}
impl Fragment<'_> {
    /// Encode validated metadata without silently emitting a v1 envelope.
    pub fn encode(&self) -> Result<Vec<u8>, AttachError> {
        if self.generation == 0 || self.body.len() > MAX_FRAGMENT {
            return Err(AttachError::Invalid);
        }
        let mut out = vec![2, self.part as u8, 0, 0];
        out.extend_from_slice(&self.generation.to_le_bytes());
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.extend_from_slice(&self.source_bytes.to_le_bytes());
        out.extend_from_slice(self.body);
        decode_fragment(&out)?;
        Ok(out)
    }
}
/// Reject v1/unknown versions and invalid metadata before allocating replay buffers.
pub fn decode_fragment(bytes: &[u8]) -> Result<Fragment<'_>, AttachError> {
    if bytes.len() < ENVELOPE_LEN
        || bytes.len() > ENVELOPE_LEN + MAX_FRAGMENT
        || bytes[0] != 2
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
    let generation = u64_at(bytes, 4)?;
    let offset = u64_at(bytes, 12)?;
    let source_bytes = u64_at(bytes, 20)?;
    let body = &bytes[ENVELOPE_LEN..];
    if generation == 0
        || (matches!(part, Part::Begin | Part::Commit) && offset != 0)
        || (matches!(part, Part::Commit | Part::RepairCommit) && !body.is_empty())
        || (matches!(part, Part::Checkpoint | Part::End | Part::Marker) && body.len() != 8)
        || (matches!(
            part,
            Part::Data | Part::History | Part::Restore | Part::RepairBytes
        ) && body.is_empty())
        || (part == Part::RepairBegin && body.len() != 12)
    {
        return Err(AttachError::Invalid);
    }
    if part == Part::Begin {
        let (boundary, _, _) = decode_begin(body)?;
        if boundary.source_bytes != source_bytes {
            return Err(AttachError::Invalid);
        }
    }
    Ok(Fragment {
        part,
        generation,
        offset,
        source_bytes,
        body,
    })
}
/// Fixed 36-byte metadata: dimensions, revision, source, UTF-8, history and restore totals.
pub fn decode_begin(body: &[u8]) -> Result<(Boundary, usize, usize), AttachError> {
    if body.len() != 36 {
        return Err(AttachError::Invalid);
    }
    let boundary = Boundary {
        columns: u16::from_le_bytes([body[0], body[1]]),
        rows: u16::from_le_bytes([body[2], body[3]]),
        revision: u64_at(body, 4)?,
        source_bytes: u64_at(body, 12)?,
        utf8_bytes: u64_at(body, 20)?,
    };
    let history =
        u32::from_le_bytes(body[28..32].try_into().map_err(|_| AttachError::Invalid)?) as usize;
    let restore =
        u32::from_le_bytes(body[32..36].try_into().map_err(|_| AttachError::Invalid)?) as usize;
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
/// Bounded v2 sender. Reservation failure does not mutate either coordinate or phase.
pub struct AttachmentSender {
    generation: u64,
    boundary: Boundary,
    history: Vec<u8>,
    restore: Vec<u8>,
    phase: Part,
    at: usize,
    fragment: usize,
    window: CreditWindow,
    repair: Option<(Vec<u8>, usize, Part)>,
}
impl AttachmentSender {
    /// Validate both metadata and bounded sending credit before emitting Begin.
    pub fn new(
        generation: u64,
        boundary: Boundary,
        history: Vec<u8>,
        restore: Vec<u8>,
        fragment: usize,
        frames: usize,
        bytes: usize,
    ) -> Result<Self, AttachError> {
        if generation == 0
            || history.len() > MAX_HISTORY
            || restore.is_empty()
            || restore.len() > MAX_RESTORE
            || fragment == 0
            || fragment > MAX_FRAGMENT
            || bytes < ENVELOPE_LEN + fragment.max(36)
        {
            return Err(AttachError::Invalid);
        }
        let value = Self {
            generation,
            boundary,
            history,
            restore,
            phase: Part::Begin,
            at: 0,
            fragment,
            window: CreditWindow::new(frames, bytes)?,
            repair: None,
        };
        decode_begin(&value.metadata())?;
        Ok(value)
    }
    fn metadata(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(36);
        out.extend_from_slice(&self.boundary.columns.to_le_bytes());
        out.extend_from_slice(&self.boundary.rows.to_le_bytes());
        out.extend_from_slice(&self.boundary.revision.to_le_bytes());
        out.extend_from_slice(&self.boundary.source_bytes.to_le_bytes());
        out.extend_from_slice(&self.boundary.utf8_bytes.to_le_bytes());
        out.extend_from_slice(&(self.history.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.restore.len() as u32).to_le_bytes());
        out
    }
    fn send(
        &mut self,
        part: Part,
        offset: u64,
        source_bytes: u64,
        body: &[u8],
    ) -> Result<Vec<u8>, AttachError> {
        let payload = Fragment {
            part,
            generation: self.generation,
            offset,
            source_bytes,
            body,
        }
        .encode()?;
        let sequence = self.window.record_send(payload.len())?;
        Ok(Frame {
            kind: FrameKind::Attachment,
            sequence,
            payload: &payload,
        }
        .encode()?)
    }
    /// Bootstrap fragments. No Data/repair can precede the atomic Commit.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, AttachError> {
        if matches!(self.phase, Part::Data | Part::End) {
            return Ok(None);
        }
        let body = match self.phase {
            Part::Begin => self.metadata(),
            Part::History => {
                self.history[self.at..(self.at + self.fragment).min(self.history.len())].to_vec()
            }
            Part::Restore => {
                self.restore[self.at..(self.at + self.fragment).min(self.restore.len())].to_vec()
            }
            Part::Commit => vec![],
            _ => return Err(AttachError::Phase),
        };
        let offset = if matches!(self.phase, Part::Begin | Part::Commit) {
            0
        } else {
            self.at as u64
        };
        let wire = self.send(self.phase, offset, self.boundary.source_bytes, &body)?;
        match self.phase {
            Part::Begin => {
                self.phase = if self.history.is_empty() {
                    Part::Restore
                } else {
                    Part::History
                };
            }
            Part::History => {
                self.at += body.len();
                if self.at == self.history.len() {
                    self.phase = Part::Restore;
                    self.at = 0;
                }
            }
            Part::Restore => {
                self.at += body.len();
                if self.at == self.restore.len() {
                    self.phase = Part::Commit;
                    self.at = 0;
                }
            }
            Part::Commit => {
                self.phase = Part::Data;
                self.history = Vec::new();
                self.restore = Vec::new();
            }
            _ => return Err(AttachError::Phase),
        }
        Ok(Some(wire))
    }
    fn live(&self) -> Result<(), AttachError> {
        if self.phase != Part::Data || self.repair.is_some() {
            Err(AttachError::Phase)
        } else {
            Ok(())
        }
    }
    /// Emit display bytes and an independently monotonic source endpoint. Mid-source-record
    /// fragments may retain the previous source boundary until the complete record is emitted.
    pub fn data(&mut self, bytes: &[u8], source_after: u64) -> Result<Vec<u8>, AttachError> {
        self.live()?;
        if bytes.is_empty()
            || bytes.len() > self.fragment
            || source_after < self.boundary.source_bytes
        {
            return Err(AttachError::Invalid);
        }
        let next = self
            .boundary
            .utf8_bytes
            .checked_add(bytes.len() as u64)
            .ok_or(AttachError::Overflow)?;
        let wire = self.send(Part::Data, self.boundary.utf8_bytes, source_after, bytes)?;
        self.boundary.utf8_bytes = next;
        self.boundary.source_bytes = source_after;
        Ok(wire)
    }
    fn control(&mut self, b: Boundary, part: Part) -> Result<Vec<u8>, AttachError> {
        self.live()?;
        if b.columns != self.boundary.columns
            || b.rows != self.boundary.rows
            || b.utf8_bytes != self.boundary.utf8_bytes
            || b.revision < self.boundary.revision
            || b.source_bytes < self.boundary.source_bytes
            || (part != Part::Marker && b.source_bytes != self.boundary.source_bytes)
        {
            return Err(AttachError::Invalid);
        }
        let wire = self.send(
            part,
            b.utf8_bytes,
            b.source_bytes,
            &b.revision.to_le_bytes(),
        )?;
        self.boundary = b;
        Ok(wire)
    }
    /// Pure state marker may advance source without display bytes (pending decoder input).
    pub fn marker(&mut self, b: Boundary) -> Result<Vec<u8>, AttachError> {
        self.control(b, Part::Marker)
    }
    /// Ordered sync checkpoint cannot invent new source/display bytes.
    pub fn checkpoint(&mut self, b: Boundary) -> Result<Vec<u8>, AttachError> {
        self.control(b, Part::Checkpoint)
    }
    /// EOF must match both live cursors and cannot silently advance consumed source.
    pub fn finish(&mut self, b: Boundary) -> Result<Vec<u8>, AttachError> {
        let wire = self.control(b, Part::End)?;
        self.phase = Part::End;
        Ok(wire)
    }
    /// Stage one bounded exact repair without advancing either cursor.
    pub fn start_repair(&mut self, b: Boundary, bytes: Vec<u8>) -> Result<(), AttachError> {
        self.live()?;
        if b.columns != self.boundary.columns
            || b.rows != self.boundary.rows
            || b.source_bytes != self.boundary.source_bytes
            || b.utf8_bytes != self.boundary.utf8_bytes
            || b.revision < self.boundary.revision
            || bytes.is_empty()
            || bytes.len() > MAX_RESTORE
        {
            return Err(AttachError::Invalid);
        }
        self.boundary = b;
        self.repair = Some((bytes, 0, Part::RepairBegin));
        Ok(())
    }
    /// Repair framing shares the same ordered sequence and credit window as live output.
    pub fn next_repair(&mut self) -> Result<Option<Vec<u8>>, AttachError> {
        let Some((bytes, at, part)) = &self.repair else {
            return Ok(None);
        };
        let part = *part;
        let (offset, body) = match part {
            Part::RepairBegin => {
                let mut meta = self.boundary.revision.to_le_bytes().to_vec();
                meta.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                (self.boundary.utf8_bytes, meta)
            }
            Part::RepairBytes => (
                *at as u64,
                bytes[*at..(*at + self.fragment).min(bytes.len())].to_vec(),
            ),
            Part::RepairCommit => (self.boundary.utf8_bytes, vec![]),
            _ => return Err(AttachError::Phase),
        };
        let wire = self.send(part, offset, self.boundary.source_bytes, &body)?;
        if let Some(r) = self.repair.as_mut() {
            match part {
                Part::RepairBegin => r.2 = Part::RepairBytes,
                Part::RepairBytes => {
                    r.1 += body.len();
                    if r.1 == r.0.len() {
                        r.2 = Part::RepairCommit;
                    }
                }
                Part::RepairCommit => self.repair = None,
                _ => return Err(AttachError::Phase),
            }
        }
        Ok(Some(wire))
    }
    /// Only this generation and an outstanding cumulative sequence release actual payload credit.
    pub fn acknowledge(&mut self, generation: u64, sequence: u32) -> Result<Released, AttachError> {
        if generation != self.generation {
            return Err(AttachError::Generation);
        }
        Ok(self.window.acknowledge(sequence)?)
    }
    /// Actual envelope plus body credit in flight, independent of source/display counters.
    pub fn outstanding_bytes(&self) -> usize {
        self.window.outstanding_bytes()
    }
}
#[cfg(test)]
#[path = "attach_v2_tests.rs"]
mod tests;
