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
