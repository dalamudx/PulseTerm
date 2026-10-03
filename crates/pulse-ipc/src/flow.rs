//! A bounded sending window; only acknowledgments of sent frames release credit.

use std::collections::VecDeque;

use crate::MAX_PAYLOAD;

/// Invalid sending-window operation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FlowError {
    /// Configuration cannot bound the queue.
    #[error("invalid sending-window limits")]
    InvalidLimits,
    /// Wait for a valid acknowledgment before sending more data.
    #[error("sending window exhausted")]
    NoCredit,
    /// Duplicated, stale or unsent sequence.
    #[error("acknowledgment is not outstanding")]
    InvalidAck,
    /// Empty or oversized frame.
    #[error("invalid payload length")]
    InvalidPayload,
}

/// Credit released by a cumulative acknowledgment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Released {
    /// Number of frames consumed.
    pub frames: usize,
    /// Number of payload bytes consumed.
    pub bytes: usize,
}

/// Pure state machine. Reset by creating a new window for a new attachment.
pub struct CreditWindow {
    max_frames: usize,
    max_bytes: usize,
    bytes: usize,
    next: u32,
    outstanding: VecDeque<(u32, usize)>,
}

impl CreditWindow {
    /// Limit both frame count and bytes; bounds metadata as well as data.
    pub fn new(max_frames: usize, max_bytes: usize) -> Result<Self, FlowError> {
        if !(1..=1024).contains(&max_frames) || max_bytes == 0 || max_bytes > 64 * MAX_PAYLOAD {
            return Err(FlowError::InvalidLimits);
        }
        Ok(Self {
            max_frames,
            max_bytes,
            bytes: 0,
            next: 0,
            outstanding: VecDeque::with_capacity(max_frames),
        })
    }

    /// Reserve credit before enqueueing a frame; sequence wraps at u32::MAX.
    pub fn record_send(&mut self, payload_bytes: usize) -> Result<u32, FlowError> {
        if payload_bytes == 0 || payload_bytes > MAX_PAYLOAD {
            return Err(FlowError::InvalidPayload);
        }
        if self.outstanding.len() == self.max_frames || self.bytes + payload_bytes > self.max_bytes
        {
            return Err(FlowError::NoCredit);
        }
        let sequence = self.next;
        self.next = self.next.wrapping_add(1);
        self.bytes += payload_bytes;
        self.outstanding.push_back((sequence, payload_bytes));
        Ok(sequence)
    }

    /// Cumulatively acknowledge an outstanding sequence, not an arbitrary byte count.
    pub fn acknowledge(&mut self, sequence: u32) -> Result<Released, FlowError> {
        let index = self
            .outstanding
            .iter()
            .position(|(seq, _)| *seq == sequence)
            .ok_or(FlowError::InvalidAck)?;
        let mut released = Released {
            frames: 0,
            bytes: 0,
        };
        for _ in 0..=index {
            if let Some((_, bytes)) = self.outstanding.pop_front() {
                released.frames += 1;
                released.bytes += bytes;
            }
        }
        self.bytes -= released.bytes;
        Ok(released)
    }

    /// Frames that must still be consumed by this attachment.
    pub fn outstanding_frames(&self) -> usize {
        self.outstanding.len()
    }
    /// Payload bytes retained or in transit.
    pub fn outstanding_bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn credit_cannot_be_inflated_by_duplicate_or_future_ack() {
        let mut window = CreditWindow::new(2, 8).unwrap();
        assert_eq!(window.record_send(4), Ok(0));
        assert_eq!(window.record_send(4), Ok(1));
        assert_eq!(window.record_send(1), Err(FlowError::NoCredit));
        assert_eq!(window.acknowledge(2), Err(FlowError::InvalidAck));
        assert_eq!(window.outstanding_bytes(), 8);
        assert_eq!(
            window.acknowledge(0),
            Ok(Released {
                frames: 1,
                bytes: 4
            })
        );
        assert_eq!(window.acknowledge(0), Err(FlowError::InvalidAck));
        assert_eq!(window.outstanding_bytes(), 4);
        assert_eq!(window.record_send(4), Ok(2));
        assert_eq!(
            window.acknowledge(2),
            Ok(Released {
                frames: 2,
                bytes: 8
            })
        );
        assert_eq!(window.outstanding_frames(), 0);
    }

    #[test]
    fn cumulative_ack_handles_sequence_wrap() {
        let mut window = CreditWindow::new(4, 64).unwrap();
        window.next = u32::MAX;
        assert_eq!(window.record_send(8), Ok(u32::MAX));
        assert_eq!(window.record_send(8), Ok(0));
        assert_eq!(
            window.acknowledge(0),
            Ok(Released {
                frames: 2,
                bytes: 16
            })
        );
        assert_eq!(window.acknowledge(u32::MAX), Err(FlowError::InvalidAck));
    }

    #[test]
    fn invalid_limits_and_payloads_are_rejected() {
        for (frames, bytes) in [(0, 1), (1025, 1), (1, 0), (1, MAX_PAYLOAD * 65)] {
            assert!(matches!(
                CreditWindow::new(frames, bytes),
                Err(FlowError::InvalidLimits)
            ));
        }
        let mut window = CreditWindow::new(2, 8).unwrap();
        assert_eq!(window.record_send(0), Err(FlowError::InvalidPayload));
        assert_eq!(
            window.record_send(MAX_PAYLOAD + 1),
            Err(FlowError::InvalidPayload)
        );
        assert_eq!(window.record_send(9), Err(FlowError::NoCredit));
    }
}
