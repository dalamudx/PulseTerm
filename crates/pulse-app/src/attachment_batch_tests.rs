#![allow(clippy::unwrap_used)]
use super::*;
use pulse_core::TermSize;
use pulse_term::session::{Lifecycle, SessionState};
fn state(end: u64) -> SessionState {
    SessionState {
        lifecycle: Lifecycle::Running,
        size: TermSize::new(80, 24).unwrap(),
        parsed_bytes: end / 2,
        utf8_bytes: end,
        source_is_utf8: false,
        revision: end,
    }
}
fn data(start: u64, bytes: &[u8]) -> StreamRecord {
    StreamRecord::Data {
        start,
        bytes: bytes.to_vec(),
        boundary: state(start + bytes.len() as u64),
    }
}
fn body(record: &StreamRecord) -> (&[u8], &SessionState) {
    let StreamRecord::Data {
        bytes, boundary, ..
    } = record
    else {
        panic!("not Data")
    };
    (bytes, boundary)
}
#[test]
fn adjacent_utf8_fragments_keep_exact_bytes_and_last_source_boundary() {
    let (tx, mut rx) = mpsc::channel(8);
    tx.try_send(data(1, &[0xb8, 0xad])).unwrap();
    tx.try_send(data(3, b"tail")).unwrap();
    let mut first = data(0, &[0xe4]);
    let mut next = None;
    coalesce(&mut first, &mut rx, &mut next, 64).unwrap();
    assert_eq!(body(&first).0, "中tail".as_bytes());
    assert_eq!(body(&first).1, &state(7));
    assert!(next.is_none());
    assert!(rx.is_empty());
}
#[test]
fn barriers_and_oversize_records_remain_ordered_without_waiting() {
    for barrier in [
        StreamRecord::Marker(state(1)),
        StreamRecord::Fence(state(1)),
        StreamRecord::Repair {
            boundary: state(1),
            bytes: vec![0x18],
            hold: None,
        },
        data(1, b"toolarge"),
    ] {
        let (tx, mut rx) = mpsc::channel(8);
        tx.try_send(barrier).unwrap();
        tx.try_send(data(8, b"later")).unwrap();
        let mut first = data(0, b"a");
        let mut next = None;
        coalesce(&mut first, &mut rx, &mut next, 4).unwrap();
        assert_eq!(body(&first).0, b"a");
        assert!(next.is_some());
        assert_eq!(rx.len(), 1);
        coalesce(&mut first, &mut rx, &mut next, 64).unwrap();
        assert_eq!(rx.len(), 1);
    }
}
#[test]
fn fixed_fragment_cap_leaves_unconsumed_records_in_the_queue() {
    let (tx, mut rx) = mpsc::channel(8);
    for start in 1..=8 {
        tx.try_send(data(start, b"x")).unwrap();
    }
    let mut first = data(0, b"x");
    let mut next = None;
    coalesce(&mut first, &mut rx, &mut next, 4).unwrap();
    assert_eq!(body(&first).0, b"xxxx");
    assert_eq!(body(&first).1.utf8_bytes, 4);
    assert!(next.is_none());
    assert_eq!(rx.len(), 5);
}
#[test]
fn coordinate_gaps_and_regressions_cannot_be_hidden_by_batching() {
    for mode in 0..5 {
        let mut bad = data(1, b"b");
        let StreamRecord::Data {
            start, boundary, ..
        } = &mut bad
        else {
            unreachable!()
        };
        match mode {
            0 => *start = 2,
            1 => boundary.utf8_bytes = 3,
            2 => boundary.parsed_bytes = 0,
            3 => boundary.revision = 0,
            _ => boundary.size = TermSize::new(81, 24).unwrap(),
        }
        let (tx, mut rx) = mpsc::channel(8);
        tx.try_send(bad).unwrap();
        let mut first = data(0, b"a");
        if let StreamRecord::Data { boundary, .. } = &mut first {
            boundary.parsed_bytes = 1;
        }
        assert!(coalesce(&mut first, &mut rx, &mut None, 64).is_err());
        assert_eq!(body(&first).0, b"a");
    }
}
#[test]
fn dropping_retained_repair_releases_its_original_single_flight_permit() {
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let hold = semaphore.clone().try_acquire_owned().unwrap();
    let (tx, mut rx) = mpsc::channel(8);
    tx.try_send(StreamRecord::Repair {
        boundary: state(1),
        bytes: vec![0x18],
        hold: Some(hold),
    })
    .unwrap();
    let mut next = None;
    coalesce(&mut data(0, b"a"), &mut rx, &mut next, 64).unwrap();
    assert_eq!(semaphore.available_permits(), 0);
    drop(next);
    assert_eq!(semaphore.available_permits(), 1);
}
