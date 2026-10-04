//! Version adapter only. The Driver owns shared cancellation, delivery and ACK deadlines.
use pulse_ipc::{
    attach::{self, AttachError},
    attach_v2,
    flow::Released,
};
use pulse_term::session::SessionState;
enum Version {
    V1(attach::AttachmentSender),
    V2(attach_v2::AttachmentSender),
}
pub(super) struct Sender {
    inner: Version,
    source: u64,
}
fn v1(b: &SessionState) -> attach::Boundary {
    attach::Boundary {
        columns: b.size.columns(),
        rows: b.size.rows(),
        revision: b.revision,
        parsed_bytes: b.parsed_bytes,
    }
}
fn v2(b: &SessionState) -> attach_v2::Boundary {
    attach_v2::Boundary {
        columns: b.size.columns(),
        rows: b.size.rows(),
        revision: b.revision,
        source_bytes: b.parsed_bytes,
        utf8_bytes: b.utf8_bytes,
    }
}
impl Sender {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dual: bool,
        generation: u64,
        b: &SessionState,
        history: Vec<u8>,
        restore: Vec<u8>,
        fragment: usize,
        frames: usize,
        bytes: usize,
    ) -> Result<Self, AttachError> {
        let inner = if dual {
            Version::V2(attach_v2::AttachmentSender::new(
                generation,
                v2(b),
                history,
                restore,
                fragment,
                frames,
                bytes,
            )?)
        } else {
            Version::V1(attach::AttachmentSender::new(
                generation,
                v1(b),
                history,
                restore,
                fragment,
                frames,
                bytes,
            )?)
        };
        Ok(Self {
            inner,
            source: b.parsed_bytes,
        })
    }
    pub fn source_bytes(&self) -> u64 {
        self.source
    }
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, AttachError> {
        match &mut self.inner {
            Version::V1(s) => s.next_frame(),
            Version::V2(s) => s.next_frame(),
        }
    }
    pub fn next_repair(&mut self) -> Result<Option<Vec<u8>>, AttachError> {
        match &mut self.inner {
            Version::V1(s) => s.next_repair(),
            Version::V2(s) => s.next_repair(),
        }
    }
    pub fn start_repair(&mut self, b: &SessionState, bytes: Vec<u8>) -> Result<(), AttachError> {
        match &mut self.inner {
            Version::V1(s) => s.start_repair(v1(b), bytes),
            Version::V2(s) => s.start_repair(v2(b), bytes),
        }
    }
    pub fn data(&mut self, bytes: &[u8], source_after: u64) -> Result<Vec<u8>, AttachError> {
        let wire = match &mut self.inner {
            Version::V1(s) => {
                let wire = s.data(bytes)?;
                self.source = self
                    .source
                    .checked_add(bytes.len() as u64)
                    .ok_or(AttachError::Overflow)?;
                wire
            }
            Version::V2(s) => {
                let wire = s.data(bytes, source_after)?;
                self.source = source_after;
                wire
            }
        };
        Ok(wire)
    }
    pub fn marker(&mut self, b: &SessionState) -> Result<Vec<u8>, AttachError> {
        let wire = match &mut self.inner {
            Version::V1(s) => s.marker(v1(b)),
            Version::V2(s) => s.marker(v2(b)),
        }?;
        self.source = b.parsed_bytes;
        Ok(wire)
    }
    pub fn checkpoint(&mut self, b: &SessionState) -> Result<Vec<u8>, AttachError> {
        match &mut self.inner {
            Version::V1(s) => s.checkpoint(v1(b)),
            Version::V2(s) => s.checkpoint(v2(b)),
        }
    }
    pub fn finish(&mut self, b: &SessionState) -> Result<Vec<u8>, AttachError> {
        match &mut self.inner {
            Version::V1(s) => s.finish(v1(b)),
            Version::V2(s) => s.finish(v2(b)),
        }
    }
    pub fn acknowledge(&mut self, generation: u64, sequence: u32) -> Result<Released, AttachError> {
        match &mut self.inner {
            Version::V1(s) => s.acknowledge(generation, sequence),
            Version::V2(s) => s.acknowledge(generation, sequence),
        }
    }
    pub fn outstanding_bytes(&self) -> usize {
        match &self.inner {
            Version::V1(s) => s.outstanding_bytes(),
            Version::V2(s) => s.outstanding_bytes(),
        }
    }
}
