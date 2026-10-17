use super::*;
#[test]
#[allow(clippy::unwrap_used)]
fn recovery_clear_cannot_manufacture_history_and_shift_hidden_cursor_on_grow() {
    let size = pulse_core::TermSize::new(80, 24).unwrap();
    let mut original = TerminalProbe::new(size, 128);
    original.feed(b"\x1b[?1049happ");
    let frame = original.restore_sequence().unwrap();
    let mut restored = TerminalProbe::new(size, 128);
    restored.feed(&frame.bytes);
    assert_eq!(original.recent_history_bounded(0, 0).unwrap().total_rows, 0);
    assert_eq!(restored.recent_history_bounded(0, 0).unwrap().total_rows, 0);
    let grow = pulse_core::TermSize::new(91, 37).unwrap();
    original.resize(grow);
    restored.resize(grow);
    assert_eq!(
        restored.restore_fingerprint(),
        original.restore_fingerprint()
    );
}
#[test]
#[allow(clippy::unwrap_used)]
fn recovery_clear_preserves_existing_primary_history_in_backend_and_renderer() {
    let size = pulse_core::TermSize::new(24, 6).unwrap();
    let seed = b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\neight\r\n";
    let mut source = TerminalProbe::new(size, 128);
    source.feed(seed);
    let history = source.recent_history_projection(128);
    let frame = source.restore_screen_bounded(1024 * 1024).unwrap();
    source.feed(&frame.bytes);
    assert_eq!(
        source
            .recent_history_projection(128)
            .into_iter()
            .map(|row| (row.cells, row.wrapped))
            .collect::<Vec<_>>(),
        history
            .into_iter()
            .map(|row| (row.cells, row.wrapped))
            .collect::<Vec<_>>()
    );
}

#[test]
#[allow(clippy::unwrap_used)]
fn restore_run_repairs_do_not_recolour_skipped_default_tail() {
    let mut source = TerminalProbe::new(pulse_core::TermSize::new(24, 6).unwrap(), 64);
    source.feed("\x1b[4h\x1b[38;2;1;2;3;48;5;200m中\x1b7\x08a".as_bytes());
    let restore = source.restore_sequence().unwrap();
    let mut fresh = TerminalProbe::new(pulse_core::TermSize::new(24, 6).unwrap(), 64);
    fresh.feed(&restore.bytes);
    assert_eq!(fresh.restore_fingerprint(), source.restore_fingerprint());
}

#[test]
fn color_pen_cannot_bypass_generation_budget() {
    let mut out = Emitter {
        buf: String::new(),
        last_print: None,
        limit: 4,
        exceeded: false,
    };
    let cell = Cell {
        fg: Color::Spec(alacritty_terminal::vte::ansi::Rgb { r: 1, g: 2, b: 3 }),
        bg: Color::Indexed(200),
        ..Cell::default()
    };
    write_pen(&mut out, &cell);
    assert!(out.exceeded, "colour emission must trip the same budget");
    assert!(
        out.buf.len() <= 4,
        "formatting colours bypassed the payload budget"
    );
}

#[test]
fn emitter_checks_all_append_paths_and_stops_after_failure() {
    let mut out = Emitter {
        buf: String::new(),
        last_print: None,
        limit: 6,
        exceeded: false,
    };
    out.push_str("a");
    out.push('中');
    assert!(write!(&mut out, "{}", 12).is_ok());
    assert_eq!(out.buf.len(), 6);
    out.print('é');
    assert!(out.exceeded);
    assert_eq!(out.buf, "a中12");
    out.push_str("ignored");
    assert!(write!(&mut out, "not appended").is_err());
    assert_eq!(out.buf.len(), 6);
}
#[test]
#[allow(clippy::unwrap_used)]
fn payload_bound_includes_incomplete_utf8_and_escape_tail() {
    let mut source = TerminalProbe::new(pulse_core::TermSize::new(24, 6).unwrap(), 0);
    source.feed(b"main\x1b[?1049halt\x1b]10;#123");
    source.feed(&[0xf0, 0x9f]);
    let before = source.restore_fingerprint();
    let restore = source.restore_sequence().unwrap();
    for budget in [0, 1, 2, restore.bytes.len() - 1] {
        assert_eq!(
            source.restore_sequence_bounded(budget),
            Err(RestoreGap::OutputTooLarge)
        );
        assert_eq!(source.restore_fingerprint(), before);
    }
    assert_eq!(
        source
            .restore_sequence_bounded(restore.bytes.len())
            .unwrap(),
        restore
    );
}

#[test]
#[allow(clippy::unwrap_used)]
fn screen_soft_wraps_remain_logical_continuations_after_recovery() {
    let size = pulse_core::TermSize::new(24, 6).unwrap();
    for alternate in [false, true] {
        let mut source = TerminalProbe::new(size, 128);
        for n in 0..12 {
            source.feed(format!("H{n} 漢字🙂e\u{301} {} END{n}\r\n", "A".repeat(37)).as_bytes());
        }
        if alternate {
            source.feed(b"\x1b[?1049hABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz\r\nZ");
        }
        let history = source.recent_history_bounded(128, 1_000_000).unwrap();
        let restore = source.restore_screen_bounded(1_000_000).unwrap();
        assert!(restore.approximations.is_empty());
        let mut fresh = TerminalProbe::new(size, 128);
        fresh.feed(&history.bytes);
        fresh.feed(&restore.bytes);
        assert_eq!(fresh.restore_fingerprint(), source.restore_fingerprint());
        for (original, recovered) in [
            (source.term.grid(), fresh.term.grid()),
            (source.term.inactive_grid(), fresh.term.inactive_grid()),
        ] {
            assert_eq!(original.history_size(), recovered.history_size());
            for line in 0..size.rows() as i32 {
                let column = Column(size.columns() as usize - 1);
                assert_eq!(
                    original[Line(line)][column].flags.contains(Flags::WRAPLINE),
                    recovered[Line(line)][column]
                        .flags
                        .contains(Flags::WRAPLINE),
                    "alternate={alternate}, viewport row={line}"
                );
            }
        }
        assert!(fresh.take_replies().is_empty());
    }
}

#[test]
#[allow(clippy::unwrap_used)]
fn unreproducible_soft_wrap_boundaries_cannot_claim_exact_recovery() {
    let size = pulse_core::TermSize::new(8, 4).unwrap();
    for (sequence, row, reason) in [
        (
            b"123456789\r\x1b[2K".as_slice(),
            0,
            "empty soft-wrap continuation",
        ),
        (
            b"123456789\x1b[1;1H\x1b[3L".as_slice(),
            3,
            "soft-wrap boundary",
        ),
    ] {
        let mut source = TerminalProbe::new(size, 64);
        source.feed(sequence);
        assert!(
            source.term.grid()[Line(row)][Column(7)]
                .flags
                .contains(Flags::WRAPLINE)
        );
        let before = source.restore_fingerprint();
        let restore = source.restore_screen_bounded(1_000_000).unwrap();
        assert!(
            restore
                .approximations
                .iter()
                .any(|gap| gap.contains(reason))
        );
        assert_eq!(source.restore_fingerprint(), before);
    }
}

#[test]
#[allow(clippy::unwrap_used)]
fn alternate_soft_wrap_drawing_cannot_scroll_inherited_partial_margins() {
    let size = pulse_core::TermSize::new(24, 6).unwrap();
    for preserve_history in [false, true] {
        let mut source = TerminalProbe::new(size, 128);
        source.feed(b"q\x1b[?1049h\x1b[6;22Ha123456789012345678901234\x1b[2;5r");
        let before = source.restore_fingerprint();
        let restore = if preserve_history {
            source.restore_screen_bounded(1_000_000).unwrap()
        } else {
            source.restore_sequence().unwrap()
        };
        assert!(restore.approximations.is_empty());
        assert_eq!(source.restore_fingerprint(), before);
        let mut fresh = TerminalProbe::new(size, 128);
        fresh.feed(&restore.bytes);
        assert_eq!(fresh.restore_fingerprint(), before);
        assert!(fresh.take_replies().is_empty());
        for suffix in [
            b"Z".as_slice(),
            b"\x1b[?1049l\x1b[T".as_slice(),
            b"\x1b[?1049h\x1b[6;1Hz".as_slice(),
        ] {
            source.feed(suffix);
            fresh.feed(suffix);
            assert_eq!(fresh.restore_fingerprint(), source.restore_fingerprint());
            assert_eq!(fresh.take_replies(), source.take_replies());
        }
    }
}
