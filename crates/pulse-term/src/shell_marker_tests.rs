use super::*;
use crate::TerminalProbe;
use pulse_core::TermSize;
fn terminal() -> Result<TerminalProbe, Box<dyn std::error::Error>> {
    Ok(TerminalProbe::new(TermSize::new(40, 8)?, 10))
}
#[test]
fn bel_and_st_every_chunk_boundary_preserve_order_and_optional_status()
-> Result<(), Box<dyn std::error::Error>> {
    let data = b"\x1b]133;A\x07\x1b]133;B\x1b\\\x1b]133;C\x07\x1b]133;D;255\x1b\\\x1b]133;D\x07";
    let expected = vec![
        ShellMarker::PromptStart,
        ShellMarker::PromptEnd,
        ShellMarker::CommandStart,
        ShellMarker::CommandEnd {
            exit_status: Some(255),
        },
        ShellMarker::CommandEnd { exit_status: None },
    ];
    for split in 0..=data.len() {
        let mut term = terminal()?;
        term.feed(&data[..split]);
        term.feed(&data[split..]);
        assert_eq!(
            term.take_shell_markers(),
            ShellMarkerBatch {
                markers: expected.clone(),
                dropped: 0
            }
        );
        assert!(term.take_shell_markers().markers.is_empty());
    }
    Ok(())
}
#[test]
fn incomplete_canceled_malformed_and_other_osc_are_not_shell_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let mut term = terminal()?;
    for body in [
        b"133;A;extra".as_slice(),
        b"133;D;-1",
        b"133;D;256",
        b"133;D;9999",
        b"133;D;",
        b"133;D; 0",
        b"133;A\x7f",
        b"633;A",
        b"52;c;133;A",
        b"133;D;secret",
    ] {
        term.feed(b"\x1b]");
        term.feed(body);
        term.feed(b"\x07");
    }
    term.feed(b"\x1b]133;A\x18\x1b]133;B\x1a\x1b]133;C\x1b[0m\x1b]133;A\x1b");
    assert!(term.take_shell_markers().markers.is_empty());
    term.feed(b"\\");
    assert_eq!(
        term.take_shell_markers().markers,
        [ShellMarker::PromptStart]
    );
    term.feed(b"\x1b]133;B");
    term.finish_input();
    assert!(term.take_shell_markers().markers.is_empty());
    Ok(())
}
#[test]
fn ignored_c0_controls_match_vte_but_osc_del_is_retained() -> Result<(), Box<dyn std::error::Error>>
{
    let mut term = terminal()?;
    term.feed(b"\x1b]13\x00\n3;A\x07\x1b]133;A\x7f\x07");
    assert_eq!(
        term.take_shell_markers().markers,
        [ShellMarker::PromptStart]
    );
    Ok(())
}
#[test]
fn queue_and_oversize_sequence_are_bounded_and_loss_is_explicit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut term = terminal()?;
    for _ in 0..MAX_SHELL_MARKERS + 7 {
        term.feed(b"\x1b]133;A\x07");
    }
    let batch = term.take_shell_markers();
    assert_eq!(batch.markers.len(), MAX_SHELL_MARKERS);
    assert_eq!(batch.dropped, 7);
    assert_eq!(term.take_shell_markers().dropped, 0);
    term.feed(b"\x1b]133;A");
    term.feed(&vec![b'0'; crate::ground::MAX_PENDING + 1]);
    term.feed(b"\x07\x1b]133;B\x07");
    assert_eq!(term.take_shell_markers().markers, [ShellMarker::PromptEnd]);
    Ok(())
}
proptest::proptest! {
    #[test]
    fn observer_matches_real_vte_osc_callbacks(tokens in proptest::collection::vec(0usize..16, 0..120)) {
        use alacritty_terminal::vte::{Parser, Perform};
        const TOKENS: [&[u8]; 16] = [b"\x1b]133;A", b"\x1b]133;B", b"\x1b]133;D;1", b"\x1b]133;C", b"\x07", b"\x1b\\", b"\x18", b"\x1a", b"\x7f", b"\x00\n", b";extra", b"\x1bPabc", b"\x1b[0m", b"\x1b]52;c;", b"abc", b"\x1b"];
        #[derive(Default)]
        struct Oracle { byte: u8, pending: Option<ShellMarker>, events: Vec<ShellMarker> }
        impl Perform for Oracle {
            fn osc_dispatch(&mut self, params: &[&[u8]], _bell: bool) {
                let raw = params.join(&b';');
                if let Some(marker) = parse(&raw) {
                    if self.byte == 0x07 { self.events.push(marker); }
                    else if self.byte == 0x1b { self.pending = Some(marker); }
                }
            }
        }
        let mut parser = Parser::new();
        let mut oracle = Oracle::default();
        let mut scanner = crate::ground::GroundScanner::default();
        let mut queue = MarkerQueue::default();
        for byte in tokens.iter().flat_map(|index| TOKENS[*index].iter()).copied() {
            if let Some(marker) = oracle.pending.take() && byte == b'\\' { oracle.events.push(marker); }
            oracle.byte = byte;
            parser.advance(&mut oracle, &[byte]);
            scanner.feed_with_osc(&[byte], |byte, body| queue.observe(byte, body));
        }
        proptest::prop_assert_eq!(queue.take().markers, oracle.events);
    }
}

#[test]
fn markers_do_not_change_screen_replies_or_recovery_risk() -> Result<(), Box<dyn std::error::Error>>
{
    let mut marked = terminal()?;
    let mut plain = terminal()?;
    marked.side.risk = 1;
    plain.side.risk = 1;
    marked.feed(b"abc\x1b]133;A\x07\x1b]133;D;0\x1b\\\x1b[6n");
    plain.feed(b"abc\x1b[6n");
    assert_eq!(marked.screen(), plain.screen());
    assert_eq!(marked.take_replies(), plain.take_replies());
    assert_eq!(marked.divergence_risk(), plain.divergence_risk());
    assert_eq!(marked.divergence_risk(), 1);
    assert_eq!(marked.take_shell_markers().markers.len(), 2);
    Ok(())
}
