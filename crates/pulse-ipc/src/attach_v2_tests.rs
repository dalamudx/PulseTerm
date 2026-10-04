#![allow(clippy::unwrap_used)]
use super::*;
fn boundary() -> Boundary {
    Boundary {
        columns: 80,
        rows: 24,
        revision: 1,
        source_bytes: (1 << 60) + 33,
        utf8_bytes: (1 << 60) + 43,
    }
}
fn sender() -> AttachmentSender {
    AttachmentSender::new(7, boundary(), vec![], b"screen".to_vec(), 3, 1, 64).unwrap()
}
fn ack(s: &mut AttachmentSender, wire: &[u8]) {
    s.acknowledge(7, crate::decode(wire).unwrap().sequence)
        .unwrap();
}
fn commit(s: &mut AttachmentSender) {
    while let Some(w) = s.next_frame().unwrap() {
        assert!(
            matches!(
                s.next_frame(),
                Err(AttachError::Flow(crate::flow::FlowError::NoCredit))
            ) || s.phase == Part::Data
        );
        ack(s, &w);
    }
}
#[test]
fn begin_preserves_two_independent_u64_coordinates_and_version() {
    let mut s = sender();
    let w = s.next_frame().unwrap().unwrap();
    let frame = crate::decode(&w).unwrap();
    let fragment = decode_fragment(frame.payload).unwrap();
    assert_eq!(decode_begin(fragment.body).unwrap().0, boundary());
    assert!(crate::attach::decode_fragment(frame.payload).is_err());
    let mut bad = frame.payload.to_vec();
    bad[0] = 1;
    assert!(decode_fragment(&bad).is_err());
    bad[0] = 3;
    assert!(decode_fragment(&bad).is_err());
    bad[0] = 2;
    bad[20] ^= 1;
    assert!(decode_fragment(&bad).is_err());
}
#[test]
fn source_only_marker_and_expanded_data_keep_credit_and_offsets_independent() {
    let mut s = sender();
    commit(&mut s);
    let mut b = boundary();
    b.source_bytes += 1;
    b.revision += 1;
    let marker = s.marker(b).unwrap();
    let f = crate::decode(&marker).unwrap();
    let p = decode_fragment(f.payload).unwrap();
    assert_eq!(p.offset, b.utf8_bytes);
    assert_eq!(p.source_bytes, b.source_bytes);
    assert_eq!(s.outstanding_bytes(), ENVELOPE_LEN + 8);
    assert_eq!(s.acknowledge(8, f.sequence), Err(AttachError::Generation));
    assert!(matches!(
        s.data("中".as_bytes(), b.source_bytes + 1),
        Err(AttachError::Flow(crate::flow::FlowError::NoCredit))
    ));
    assert_eq!(s.boundary, b);
    ack(&mut s, &marker);
    let wire = s.data("中".as_bytes(), b.source_bytes + 1).unwrap();
    let p = decode_fragment(crate::decode(&wire).unwrap().payload).unwrap();
    assert_eq!(p.offset, b.utf8_bytes);
    assert_eq!(p.source_bytes, b.source_bytes + 1);
    assert_eq!(p.body, "中".as_bytes());
    ack(&mut s, &wire);
    b.source_bytes += 1;
    b.utf8_bytes += 3;
    assert!(s.finish(b).is_ok());
    assert!(s.data(b"x", b.source_bytes + 1).is_err());
}
#[test]
fn backward_source_and_unmatched_display_end_are_rejected() {
    let mut s = sender();
    commit(&mut s);
    let b = boundary();
    assert!(s.data(b"x", b.source_bytes - 1).is_err());
    assert!(
        s.finish(Boundary {
            utf8_bytes: b.utf8_bytes + 1,
            ..b
        })
        .is_err()
    );
    assert!(
        s.finish(Boundary {
            source_bytes: b.source_bytes + 1,
            ..b
        })
        .is_err()
    );
    assert!(
        s.marker(Boundary {
            source_bytes: b.source_bytes - 1,
            ..b
        })
        .is_err()
    );
    assert_eq!(s.outstanding_bytes(), 0);
    assert!(s.finish(b).is_ok());
}
#[test]
fn repair_requires_both_cursors_and_is_atomic_under_pressure() {
    let mut s = sender();
    commit(&mut s);
    let b = boundary();
    assert!(
        s.start_repair(
            Boundary {
                source_bytes: b.source_bytes + 1,
                ..b
            },
            vec![1]
        )
        .is_err()
    );
    assert!(
        s.start_repair(
            Boundary {
                utf8_bytes: b.utf8_bytes + 1,
                ..b
            },
            vec![1]
        )
        .is_err()
    );
    s.start_repair(b, b"redraw".to_vec()).unwrap();
    assert!(s.data(b"x", b.source_bytes).is_err());
    let mut recovered = Vec::new();
    while let Some(w) = s.next_repair().unwrap() {
        let frame = crate::decode(&w).unwrap();
        let p = decode_fragment(frame.payload).unwrap();
        assert_eq!(p.source_bytes, b.source_bytes);
        if p.part == Part::RepairBytes {
            recovered.extend_from_slice(p.body);
        } else {
            assert_eq!(p.offset, b.utf8_bytes);
        }
        if s.repair.is_some() {
            assert!(matches!(
                s.next_repair(),
                Err(AttachError::Flow(crate::flow::FlowError::NoCredit))
            ));
        }
        ack(&mut s, &w);
    }
    assert_eq!(recovered, b"redraw");
    assert!(s.finish(b).is_ok());
}
#[test]
fn shared_fixture_is_generated_byte_for_byte_with_independent_coordinates() {
    let b = boundary();
    let mut s = AttachmentSender::new((1 << 60) + 7, b, vec![], b"A".to_vec(), 3, 1, 64).unwrap();
    let mut wires = Vec::new();
    while let Some(w) = s.next_frame().unwrap() {
        s.acknowledge((1 << 60) + 7, crate::decode(&w).unwrap().sequence)
            .unwrap();
        wires.push(w);
    }
    let mut next = Boundary {
        source_bytes: b.source_bytes + 1,
        revision: 2,
        ..b
    };
    let w = s.marker(next).unwrap();
    s.acknowledge((1 << 60) + 7, crate::decode(&w).unwrap().sequence)
        .unwrap();
    wires.push(w);
    let w = s.data("中".as_bytes(), b.source_bytes + 2).unwrap();
    s.acknowledge((1 << 60) + 7, crate::decode(&w).unwrap().sequence)
        .unwrap();
    wires.push(w);
    next.source_bytes += 1;
    next.utf8_bytes += 3;
    next.revision = 3;
    s.start_repair(next, b"B".to_vec()).unwrap();
    while let Some(w) = s.next_repair().unwrap() {
        s.acknowledge((1 << 60) + 7, crate::decode(&w).unwrap().sequence)
            .unwrap();
        wires.push(w);
    }
    next.revision = 4;
    wires.push(s.finish(next).unwrap());
    let expected: Vec<Vec<u8>> = include_str!("../../../tests/fixtures/frames/attachment-v2.hex")
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|hex| u8::from_str_radix(hex, 16).unwrap())
                .collect()
        })
        .collect();
    assert_eq!(wires, expected);
}

#[test]
fn fragment_limits_precede_body_copy_and_commit_releases_bootstrap_buffers() {
    let oversized = vec![0; MAX_FRAGMENT + 1];
    assert_eq!(
        Fragment {
            part: Part::Data,
            generation: 7,
            offset: 0,
            source_bytes: 0,
            body: &oversized
        }
        .encode(),
        Err(AttachError::Invalid)
    );
    let mut s = sender();
    commit(&mut s);
    assert_eq!(s.history.capacity(), 0);
    assert_eq!(s.restore.capacity(), 0);
}

#[test]
fn utf8_cursor_overflow_is_checked_before_reserving_credit() {
    let mut s = AttachmentSender::new(
        7,
        Boundary {
            utf8_bytes: u64::MAX,
            ..boundary()
        },
        vec![],
        vec![1],
        3,
        1,
        64,
    )
    .unwrap();
    commit(&mut s);
    assert_eq!(
        s.data(b"x", boundary().source_bytes),
        Err(AttachError::Overflow)
    );
    assert_eq!(s.outstanding_bytes(), 0);
}
