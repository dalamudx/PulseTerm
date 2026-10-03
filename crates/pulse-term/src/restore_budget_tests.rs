use super::*;
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
