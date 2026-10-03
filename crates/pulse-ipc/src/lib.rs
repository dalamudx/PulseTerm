//! Session-local binary frames. The enclosing channel supplies frame boundaries.
#![forbid(unsafe_code)]

/// Header: kind u8, flags u8, sequence u32 (little endian).
pub const HEADER_LEN: usize = 6;
/// Bound payload allocation at every IPC boundary.
pub const MAX_PAYLOAD: usize = 1024 * 1024;

/// Frame kinds supported by the initial codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    /// Terminal VT output.
    Data = 0x01,
    /// VT snapshot.
    Snapshot = 0x02,
    /// Terminal title metadata.
    Title = 0x03,
    /// Terminal bell.
    Bell = 0x04,
    /// OSC 133 mark.
    Mark = 0x05,
    /// Working directory metadata.
    Cwd = 0x06,
    /// Session exit.
    Exit = 0x07,
    /// Desktop dirty rectangle.
    Rect = 0x20,
    /// H.264 bitstream.
    H264 = 0x21,
    /// Desktop cursor.
    Cursor = 0x22,
    /// Desktop dimensions.
    Resize = 0x23,
    /// Reserved audio payload.
    Audio = 0x24,
    /// File listing page.
    ListChunk = 0x40,
    /// Transfer progress.
    Progress = 0x41,
    /// RFB stream bytes; never skip these frames.
    RfbBytes = 0x60,
}

impl TryFrom<u8> for FrameKind {
    type Error = FrameError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0x01 => Self::Data,
            0x02 => Self::Snapshot,
            0x03 => Self::Title,
            0x04 => Self::Bell,
            0x05 => Self::Mark,
            0x06 => Self::Cwd,
            0x07 => Self::Exit,
            0x20 => Self::Rect,
            0x21 => Self::H264,
            0x22 => Self::Cursor,
            0x23 => Self::Resize,
            0x24 => Self::Audio,
            0x40 => Self::ListChunk,
            0x41 => Self::Progress,
            0x60 => Self::RfbBytes,
            _ => return Err(FrameError::UnknownKind(value)),
        })
    }
}

/// Codec failures are independent of the transport.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    /// Fewer than six bytes.
    #[error("truncated frame header")]
    Truncated,
    /// Kind not defined by this protocol version.
    #[error("unknown frame kind: {0:#x}")]
    UnknownKind(u8),
    /// Reserved bits must remain zero until versioned.
    #[error("unsupported frame flags: {0:#x}")]
    UnsupportedFlags(u8),
    /// Payload exceeds the limit.
    #[error("frame payload exceeds 1 MiB")]
    TooLarge,
}

/// Borrowed payload avoids copying large frames during decode.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Payload discriminator.
    pub kind: FrameKind,
    /// Per-attachment sequence, wrapping at u32::MAX.
    pub sequence: u32,
    /// Interpretation depends on kind.
    pub payload: &'a [u8],
}

impl Frame<'_> {
    /// Encode a complete channel message.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        if self.payload.len() > MAX_PAYLOAD {
            return Err(FrameError::TooLarge);
        }
        let mut bytes = Vec::with_capacity(HEADER_LEN + self.payload.len());
        bytes.extend_from_slice(&[self.kind as u8, 0]);
        bytes.extend_from_slice(&self.sequence.to_le_bytes());
        bytes.extend_from_slice(self.payload);
        Ok(bytes)
    }
}

/// Decode only a complete message, never a partial TCP read.
pub fn decode(bytes: &[u8]) -> Result<Frame<'_>, FrameError> {
    if bytes.len() < HEADER_LEN {
        return Err(FrameError::Truncated);
    }
    if bytes.len() - HEADER_LEN > MAX_PAYLOAD {
        return Err(FrameError::TooLarge);
    }
    let kind = FrameKind::try_from(bytes[0])?;
    if bytes[1] != 0 {
        return Err(FrameError::UnsupportedFlags(bytes[1]));
    }
    let sequence = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
    Ok(Frame {
        kind,
        sequence,
        payload: &bytes[HEADER_LEN..],
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn shared_fixture() {
        let fixture: Vec<u8> = serde_free_hex();
        let frame = decode(&fixture).unwrap();
        assert_eq!(frame.kind, FrameKind::Data);
        assert_eq!(frame.sequence, 0x12345678);
        assert_eq!(frame.payload, b"hello\r\n");
        assert_eq!(frame.encode().unwrap(), fixture);
    }

    fn serde_free_hex() -> Vec<u8> {
        include_str!("../../../tests/fixtures/frames/data.hex")
            .split_whitespace()
            .map(|v| u8::from_str_radix(v, 16).unwrap())
            .collect()
    }

    #[test]
    fn rejects_invalid_messages() {
        for len in 0..HEADER_LEN {
            assert_eq!(decode(&vec![0; len]), Err(FrameError::Truncated));
        }
        assert_eq!(
            decode(&[255, 0, 0, 0, 0, 0]),
            Err(FrameError::UnknownKind(255))
        );
        assert_eq!(
            decode(&[1, 1, 0, 0, 0, 0]),
            Err(FrameError::UnsupportedFlags(1))
        );
        let payload = vec![0; MAX_PAYLOAD + 1];
        assert_eq!(
            Frame {
                kind: FrameKind::Data,
                sequence: 0,
                payload: &payload
            }
            .encode(),
            Err(FrameError::TooLarge)
        );
        assert_eq!(
            decode(&vec![0; MAX_PAYLOAD + HEADER_LEN + 1]),
            Err(FrameError::TooLarge)
        );
    }

    proptest! {
        #[test]
        fn roundtrip(sequence in any::<u32>(), payload in prop::collection::vec(any::<u8>(), 0..4096)) {
            let frame = Frame { kind: FrameKind::RfbBytes, sequence, payload: &payload };
            let encoded = frame.encode().unwrap();
            prop_assert_eq!(decode(&encoded).unwrap(), frame);
        }
        #[test]
        fn arbitrary_input_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let _ = decode(&bytes);
        }
    }
}
