use super::*;
#[test]
#[allow(clippy::unwrap_used)]
fn recent_cutoff_never_forges_a_hard_line_from_a_continuation() {
    let mut source = TerminalProbe::new(pulse_core::TermSize::new(8, 4).unwrap(), 64);
    source.feed("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\r\nend\r\n".as_bytes());
    let replay = source.recent_history_bounded(2, 100_000).unwrap();
    assert_eq!(replay.retained_rows, 0);
    assert!(replay.total_rows > 2);
    assert!(replay.bytes.is_empty());
}
use crate::restore::{cell_key, cursor_key};
#[test]
#[allow(clippy::unwrap_used)]
fn history_soft_wrap_colours_links_and_hidden_primary_survive_restore() {
    let size = pulse_core::TermSize::new(16, 4).unwrap();
    for alternate in [false, true] {
        let mut source = TerminalProbe::new(size, 64);
        source.feed(b"\x1b[31m123456789012345678901234567890\x1b[0m\r\n\x1b]8;;https://x\x1b\\LINK\x1b]8;;\x1b\\\r\nthird\r\nfourth\r\nfifth\r\nlast");
        if alternate {
            source.feed(b"\x1b[?1049hALT");
        }
        let history = source.recent_history_bounded(100, 64 * 1024).unwrap();
        assert!(history.retained_rows > 0);
        assert_eq!(history.total_rows, history.retained_rows);
        let restore = source.restore_screen_bounded(1024 * 1024).unwrap();
        assert!(restore.approximations.is_empty());
        assert!(!restore.bytes.windows(2).any(|w| w == b"\x1bc"));
        let primary = if alternate {
            source.term.inactive_grid()
        } else {
            source.term.grid()
        };
        let old_cursor = cursor_key(&primary.cursor);
        let mut fresh = TerminalProbe::new(size, 64);
        fresh.feed(&history.bytes);
        fresh.feed(&restore.bytes);
        assert_eq!(fresh.restore_fingerprint(), source.restore_fingerprint());
        let replay = if alternate {
            fresh.term.inactive_grid()
        } else {
            fresh.term.grid()
        };
        assert_eq!(replay.history_size(), primary.history_size());
        for line in -(history.retained_rows as i32)..0 {
            for col in 0..primary.columns() {
                assert_eq!(
                    cell_key(&replay[Line(line)][Column(col)]),
                    cell_key(&primary[Line(line)][Column(col)]),
                    "{line}:{col}"
                );
                assert_eq!(
                    replay[Line(line)][Column(col)]
                        .flags
                        .contains(Flags::WRAPLINE),
                    primary[Line(line)][Column(col)]
                        .flags
                        .contains(Flags::WRAPLINE)
                );
            }
        }
        assert_eq!(old_cursor, cursor_key(&primary.cursor));
        assert!(fresh.take_replies().is_empty());
    }
}
#[test]
#[allow(clippy::unwrap_used)]
fn recent_history_budget_and_suffix_limit_are_explicit() {
    let size = pulse_core::TermSize::new(16, 4).unwrap();
    let mut source = TerminalProbe::new(size, 64);
    for _ in 0..20 {
        source.feed(b"row\r\n");
    }
    let replay = source.recent_history_bounded(3, 64 * 1024).unwrap();
    assert_eq!(replay.retained_rows, 3);
    assert!(replay.total_rows > 3);
    assert!(
        source
            .recent_history_bounded(0, 0)
            .unwrap()
            .bytes
            .is_empty()
    );
    assert_eq!(
        source
            .recent_history_bounded(3, replay.bytes.len() - 1)
            .unwrap_err(),
        RestoreGap::OutputTooLarge
    );
    assert_eq!(
        source
            .recent_history_bounded(3, replay.bytes.len())
            .unwrap()
            .bytes,
        replay.bytes
    );
}
