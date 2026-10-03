use super::*;
#[test]
#[allow(clippy::unwrap_used)]
fn shared_fixture_matches_encoder_byte_for_byte() {
    let epoch = (1u64 << 60) + 7;
    let mut sender = AttachmentSender::new(
        epoch,
        Boundary {
            columns: 32,
            rows: 8,
            revision: u64::MAX - 4,
            parsed_bytes: 9,
        },
        b"older\r\n".to_vec(),
        b"SCREEN".to_vec(),
        3,
        1,
        100,
    )
    .unwrap();
    let mut frames = Vec::new();
    while let Some(frame) = sender.next_frame().unwrap() {
        let seq = crate::decode(&frame).unwrap().sequence;
        sender.acknowledge(epoch, seq).unwrap();
        frames.push(frame);
    }
    frames.push(sender.data(b"Z").unwrap());
    let fixture: Vec<Vec<u8>> = include_str!("../../../tests/fixtures/frames/attachment.hex")
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|v| u8::from_str_radix(v, 16).unwrap())
                .collect()
        })
        .collect();
    assert_eq!(frames, fixture);
}

#[allow(clippy::unwrap_used)]
fn sender(frames: usize) -> AttachmentSender {
    AttachmentSender::new(
        7,
        Boundary {
            columns: 32,
            rows: 8,
            revision: u64::MAX - 4,
            parsed_bytes: 9,
        },
        b"older\r\n".to_vec(),
        b"\x1bcSCREEN".to_vec(),
        3,
        frames,
        100,
    )
    .unwrap()
}
#[test]
#[allow(clippy::unwrap_used)]
fn protocol_order_offsets_and_credit_are_exact() {
    let mut sender = sender(1);
    let mut kinds = Vec::new();
    let mut history = Vec::new();
    let mut restore = Vec::new();
    assert_eq!(sender.data(b"x"), Err(AttachError::Phase));
    while let Some(wire) = sender.next_frame().unwrap() {
        let frame = crate::decode(&wire).unwrap();
        let part = decode_fragment(frame.payload).unwrap();
        kinds.push(part.part);
        match part.part {
            Part::History => {
                assert_eq!(part.offset, history.len() as u64);
                history.extend_from_slice(part.body);
            }
            Part::Restore => {
                assert_eq!(part.offset, restore.len() as u64);
                restore.extend_from_slice(part.body);
            }
            Part::Begin => {
                assert_eq!(decode_begin(part.body).unwrap().0.revision, u64::MAX - 4);
            }
            _ => {}
        }
        assert!(sender.outstanding_bytes() <= 100);
        if part.part != Part::Commit {
            assert_eq!(
                sender.next_frame(),
                Err(AttachError::Flow(FlowError::NoCredit))
            );
        }
        assert_eq!(
            sender.acknowledge(8, frame.sequence),
            Err(AttachError::Generation)
        );
        sender.acknowledge(7, frame.sequence).unwrap();
        assert_eq!(
            sender.acknowledge(7, frame.sequence),
            Err(AttachError::Flow(FlowError::InvalidAck))
        );
    }
    assert_eq!(kinds.first(), Some(&Part::Begin));
    assert_eq!(kinds.last(), Some(&Part::Commit));
    assert_eq!(history, b"older\r\n");
    assert_eq!(restore, b"\x1bcSCREEN");
    for (data, offset) in [(b"x".as_slice(), 9), (b"yz".as_slice(), 10)] {
        let wire = sender.data(data).unwrap();
        let frame = crate::decode(&wire).unwrap();
        let part = decode_fragment(frame.payload).unwrap();
        assert_eq!(part.part, Part::Data);
        assert_eq!(part.offset, offset);
        sender.acknowledge(7, frame.sequence).unwrap();
    }
}
#[test]
#[allow(clippy::unwrap_used)]
fn rejects_invalid_fragment_and_counter_overflow_without_credit_change() {
    let mut sender = sender(8);
    while let Some(wire) = sender.next_frame().unwrap() {
        let frame = crate::decode(&wire).unwrap();
        sender.acknowledge(7, frame.sequence).unwrap();
    }
    let credit = sender.outstanding_bytes();
    assert_eq!(sender.data(&[1, 2, 3, 4]), Err(AttachError::Invalid));
    assert_eq!(sender.data(&[]), Err(AttachError::Invalid));
    sender.live_offset = u64::MAX;
    assert_eq!(sender.data(&[1]), Err(AttachError::Overflow));
    assert_eq!(sender.outstanding_bytes(), credit);
}
#[test]
#[allow(clippy::unwrap_used)]
fn empty_history_and_allocation_limits() {
    let b = Boundary {
        columns: 32,
        rows: 8,
        revision: 0,
        parsed_bytes: 0,
    };
    for (generation, fragment, history, restore) in [
        (0, 1, 0, 1),
        (1, 0, 0, 1),
        (1, MAX_FRAGMENT + 1, 0, 1),
        (1, 1, MAX_HISTORY + 1, 1),
        (1, 1, 0, 0),
        (1, 1, 0, MAX_RESTORE + 1),
    ] {
        assert!(
            AttachmentSender::new(
                generation,
                b,
                vec![0; history],
                vec![0; restore],
                fragment,
                1,
                ENVELOPE_LEN + MAX_FRAGMENT
            )
            .is_err()
        );
    }
    let mut sender = AttachmentSender::new(1, b, vec![], vec![1], 8, 8, 1024).unwrap();
    let mut parts = Vec::new();
    while let Some(wire) = sender.next_frame().unwrap() {
        parts.push(
            decode_fragment(crate::decode(&wire).unwrap().payload)
                .unwrap()
                .part,
        );
    }
    assert_eq!(parts, [Part::Begin, Part::Restore, Part::Commit]);
}
proptest::proptest! {
    #[test]
    fn malformed_fragment_never_panics(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..1024)){
        let _=decode_fragment(&bytes);
    }
}
