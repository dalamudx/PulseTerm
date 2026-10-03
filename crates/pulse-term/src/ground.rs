//! Coarse model of the vte 0.15 escape-sequence state machine. It only answers
//! "is the parser between sequences?" and keeps the (bounded) bytes of an
//! unfinished sequence so a restore can replay them. Differentially tested
//! against vte itself; it never feeds or alters the real parser.

/// Longest unfinished sequence kept for replay. Longer ones (e.g. huge OSC 52)
/// make a restore wait until the parser returns to ground.
pub(crate) const MAX_PENDING: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    Osc,
    /// DCS, SOS, PM and APC strings: everything up to ESC is consumed.
    String,
}

#[derive(Debug, Clone)]
pub(crate) struct GroundScanner {
    state: State,
    pending: Vec<u8>,
    overflow: bool,
}

impl Default for GroundScanner {
    fn default() -> Self {
        Self {
            state: State::Ground,
            pending: Vec::new(),
            overflow: false,
        }
    }
}

impl GroundScanner {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.step(byte);
        }
    }

    fn step(&mut self, byte: u8) {
        use State::*;
        let next = match (self.state, byte) {
            (_, 0x18 | 0x1a) => Ground,
            (_, 0x1b) => Escape,
            (Ground, _) => Ground,
            (Escape, 0x20..=0x2f) => EscapeIntermediate,
            (Escape, b'[') => Csi,
            (Escape, b']') => Osc,
            (Escape, b'P' | b'X' | b'^' | b'_') => String,
            (Escape | EscapeIntermediate, 0x30..=0x7e) => Ground,
            (Csi, 0x40..=0x7e) => Ground,
            (Osc, 0x07) => Ground,
            (state, _) => state,
        };
        if next == Ground {
            self.pending.clear();
            self.overflow = false;
        } else {
            if self.state == Ground || byte == 0x1b {
                // A new sequence starts; an aborted partial sequence is dropped by
                // vte as well, so only the newest one needs replaying.
                self.pending.clear();
                self.overflow = false;
            }
            // C0 controls inside a sequence are executed immediately by vte (or
            // ignored in strings); they are already applied and must not be
            // replayed. DEL is ignored. Neither changes the sequence itself.
            let executed =
                matches!(byte, 0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f) && next == self.state;
            if executed {
            } else if self.pending.len() < MAX_PENDING {
                self.pending.push(byte);
            } else {
                self.overflow = true;
            }
        }
        self.state = next;
    }

    #[cfg(test)]
    pub fn is_ground(&self) -> bool {
        self.state == State::Ground
    }

    /// Bytes of the unfinished sequence, or `None` if it exceeded [`MAX_PENDING`].
    pub fn pending(&self) -> Option<&[u8]> {
        (!self.overflow).then_some(self.pending.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::{Parser, Perform};
    use proptest::prelude::*;

    #[derive(Default)]
    struct Printed(Vec<char>);
    impl Perform for Printed {
        fn print(&mut self, c: char) {
            self.0.push(c);
        }
    }

    const TOKENS: &[&[u8]] = &[
        b"\x1b",
        b"[",
        b"]",
        b"P",
        b"X",
        b"^",
        b"_",
        b"\\",
        b"\x07",
        b"\x18",
        b"\x1a",
        b";",
        b"0",
        b"1",
        b"?",
        b" ",
        b"(",
        b"m",
        b"H",
        b"q",
        b"\x0e",
        b"\x0f",
        b"\r",
        b"\n",
        b"\x7f",
        "é".as_bytes(),
        "中".as_bytes(),
        b"a",
    ];

    proptest! {
        /// Oracle: vte prints an `x` iff it is in ground state before it.
        #[test]
        fn ground_matches_vte(tokens in prop::collection::vec(0..TOKENS.len(), 0..40)) {
            let bytes: Vec<u8> = tokens.iter().flat_map(|&i| TOKENS[i].iter().copied()).collect();
            let mut parser = Parser::new();
            let mut printed = Printed::default();
            parser.advance(&mut printed, &bytes);
            printed.0.clear();
            parser.advance(&mut printed, b"x");
            let mut scanner = GroundScanner::default();
            scanner.feed(&bytes);
            prop_assert_eq!(printed.0 == ['x'], scanner.is_ground(), "bytes {:?}", bytes);
        }

        /// Replaying the pending tail into a fresh parser reproduces the state.
        #[test]
        fn pending_tail_reproduces_state(tokens in prop::collection::vec(0..TOKENS.len(), 0..40)) {
            let bytes: Vec<u8> = tokens.iter().flat_map(|&i| TOKENS[i].iter().copied()).collect();
            let mut scanner = GroundScanner::default();
            scanner.feed(&bytes);
            let mut fresh = GroundScanner::default();
            fresh.feed(scanner.pending().unwrap_or_default());
            prop_assert_eq!(fresh.is_ground(), scanner.is_ground());
            let mut original = Parser::new();
            let mut replay = Parser::new();
            let (mut a, mut b) = (Printed::default(), Printed::default());
            original.advance(&mut a, &bytes);
            replay.advance(&mut b, scanner.pending().unwrap_or_default());
            a.0.clear();
            b.0.clear();
            original.advance(&mut a, b"x;1m\x07\x1b\\yz");
            replay.advance(&mut b, b"x;1m\x07\x1b\\yz");
            prop_assert_eq!(a.0, b.0);
        }
    }

    #[test]
    fn oversized_sequence_reports_overflow_until_ground() {
        let mut scanner = GroundScanner::default();
        scanner.feed(b"\x1b]52;c;");
        scanner.feed(&vec![b'A'; MAX_PENDING]);
        assert!(scanner.pending().is_none());
        scanner.feed(b"\x07");
        assert!(scanner.is_ground());
        assert_eq!(scanner.pending(), Some(&[][..]));
    }
}
