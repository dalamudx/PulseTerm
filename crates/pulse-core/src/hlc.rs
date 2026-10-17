//! Hybrid timestamp value; generation takes an explicitly supplied backend wall clock.
use uuid::Uuid;
/// Static invalid/exhausted timestamp failure, no metadata values included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("hlc.invalid_or_exhausted")]
pub struct InvalidHlc;
/// Comparable timestamp metadata. Not a trusted clock, revision or permission.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hlc {
    wall: u64,
    counter: u32,
    node: Uuid,
}
impl Hlc {
    /// Positive SQLite-compatible milliseconds and a non-nil stable backend node.
    pub fn new(wall: u64, counter: u32, node: Uuid) -> Result<Self, InvalidHlc> {
        if wall == 0 || wall > i64::MAX as u64 || node.is_nil() {
            return Err(InvalidHlc);
        }
        Ok(Self {
            wall,
            counter,
            node,
        })
    }
    /// Advance despite backward/equal wall clock. Never wrap the logical counter.
    pub fn next(self, now: u64) -> Result<Self, InvalidHlc> {
        if now > self.wall {
            Self::new(now, 0, self.node)
        } else {
            Self::new(
                self.wall,
                self.counter.checked_add(1).ok_or(InvalidHlc)?,
                self.node,
            )
        }
    }
    /// Canonical bounded ASCII storage form; not an externally accepted clock update.
    pub fn encode(self) -> String {
        format!("{}:{}:{}", self.wall, self.counter, self.node)
    }
    /// Reject unknown syntax, padding, noncanonical UUIDs and overflow before use.
    pub fn parse(value: &str) -> Result<Self, InvalidHlc> {
        if value.len() > 67 {
            return Err(InvalidHlc);
        }
        let mut p = value.split(':');
        let wall = p
            .next()
            .ok_or(InvalidHlc)?
            .parse()
            .map_err(|_| InvalidHlc)?;
        let counter = p
            .next()
            .ok_or(InvalidHlc)?
            .parse()
            .map_err(|_| InvalidHlc)?;
        let node = Uuid::parse_str(p.next().ok_or(InvalidHlc)?).map_err(|_| InvalidHlc)?;
        if p.next().is_some() {
            return Err(InvalidHlc);
        }
        let h = Self::new(wall, counter, node)?;
        if h.encode() != value {
            return Err(InvalidHlc);
        }
        Ok(h)
    }
    /// Backend timestamp milliseconds, not authorization expiry.
    pub fn wall(self) -> u64 {
        self.wall
    }
    /// Stable node discriminator, not registry/store affinity.
    pub fn node(self) -> Uuid {
        self.node
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_canonical_skew_and_exhaustion() -> Result<(), InvalidHlc> {
        let n = Uuid::from_bytes([1; 16]);
        let mut h = Hlc::new(100, 0, n)?;
        for now in [100, 99, 1, 102, 101] {
            let next = h.next(now)?;
            assert!(next > h);
            assert!(Hlc::parse(&next.encode())? == next);
            assert_eq!(next.node(), n);
            h = next;
        }
        assert!(Hlc::new(100, u32::MAX, n)?.next(100).is_err());
        assert!(Hlc::new(100, u32::MAX, n)?.next(101).is_ok());
        assert!(Hlc::new(0, 0, n).is_err());
        assert!(Hlc::new(1, 0, Uuid::nil()).is_err());
        for s in [
            "",
            "001:0:01010101-0101-0101-0101-010101010101",
            "1:00:01010101-0101-0101-0101-010101010101",
            "1:0:01010101010101010101010101010101",
            "1:4294967296:01010101-0101-0101-0101-010101010101",
        ] {
            assert!(Hlc::parse(s).is_err());
        }
        Ok(())
    }
}
